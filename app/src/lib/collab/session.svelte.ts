// A live co-writing session: the shared Y.Doc, its connection to the relay,
// presence, local persistence, and the reconcile loop that keeps this writer's
// folder in step with the shared document.

import * as Y from "yjs";
import { WebsocketProvider } from "y-websocket";
import { api, ApiError, type CollabConfig, type ProjectView } from "../api";
import {
  MODEL_VERSION,
  actKey,
  actOrder,
  addScene,
  baseFromDisk,
  emptyBase,
  foldExternal,
  orderAt,
  pickMeta,
  planContent,
  planOrder,
  planStructure,
  readDoc,
  readScene,
  sceneBody,
  scenesMap,
  seedFromDisk,
  setFile,
  setMeta,
  updateBase,
  type Base,
  type DiskSnapshot,
  type Op,
  type SharedMeta,
} from "./model";

export interface Peer {
  clientId: number;
  name: string;
  color: string;
  scene: string | null;
}

export type ConnStatus = "connecting" | "connected" | "offline";

const PERSIST_DELAY_MS = 1200;
const RECONCILE_DELAY_MS = 400;
const POLL_MS = 4000;
const LOCAL = "local"; // transaction origin for edits made in this app's UI
const DISK = "disk"; // origin for edits folded in from files

// ------------------------------------------------------------------ small helpers

export function toBase64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

export function fromBase64(b64: string): Uint8Array {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

export function newRoomId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(18));
  return toBase64(bytes).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function newSceneId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(6));
  return "sc-" + [...bytes].map((b) => b.toString(36).padStart(2, "0")).join("").slice(0, 10);
}

const COLORS = ["#c0392b", "#2f6e74", "#8e44ad", "#b7791f", "#2c7a4b", "#2b5797", "#c2185b", "#5d6d7e"];
export function colorFor(name: string): string {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return COLORS[h % COLORS.length];
}

/** Invite codes carry the relay address, the room (the secret) and the play's title. */
export function inviteCode(c: { relay: string; room: string }, title: string): string {
  const json = JSON.stringify({ r: c.relay, k: c.room, t: title });
  return "sw1." + toBase64(new TextEncoder().encode(json)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function readInvite(code: string): { relay: string; room: string; title: string } {
  const m = code.trim().match(/^sw1\.([A-Za-z0-9_-]+)$/);
  if (!m) throw new Error("That doesn't look like a Stagewright invite code (it starts with sw1.).");
  const b64 = m[1].replace(/-/g, "+").replace(/_/g, "/");
  const json = JSON.parse(new TextDecoder().decode(fromBase64(b64 + "=".repeat((4 - (b64.length % 4)) % 4))));
  if (typeof json.r !== "string" || typeof json.k !== "string") throw new Error("This invite code is incomplete.");
  return { relay: json.r, room: json.k, title: json.t ?? "" };
}

function snapshot(): Promise<DiskSnapshot> {
  return api.projectSnapshot() as Promise<DiskSnapshot>;
}

// ------------------------------------------------------------------ the session

export class CollabSession {
  status = $state<ConnStatus>("connecting");
  synced = $state(false);
  peers = $state<Peer[]>([]);
  problem = $state<string | null>(null);
  /** Bumps whenever the shared doc changes, so views can re-read it. */
  tick = $state(0);

  readonly doc: Y.Doc;
  readonly provider: WebsocketProvider;
  private base: Base;
  private running = false;
  private again = false;
  private stopped = false;
  private reconcileTimer: ReturnType<typeof setTimeout> | null = null;
  private persistTimer: ReturnType<typeof setTimeout> | null = null;
  private poll: ReturnType<typeof setInterval>;
  private undo = new Map<string, Y.UndoManager>();

  private constructor(
    readonly config: CollabConfig,
    doc: Y.Doc,
    provider: WebsocketProvider,
    base: Base,
    private onProject: (view: ProjectView) => void,
    private onShared: () => void,
  ) {
    this.doc = doc;
    this.provider = provider;
    this.base = base;

    provider.awareness.setLocalStateField("user", {
      name: config.name,
      color: config.color,
      colorLight: config.color + "33",
    });
    provider.on("status", (e: { status: string }) => {
      this.status = e.status === "connected" ? "connected" : e.status === "connecting" ? "connecting" : "offline";
    });
    provider.on("sync", (s: boolean) => (this.synced = s));
    provider.on("connection-close", () => (this.status = "offline"));
    provider.on("connection-error", () => (this.status = "offline"));
    if (provider.wsconnected) this.status = "connected";
    if (provider.synced) this.synced = true;
    provider.awareness.on("change", () => this.readPeers());

    doc.on("update", () => {
      this.tick++;
      this.onShared();
      this.schedulePersist();
      this.scheduleReconcile();
    });
    this.poll = setInterval(() => this.scheduleReconcile(), POLL_MS);
    this.scheduleReconcile(0);
  }

  // -------------------------------------------------------------- starting

  private static connect(config: CollabConfig, doc: Y.Doc): WebsocketProvider {
    return new WebsocketProvider(config.relay, config.room, doc, { connect: true, maxBackoffTime: 5000 });
  }

  /** Resume a session saved in the project's .stagewright folder. */
  static async resume(
    config: CollabConfig,
    saved: { state: string | null; base: unknown },
    onProject: (v: ProjectView) => void,
    onShared: () => void,
  ): Promise<CollabSession> {
    const doc = new Y.Doc();
    if (saved.state) Y.applyUpdate(doc, fromBase64(saved.state), DISK);
    const base = (saved.base as Base | null) ?? emptyBase();
    return new CollabSession(config, doc, CollabSession.connect(config, doc), base, onProject, onShared);
  }

  /** Share the open project: seed a new room from this folder. */
  static async share(
    relay: string,
    name: string,
    onProject: (v: ProjectView) => void,
    onShared: () => void,
  ): Promise<CollabSession> {
    const config: CollabConfig = { relay, room: newRoomId(), name, color: colorFor(name) };
    const snap = await snapshot();
    const doc = new Y.Doc();
    seedFromDisk(doc, snap);
    const base = baseFromDisk(snap);
    await api.collabWrite({ config, state: toBase64(Y.encodeStateAsUpdate(doc)), base });
    return new CollabSession(config, doc, CollabSession.connect(config, doc), base, onProject, onShared);
  }

  /** Fetch a shared play from the relay (before a folder exists for it). */
  static async fetch(invite: string, name: string, timeoutMs = 15000) {
    const { relay, room, title } = readInvite(invite);
    const config: CollabConfig = { relay, room, name, color: colorFor(name) };
    const doc = new Y.Doc();
    const provider = CollabSession.connect(config, doc);
    const ok = await new Promise<boolean>((resolve) => {
      const t = setTimeout(() => resolve(false), timeoutMs);
      provider.once("sync", (s: boolean) => {
        clearTimeout(t);
        resolve(s);
      });
    });
    const v = doc.getMap("info").get("v");
    if (!ok || v === undefined) {
      provider.destroy();
      throw new Error(
        ok
          ? `The relay has no play for this code yet. Ask the person who shared "${title}" to open it while connected.`
          : `Couldn't reach the relay at ${relay}. Check the address and your connection.`,
      );
    }
    if (v !== MODEL_VERSION) {
      provider.destroy();
      throw new Error("This play was shared from a newer version of Stagewright. Update the app to join.");
    }
    return { config, doc, provider, files: readDoc(doc).files, title };
  }

  /** Adopt a fetched play once its folder has been created. */
  static adopt(
    fetched: { config: CollabConfig; doc: Y.Doc; provider: WebsocketProvider; files: Record<string, string> },
    onProject: (v: ProjectView) => void,
    onShared: () => void,
  ): CollabSession {
    const base: Base = { scenes: {}, files: { ...fetched.files } };
    return new CollabSession(fetched.config, fetched.doc, fetched.provider, base, onProject, onShared);
  }

  // -------------------------------------------------------------- presence

  private readPeers() {
    const me = this.provider.awareness.clientID;
    const out: Peer[] = [];
    this.provider.awareness.getStates().forEach((st, id) => {
      if (id === me || !st.user) return;
      out.push({ clientId: id, name: st.user.name ?? "Writer", color: st.user.color ?? "#888", scene: st.scene ?? null });
    });
    this.peers = out;
  }

  setScene(id: string | null) {
    this.provider.awareness.setLocalStateField("scene", id);
  }

  inviteCode(title: string): string {
    return inviteCode(this.config, title);
  }

  // -------------------------------------------------------------- reading

  sceneText(id: string): Y.Text | null {
    return sceneBody(this.doc, id);
  }

  sceneMeta(id: string): SharedMeta | null {
    return readScene(this.doc, id)?.meta ?? null;
  }

  undoManager(id: string): Y.UndoManager | null {
    const t = this.sceneText(id);
    if (!t) return null;
    let u = this.undo.get(id);
    if (!u) {
      u = new Y.UndoManager(t);
      this.undo.set(id, u);
    }
    return u;
  }

  // -------------------------------------------------------------- local edits (UI)

  createScene(act: number, after: string | null, title = "New scene", episode: number | null = null, body = ""): string {
    const st = readDoc(this.doc);
    const ids = actOrder(st, act, episode);
    const index = after ? ids.indexOf(after) + 1 : ids.length;
    const id = newSceneId();
    this.doc.transact(() => {
      (this.doc.getMap("acts") as Y.Map<boolean>).set(actKey(episode, act), true);
      addScene(this.doc, id, act, orderAt(st, ids, index), pickMeta({ title, status: "draft" }), body, episode);
    }, LOCAL);
    return id;
  }

  trashScene(id: string) {
    this.doc.transact(() => scenesMap(this.doc).get(id)?.set("trashed", true), LOCAL);
  }

  moveScene(id: string, act: number, index: number, episode: number | null = null) {
    const st = readDoc(this.doc);
    const ids = actOrder(st, act, episode).filter((x) => x !== id);
    const order = orderAt(st, ids, Math.max(0, Math.min(index, ids.length)));
    this.doc.transact(() => {
      const s = scenesMap(this.doc).get(id);
      if (!s) return;
      s.set("act", act);
      s.set("episode", episode);
      s.set("order", order);
    }, LOCAL);
  }

  createAct(episode: number | null = null): number {
    const st = readDoc(this.doc);
    const next = Math.max(0, ...st.acts.filter((a) => a.episode === episode).map((a) => a.act)) + 1;
    this.doc.transact(() => (this.doc.getMap("acts") as Y.Map<boolean>).set(actKey(episode, next), true), LOCAL);
    return next;
  }

  updateMeta(id: string, patch: SharedMeta) {
    this.doc.transact(() => setMeta(this.doc, id, patch), LOCAL);
  }

  /** A project file was saved (and validated) on disk; share the new text. */
  setFile(name: string, text: string) {
    this.doc.transact(() => setFile(this.doc, name, text), LOCAL);
  }

  // -------------------------------------------------------------- persistence

  private schedulePersist() {
    if (this.persistTimer) clearTimeout(this.persistTimer);
    this.persistTimer = setTimeout(() => void this.persist(), PERSIST_DELAY_MS);
  }

  async persist() {
    if (this.persistTimer) clearTimeout(this.persistTimer);
    this.persistTimer = null;
    if (this.stopped) return;
    await api.collabWrite({ state: toBase64(Y.encodeStateAsUpdate(this.doc)), base: this.base }).catch(() => undefined);
  }

  // -------------------------------------------------------------- reconcile

  scheduleReconcile(delay = RECONCILE_DELAY_MS) {
    if (this.stopped) return;
    if (this.reconcileTimer) clearTimeout(this.reconcileTimer);
    this.reconcileTimer = setTimeout(() => void this.reconcile(), delay);
  }

  /** One pass: fold outside edits in, then make the folder match the shared doc. */
  async reconcile(): Promise<void> {
    if (this.stopped) return;
    if (this.running) {
      this.again = true;
      return;
    }
    this.running = true;
    try {
      let snap = await snapshot();
      this.doc.transact(() => foldExternal(this.doc, snap, this.base), DISK);
      const want = readDoc(this.doc);
      let wrote = false;
      for (const plan of [planStructure, planOrder, planContent]) {
        const ops = plan(snap, want);
        if (!ops.length) continue;
        await this.execute(ops);
        wrote = true;
        snap = await snapshot();
      }
      const before = JSON.stringify(this.base);
      this.base = updateBase(this.base, snap, want);
      if (JSON.stringify(this.base) !== before) this.schedulePersist();
      if (wrote || this.firstPass) this.onProject(snap.view);
      this.firstPass = false;
      this.problem = null;
    } catch (e) {
      this.problem = e instanceof Error ? e.message : String(e);
    } finally {
      this.running = false;
      if (this.again) {
        this.again = false;
        this.scheduleReconcile(50);
      }
    }
  }
  private firstPass = true;

  private async execute(ops: Op[]) {
    const versions = new Map<string, string>();
    const problems: string[] = [];
    for (const op of ops) {
      try {
        switch (op.kind) {
          case "act_ensure":
            await api.actEnsure(op.act, op.episode);
            break;
          case "create":
            await api.sceneCreateWithId(op.act, op.title, op.id, op.episode);
            break;
          case "trash":
            await api.sceneTrash(op.id);
            break;
          case "reorder":
            await api.scenesReorder(op.act, op.ids, op.episode);
            break;
          case "meta": {
            const r = await api.sceneUpdateMeta(op.id, op.patch, versions.get(op.id) ?? op.version);
            versions.set(op.id, r.version);
            break;
          }
          case "body": {
            const r = await api.sceneSave(op.id, op.body, versions.get(op.id) ?? op.version);
            versions.set(op.id, r.version);
            break;
          }
          case "file":
            await api.fileSave(op.name, op.text, op.version);
            break;
        }
      } catch (e) {
        // A conflict means the file changed under us: the next pass folds it in.
        if (e instanceof ApiError && e.code === "conflict") this.again = true;
        else problems.push(e instanceof Error ? e.message : String(e));
      }
    }
    if (problems.length) throw new Error(problems.join("; "));
  }

  // -------------------------------------------------------------- stopping

  async stop() {
    if (this.stopped) return;
    await this.persist();
    this.stopped = true;
    clearInterval(this.poll);
    if (this.reconcileTimer) clearTimeout(this.reconcileTimer);
    this.provider.awareness.setLocalState(null);
    this.provider.destroy();
    for (const u of this.undo.values()) u.destroy();
    this.doc.destroy();
  }

  /** Stop sharing on this machine. The files stay; the room stays for others. */
  async leave() {
    await this.stop();
    await api.collabWrite({ config: null });
  }
}
