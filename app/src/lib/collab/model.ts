// The shared document for co-writing, and the reconciliation between it and
// this writer's project folder.
//
// Y.Doc layout (MODEL_VERSION 1):
//   info:   Y.Map  { v: 1, title }
//   acts:   Y.Map  "<episode>:<act>" (episode empty outside a series) -> true
//   scenes: Y.Map  id -> Y.Map { act: number, episode: number|null, order: number, trashed: boolean,
//                                meta: Y.Map<key, JSON>, body: Y.Text }
//   files:  Y.Map  name -> Y.Text            (project.yaml, characters.yaml, …)
//
// Each writer keeps ordinary files on disk. A reconcile pass:
//   1. folds edits made OUTSIDE the app into the doc (three-way, against `base`,
//      the content this writer last wrote), then
//   2. plans the file operations that make the folder match the doc.
// Everything here is pure (no API calls) so it can be unit tested.

import * as Y from "yjs";
import DiffMatchPatch from "diff-match-patch";
import type { ProjectFile, ProjectView, SceneMeta } from "../api";

export const MODEL_VERSION = 1;

const META_KEYS = [
  "title",
  "status",
  "location",
  "duration",
  "onstage_silent",
  "synopsis",
  "notes",
  "tags",
  "ratings",
] as const;
export type MetaKey = (typeof META_KEYS)[number];
export type SharedMeta = Partial<Record<MetaKey, unknown>>;

export interface DiskScene {
  id: string;
  act: number;
  episode?: number | null;
  meta: SceneMeta;
  body: string;
  version: string;
}

export interface DiskSnapshot {
  view: ProjectView;
  scenes: DiskScene[];
  files: ProjectFile[];
}

/** What this writer last wrote to disk (or last saw there). */
export interface Base {
  scenes: Record<string, { body: string; meta: SharedMeta }>;
  files: Record<string, string>;
  /**
   * Act folders (`actKey`s) the disk and the doc last agreed on. An act on disk
   * but not in the doc is new on disk if it isn't listed here, and was deleted
   * by a writer if it is. Absent in bases saved before acts could be deleted.
   */
  acts?: string[];
}

export const emptyBase = (): Base => ({ scenes: {}, files: {}, acts: [] });

const dmp = new DiffMatchPatch();

// ------------------------------------------------------------------ helpers

export function pickMeta(meta: Partial<SceneMeta> | Record<string, unknown>): SharedMeta {
  const out: SharedMeta = {};
  const m = meta as Record<string, unknown>;
  for (const k of META_KEYS) {
    const v = m[k];
    // Normalise "absent" values so disk and doc compare equal.
    if (v === undefined || v === null) out[k] = null;
    else if (Array.isArray(v) && v.length === 0 && (k === "tags" || k === "onstage_silent")) out[k] = [];
    else out[k] = v;
  }
  if (out.tags === null) out.tags = [];
  if (out.onstage_silent === null) out.onstage_silent = [];
  if (out.ratings === null) out.ratings = {};
  return out;
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(sortKeys(a)) === JSON.stringify(sortKeys(b));
}

function sortKeys(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(sortKeys);
  if (v && typeof v === "object") {
    return Object.fromEntries(
      Object.keys(v as object)
        .sort()
        .map((k) => [k, sortKeys((v as Record<string, unknown>)[k])]),
    );
  }
  return v;
}

/** Replace `ytext`'s content with `next` using minimal edits (keeps others' cursors stable). */
export function applyText(ytext: Y.Text, next: string) {
  const cur = ytext.toString();
  if (cur === next) return;
  const diffs = dmp.diff_main(cur, next);
  dmp.diff_cleanupEfficiency(diffs);
  let pos = 0;
  for (const [op, text] of diffs) {
    if (op === 0) pos += text.length;
    else if (op === -1) ytext.delete(pos, text.length);
    else {
      ytext.insert(pos, text);
      pos += text.length;
    }
  }
}

/**
 * Three-way merge of plain text. `base` is the common ancestor, `theirs` the
 * outside edit, `ours` the shared document. Non-overlapping changes both survive.
 */
export function mergeText(base: string, theirs: string, ours: string): string {
  if (theirs === base) return ours;
  if (ours === base) return theirs;
  if (theirs === ours) return ours;
  const patches = dmp.patch_make(base, theirs);
  const [merged] = dmp.patch_apply(patches, ours);
  return merged;
}

// ------------------------------------------------------------------ reading the doc

interface SharedScene {
  id: string;
  act: number;
  episode: number | null;
  order: number;
  trashed: boolean;
  meta: SharedMeta;
  body: string;
}

export function scenesMap(doc: Y.Doc): Y.Map<Y.Map<unknown>> {
  return doc.getMap("scenes") as Y.Map<Y.Map<unknown>>;
}

export function sceneBody(doc: Y.Doc, id: string): Y.Text | null {
  return (scenesMap(doc).get(id)?.get("body") as Y.Text | undefined) ?? null;
}

export function readScene(doc: Y.Doc, id: string): SharedScene | null {
  const s = scenesMap(doc).get(id);
  if (!s) return null;
  const meta = s.get("meta") as Y.Map<unknown> | undefined;
  return {
    id,
    act: Number(s.get("act")),
    episode: (s.get("episode") as number | null | undefined) ?? null,
    order: Number(s.get("order")),
    trashed: !!s.get("trashed"),
    meta: pickMeta(meta ? (meta.toJSON() as Record<string, unknown>) : {}),
    body: (s.get("body") as Y.Text | undefined)?.toString() ?? "",
  };
}

interface ActKey {
  episode: number | null;
  act: number;
}

export const actKey = (episode: number | null | undefined, act: number) => `${episode ?? ""}:${act}`;

function parseActKey(k: string): ActKey {
  if (!k.includes(":")) return { episode: null, act: Number(k) }; // older docs
  const [e, a] = k.split(":");
  return { episode: e === "" ? null : Number(e), act: Number(a) };
}

interface SharedState {
  acts: ActKey[];
  scenes: SharedScene[];
  files: Record<string, string>;
  title: string | null;
}

export function readDoc(doc: Y.Doc): SharedState {
  const acts = [...(doc.getMap("acts") as Y.Map<boolean>).keys()]
    .map(parseActKey)
    .sort((a, b) => (a.episode ?? 0) - (b.episode ?? 0) || a.act - b.act);
  const scenes = [...scenesMap(doc).keys()].map((id) => readScene(doc, id)!).filter(Boolean);
  const files: Record<string, string> = {};
  (doc.getMap("files") as Y.Map<Y.Text>).forEach((t, name) => (files[name] = t.toString()));
  return { acts, scenes, files, title: (doc.getMap("info").get("title") as string) ?? null };
}

/** Live scenes of `act` (in `episode`, for a series) in running order. */
export function actOrder(state: SharedState, act: number, episode: number | null = null): string[] {
  return state.scenes
    .filter((s) => !s.trashed && s.act === act && s.episode === episode)
    .sort((a, b) => a.order - b.order || (a.id < b.id ? -1 : 1))
    .map((s) => s.id);
}

// ------------------------------------------------------------------ writing the doc

function newScene(act: number, episode: number | null, order: number, meta: SharedMeta, body: string): Y.Map<unknown> {
  const m = new Y.Map<unknown>();
  m.set("act", act);
  m.set("episode", episode);
  m.set("order", order);
  m.set("trashed", false);
  const ym = new Y.Map<unknown>();
  for (const [k, v] of Object.entries(meta)) ym.set(k, v);
  m.set("meta", ym);
  m.set("body", new Y.Text(body));
  return m;
}

export function addScene(
  doc: Y.Doc,
  id: string,
  act: number,
  order: number,
  meta: SharedMeta,
  body = "",
  episode: number | null = null,
) {
  scenesMap(doc).set(id, newScene(act, episode, order, meta, body));
}

/**
 * Delete an act in the doc: its scenes are trashed, and (unless it is act 0)
 * later acts of the same episode move up one. Call inside a transaction.
 */
export function deleteActInDoc(doc: Y.Doc, act: number, episode: number | null) {
  const acts = doc.getMap("acts") as Y.Map<boolean>;
  const st = readDoc(doc);
  for (const s of st.scenes) {
    if (s.episode !== episode || s.trashed) continue;
    const m = scenesMap(doc).get(s.id)!;
    if (s.act === act) m.set("trashed", true);
    else if (act >= 1 && s.act > act) m.set("act", s.act - 1);
  }
  const mine = st.acts.filter((a) => a.episode === episode).map((a) => a.act);
  for (const a of mine) acts.delete(actKey(episode, a));
  for (const a of mine) {
    if (a === act) continue;
    acts.set(actKey(episode, act >= 1 && a > act ? a - 1 : a), true);
  }
}

/** Delete an episode in the doc: its scenes are trashed, later episodes move up. */
export function deleteEpisodeInDoc(doc: Y.Doc, episode: number) {
  const acts = doc.getMap("acts") as Y.Map<boolean>;
  const st = readDoc(doc);
  for (const s of st.scenes) {
    if (s.episode === null || s.trashed) continue;
    const m = scenesMap(doc).get(s.id)!;
    if (s.episode === episode) m.set("trashed", true);
    else if (s.episode > episode) m.set("episode", s.episode - 1);
  }
  for (const a of st.acts) if (a.episode !== null && a.episode >= episode) acts.delete(actKey(a.episode, a.act));
  for (const a of st.acts) if (a.episode !== null && a.episode > episode) acts.set(actKey(a.episode - 1, a.act), true);
}

export function setMeta(doc: Y.Doc, id: string, patch: SharedMeta) {
  const meta = scenesMap(doc).get(id)?.get("meta") as Y.Map<unknown> | undefined;
  if (!meta) return;
  doc.transact(() => {
    for (const [k, v] of Object.entries(patch)) if (!same(meta.get(k), v)) meta.set(k, v);
  });
}

export function setFile(doc: Y.Doc, name: string, text: string) {
  const files = doc.getMap("files") as Y.Map<Y.Text>;
  const t = files.get(name);
  if (t) applyText(t, text);
  else files.set(name, new Y.Text(text));
}

/** Order value placing a scene at `index` among `ids` (which excludes it). */
export function orderAt(state: SharedState, ids: string[], index: number): number {
  const ord = (id: string) => state.scenes.find((s) => s.id === id)!.order;
  const before = index > 0 ? ord(ids[index - 1]) : null;
  const after = index < ids.length ? ord(ids[index]) : null;
  if (before === null && after === null) return 0;
  if (before === null) return after! - 1;
  if (after === null) return before + 1;
  return (before + after) / 2;
}

/** Fill an empty doc from this writer's folder (the writer who shares first). */
export function seedFromDisk(doc: Y.Doc, snap: DiskSnapshot) {
  doc.transact(() => {
    doc.getMap("info").set("v", MODEL_VERSION);
    doc.getMap("info").set("title", snap.view.title);
    const acts = doc.getMap("acts");
    for (const a of snap.view.acts) acts.set(actKey(a.episode, a.number), true);
    const byId = new Map(snap.scenes.map((s) => [s.id, s]));
    for (const act of snap.view.acts) {
      act.scenes.forEach((summary, i) => {
        const s = byId.get(summary.id)!;
        addScene(doc, s.id, act.number, i, pickMeta(s.meta), s.body, act.episode ?? null);
      });
    }
    for (const f of snap.files) setFile(doc, f.name, f.text);
  });
}

export function baseFromDisk(snap: DiskSnapshot): Base {
  const base = emptyBase();
  for (const s of snap.scenes) base.scenes[s.id] = { body: s.body, meta: pickMeta(s.meta) };
  for (const f of snap.files) base.files[f.name] = f.text;
  base.acts = snap.view.acts.map((a) => actKey(a.episode, a.number));
  return base;
}

// ------------------------------------------------------------------ pass 1: outside edits -> doc

interface FoldReport {
  bodies: string[];
  metas: string[];
  added: string[];
  removed: string[];
  files: string[];
}

/**
 * Bring edits made to files outside the app into the shared doc.
 * A scene or file counts as edited outside when disk differs from `base`.
 */
export function foldExternal(doc: Y.Doc, snap: DiskSnapshot, base: Base): FoldReport {
  const report: FoldReport = { bodies: [], metas: [], added: [], removed: [], files: [] };
  const state = readDoc(doc);
  const onDisk = new Set(snap.scenes.map((s) => s.id));
  doc.transact(() => {
    // Acts new on disk are shared (e.g. a new episode added here); ones a writer
    // deleted from the doc stay deleted, and their folders are removed later.
    const acts = doc.getMap("acts") as Y.Map<boolean>;
    const known = new Set(base.acts ?? []);
    for (const a of snap.view.acts) {
      const k = actKey(a.episode, a.number);
      if (!acts.has(k) && !known.has(k)) acts.set(k, true);
    }
    for (const d of snap.scenes) {
      const shared = state.scenes.find((s) => s.id === d.id);
      const b = base.scenes[d.id];
      const diskMeta = pickMeta(d.meta);
      if (!shared) {
        if (!b) {
          // A scene file that appeared outside the app: share it.
          const ep = d.episode ?? null;
          const order =
            Math.max(-1, ...state.scenes.filter((s) => s.act === d.act && s.episode === ep).map((s) => s.order)) + 1;
          addScene(doc, d.id, d.act, order, diskMeta, d.body, ep);
          acts.set(actKey(ep, d.act), true);
          report.added.push(d.id);
        }
        continue;
      }
      if (!b || shared.trashed) continue;
      if (d.body !== b.body) {
        const ytext = sceneBody(doc, d.id)!;
        applyText(ytext, mergeText(b.body, d.body, ytext.toString()));
        report.bodies.push(d.id);
      }
      const patch: SharedMeta = {};
      for (const k of META_KEYS) if (!same(diskMeta[k], b.meta[k])) patch[k] = diskMeta[k];
      if (Object.keys(patch).length) {
        setMeta(doc, d.id, patch);
        report.metas.push(d.id);
      }
    }
    // Deleted outside the app (we had written it, now it is gone): trash it for everyone.
    for (const s of state.scenes) {
      if (!s.trashed && base.scenes[s.id] && !onDisk.has(s.id)) {
        scenesMap(doc).get(s.id)!.set("trashed", true);
        report.removed.push(s.id);
      }
    }
    for (const f of snap.files) {
      const b = base.files[f.name];
      if (b !== undefined && f.text !== b) {
        const cur = state.files[f.name] ?? "";
        setFile(doc, f.name, mergeText(b, f.text, cur));
        report.files.push(f.name);
      }
    }
  });
  return report;
}

// ------------------------------------------------------------------ pass 2: doc -> disk

export type Op =
  | { kind: "act_ensure"; act: number; episode: number | null }
  | { kind: "act_remove"; act: number; episode: number | null }
  | { kind: "create"; id: string; act: number; episode: number | null; title: string }
  | { kind: "trash"; id: string }
  | { kind: "reorder"; act: number; episode: number | null; ids: string[] }
  | { kind: "meta"; id: string; patch: SharedMeta; version: string }
  | { kind: "body"; id: string; body: string; version: string }
  | { kind: "file"; name: string; text: string; version: string };

/** Every act the doc wants: its act list plus any act a live scene is in. */
function wantedActs(want: SharedState): Map<string, ActKey> {
  const wantActs = new Map<string, ActKey>();
  for (const a of want.acts) wantActs.set(actKey(a.episode, a.act), a);
  for (const s of want.scenes) if (!s.trashed) wantActs.set(actKey(s.episode, s.act), { episode: s.episode, act: s.act });
  return wantActs;
}

/** Structural ops: acts, new scenes, trashed scenes. Run these first, then re-snapshot. */
export function planStructure(snap: DiskSnapshot, want: SharedState): Op[] {
  const ops: Op[] = [];
  const diskActs = new Set(snap.view.acts.map((a) => actKey(a.episode, a.number)));
  const wantActs = wantedActs(want);
  const sorted = [...wantActs.values()].sort((x, y) => (x.episode ?? 0) - (y.episode ?? 0) || x.act - y.act);
  for (const a of sorted) {
    if (!diskActs.has(actKey(a.episode, a.act))) ops.push({ kind: "act_ensure", act: a.act, episode: a.episode });
  }
  const disk = new Map(snap.scenes.map((s) => [s.id, s]));
  for (const s of want.scenes) {
    if (!s.trashed && !disk.has(s.id)) {
      ops.push({ kind: "create", id: s.id, act: s.act, episode: s.episode, title: String(s.meta.title ?? "Untitled") });
    }
    if (s.trashed && disk.has(s.id)) ops.push({ kind: "trash", id: s.id });
  }
  return ops;
}

/** Running order per act, as moves. Run after structure, then re-snapshot. */
export function planOrder(snap: DiskSnapshot, want: SharedState): Op[] {
  const ops: Op[] = [];
  for (const act of snap.view.acts) {
    const episode = act.episode ?? null;
    const target = actOrder(want, act.number, episode);
    const current = act.scenes.map((s) => s.id);
    const disk = new Set(snap.scenes.map((s) => s.id));
    const targetOnDisk = target.filter((id) => disk.has(id));
    if (targetOnDisk.length && JSON.stringify(targetOnDisk) !== JSON.stringify(current)) {
      ops.push({ kind: "reorder", act: act.number, episode, ids: targetOnDisk });
    }
  }
  return ops;
}

/** Last: remove act folders the doc no longer has, once moves have emptied them. */
export function planCleanup(snap: DiskSnapshot, want: SharedState): Op[] {
  const wantActs = wantedActs(want);
  return snap.view.acts
    .filter((a) => !wantActs.has(actKey(a.episode, a.number)) && a.scenes.length === 0)
    .map((a) => ({ kind: "act_remove", act: a.number, episode: a.episode ?? null }) as Op);
}

/** Metadata, text and project files. */
export function planContent(snap: DiskSnapshot, want: SharedState): Op[] {
  const ops: Op[] = [];
  const shared = new Map(want.scenes.map((s) => [s.id, s]));
  for (const d of snap.scenes) {
    const s = shared.get(d.id);
    if (!s || s.trashed) continue;
    const diskMeta = pickMeta(d.meta);
    const patch: SharedMeta = {};
    for (const k of META_KEYS) if (!same(diskMeta[k], s.meta[k])) patch[k] = s.meta[k];
    // A missing title would be refused; keep the disk one.
    if ("title" in patch && (typeof patch.title !== "string" || !patch.title.trim())) delete patch.title;
    if (Object.keys(patch).length) ops.push({ kind: "meta", id: d.id, patch, version: d.version });
    if (d.body !== s.body) ops.push({ kind: "body", id: d.id, body: s.body, version: d.version });
  }
  const diskFiles = new Map(snap.files.map((f) => [f.name, f]));
  for (const [name, text] of Object.entries(want.files)) {
    const f = diskFiles.get(name);
    if (!f || f.text !== text) ops.push({ kind: "file", name, text, version: f?.version ?? EMPTY_VERSION });
  }
  return ops;
}

/** version_of("") in the Rust API (FNV-1a offset basis). */
const EMPTY_VERSION = "cbf29ce484222325";

/** After writing, record as base everything the disk now agrees with the doc on. */
export function updateBase(base: Base, snap: DiskSnapshot, want: SharedState): Base {
  const wantActs = wantedActs(want);
  const next: Base = {
    scenes: { ...base.scenes },
    files: { ...base.files },
    acts: snap.view.acts.map((a) => actKey(a.episode, a.number)).filter((k) => wantActs.has(k)),
  };
  const shared = new Map(want.scenes.map((s) => [s.id, s]));
  const onDisk = new Set<string>();
  for (const d of snap.scenes) {
    onDisk.add(d.id);
    const s = shared.get(d.id);
    const diskMeta = pickMeta(d.meta);
    if (s && d.body === s.body && META_KEYS.every((k) => same(diskMeta[k], s.meta[k]) || (k === "title" && !s.meta.title))) {
      next.scenes[d.id] = { body: d.body, meta: diskMeta };
    }
  }
  for (const id of Object.keys(next.scenes)) if (!onDisk.has(id)) delete next.scenes[id];
  for (const f of snap.files) if (want.files[f.name] === f.text) next.files[f.name] = f.text;
  return next;
}
