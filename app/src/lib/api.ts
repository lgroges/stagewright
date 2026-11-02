import type { Issue } from "./editor/check";
// Client for the stagewright-api dispatcher. In the desktop app calls go through
// Tauri's `api` command; in a browser they go to `stagewright serve` over HTTP.
// Both reach the same Rust code.

export interface ApiErrorBody {
  code: string;
  message: string;
  detail?: { version?: string } & Record<string, unknown>;
}

export class ApiError extends Error {
  constructor(public body: ApiErrorBody) {
    super(body.message);
  }
  get code() {
    return this.body.code;
  }
}

export type Format = "stage" | "film" | "tv-single" | "tv-multi";

export const FORMATS: { id: Format; label: string }[] = [
  { id: "stage", label: "Stage play" },
  { id: "film", label: "Film screenplay" },
  { id: "tv-single", label: "TV, single-camera (drama)" },
  { id: "tv-multi", label: "TV, multi-camera (sitcom)" },
];

export interface SceneSummary {
  id: string;
  title: string;
  act: number;
  episode: number | null;
  label: string;
  eighths: number;
  pages: string;
  headings: string[];
  number: number | null;
  status: string | null;
  location: string | null;
  file: string;
  minutes: number;
  manual_duration: boolean;
  words: number;
  speakers: string[];
}

export interface Act {
  number: number;
  episode: number | null;
  title: string;
  scenes: SceneSummary[];
}

export interface Character {
  id: string;
  cue: string;
  name?: string | null;
  description?: string | null;
  track?: string | null;
}

export interface ProjectView {
  root: string;
  title: string;
  subtitle: string | null;
  authors: string[];
  rating_fields: string[];
  running_time: { words_per_minute: number; seconds_per_lyric_line: number };
  statuses: string[];
  format: Format;
  format_label: string;
  /** (CONT'D) added automatically (project.yaml auto_contd, or the format's default). */
  auto_contd: boolean;
  series: boolean;
  episodes: { number: number; title: string }[];
  heading_locations: string[];
  acts: Act[];
  characters: Character[];
  locations: { id: string; name?: string | null }[];
  cues: string[];
  files: string[];
  created?: string;
  created_act?: number;
  created_episode?: number;
}

export interface SceneMeta {
  id: string;
  title: string | null;
  act: number | null;
  scene: number | null;
  status: string | null;
  location: string | null;
  duration: number | null;
  onstage_silent: string[];
  synopsis: string | null;
  notes: string | null;
  tags: string[];
  ratings: Record<string, number>;
}

export interface SceneDoc {
  id: string;
  meta: SceneMeta;
  body: string;
  version: string;
  summary: SceneSummary;
}

export type DoublingIssue =
  | { kind: "clash"; scene: string; track: string; roles: string[] }
  | { kind: "quick_change"; from_scene: string; to_scene: string; track: string; from_role: string; to_role: string }
  | { kind: "unassigned"; role: string; scenes: string[] };

export interface HeadingInfo {
  text: string;
  setting: string | null;
  location: string;
  time: string | null;
}

export interface StatsScene {
  id: string;
  label: string;
  title: string;
  act: number;
  episode: number | null;
  eighths: number;
  pages: string;
  headings: HeadingInfo[];
  minutes: number;
  estimated_minutes: number;
  manual_duration: boolean;
  words: number;
  lyric_lines: number;
  songs: string[];
  speeches: Record<string, number>;
  present: string[];
}

export interface Stats {
  format: Format;
  series: boolean;
  episodes: { number: number; title: string }[];
  total_pages: string;
  scenes: StatsScene[];
  acts: { number: number; episode: number | null; title: string; minutes: number; pages: string }[];
  total_minutes: number;
  roles: { cue: string; track: string | null }[];
  issues: DoublingIssue[];
  running_time: { words_per_minute: number; seconds_per_lyric_line: number };
}

export interface ProjectFile {
  name: string;
  text: string;
  version: string;
}

// ---------------------------------------------------------------- cast and places

export type Entry = Record<string, unknown> & { id: string };

export interface CastCharacter {
  entry: Entry & { cue: string; name?: string; description?: string; track?: string | null };
  cue: string;
  /** Scene labels ("1.2", "E1 2.3") the character is in. */
  scenes: string[];
  speeches: number;
  /** Scenes where they are on stage but silent. */
  silent: number;
}

export interface UnlistedCharacter {
  cue: string;
  scenes: string[];
  speeches: number;
  silent: number;
}

export interface CastPlace {
  entry: Entry & { name?: string; description?: string };
  id: string;
  /** How a scene heading writes it: "THE QUAY". */
  heading: string;
  scenes: string[];
  headings: number;
}

export interface UnlistedPlace {
  place: string;
  /** "heading": from scene headings; "location": an id scenes use that isn't listed. */
  kind: "heading" | "location";
  headings: number;
  scenes: string[];
}

export interface CastView {
  screen: boolean;
  characters: CastCharacter[];
  unlisted_characters: UnlistedCharacter[];
  tracks: string[];
  locations: CastPlace[];
  unlisted_places: UnlistedPlace[];
}

export interface RenameResult {
  applied: boolean;
  scenes: number;
  cues: number;
  songs: number;
  mentions: number;
  headings: number;
  silent: number;
  files: string[];
  merge: boolean;
}

export interface SharedSceneText {
  id: string;
  body: string;
  onstage_silent: string[];
}

// ---------------------------------------------------------------- script check

export interface CheckInfo {
  code: string;
  name: string;
  on: boolean;
}

export interface ProjectIssue extends Issue {
  scene: string;
  label: string;
  title: string;
  /** The line as it is in the file. */
  text: string;
}

export interface ProjectCheck {
  issues: ProjectIssue[];
  warnings: number;
  infos: number;
  checks: CheckInfo[];
}

type TauriInternals = { invoke: (cmd: string, args: unknown) => Promise<unknown> };

export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function transport(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      return await invoke("api", { cmd, args });
    } catch (e) {
      if (e && typeof e === "object" && "code" in e) throw new ApiError(e as ApiErrorBody);
      throw new ApiError({ code: "transport", message: String(e) });
    }
  }
  let res: Response;
  try {
    res = await fetch(`/api/${cmd}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args),
    });
  } catch (e) {
    throw new ApiError({ code: "transport", message: `cannot reach stagewright serve: ${e}` });
  }
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new ApiError(json ?? { code: "http", message: `HTTP ${res.status}` });
  return json;
}

// Calls run one at a time, in order. Saves and metadata edits both depend on
// the scene's latest version, so they must never overlap.
let chain: Promise<unknown> = Promise.resolve();
export function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const run = chain.then(() => transport(cmd, args) as Promise<T>);
  chain = run.catch(() => undefined);
  return run;
}

/** Like `call`, but args are computed when the call actually runs (after earlier calls finish). */
export function callLate<T>(cmd: string, args: () => Record<string, unknown>): Promise<T> {
  const run = chain.then(() => transport(cmd, args()) as Promise<T>);
  chain = run.catch(() => undefined);
  return run;
}

export interface CollabConfig {
  relay: string;
  room: string;
  name: string;
  color: string;
}

export interface DiskSnapshotBody {
  view: ProjectView;
  scenes: { id: string; act: number; episode: number | null; meta: SceneMeta; body: string; version: string }[];
  files: ProjectFile[];
}

export const api = {
  projectOpen: (root: string) => call<ProjectView>("project_open", { root }),
  projectCreate: (root: string, title: string, format: Format = "stage", series = false) =>
    call<ProjectView>("project_create", { root, title, format, series }),
  projectView: () => call<ProjectView>("project_view"),
  projectClose: () => call<null>("project_close"),
  ping: () => call<{ ok: boolean; root: string | null }>("ping"),
  sceneRead: (id: string) => call<SceneDoc>("scene_read", { id }),
  sceneCreate: (act: number, after: string | null, title: string, episode: number | null = null) =>
    call<ProjectView>("scene_create", { act, after, title, episode }),
  sceneTrash: (id: string) => call<ProjectView>("scene_trash", { id }),
  scenesReorder: (act: number, ids: string[], episode: number | null = null) =>
    call<ProjectView>("scenes_reorder", { act, ids, episode }),
  actCreate: (episode: number | null = null) => call<ProjectView>("act_create", { episode }),
  episodeCreate: (title?: string) => call<ProjectView>("episode_create", { title }),
  setOption: (name: string, value: unknown) => call<ProjectView>("set_option", { name, value }),
  yamlSetOption: (text: string, name: string, value: unknown) => call<{ text: string }>("yaml_set_option", { text, name, value }),
  checkScene: (id: string, body: string) => call<{ issues: Issue[] }>("check_scene", { id, body }),
  checkProject: () => call<ProjectCheck>("check_project"),
  setCheck: (code: string, on: boolean) => call<ProjectCheck>("set_check", { code, on }),
  yamlSetCheck: (text: string, code: string, on: boolean) => call<{ text: string }>("yaml_set_check", { text, code, on }),
  castView: () => call<CastView>("cast_view"),
  characterSave: (id: string | null, entry: Record<string, unknown>) =>
    call<{ cast: CastView; files: Record<string, string> }>("character_save", { id, entry }),
  characterDelete: (id: string) => call<{ cast: CastView; files: Record<string, string> }>("character_delete", { id }),
  locationSave: (id: string | null, entry: Record<string, unknown>) =>
    call<{ cast: CastView; files: Record<string, string> }>("location_save", { id, entry }),
  locationDelete: (id: string) => call<{ cast: CastView; files: Record<string, string> }>("location_delete", { id }),
  rename: (kind: "character" | "place", old: string, next: string, apply: boolean) =>
    call<RenameResult & { cast: CastView }>(kind === "character" ? "character_rename" : "place_rename", { old, new: next, apply }),
  renameShared: (kind: "character" | "place", old: string, next: string, scenes: SharedSceneText[], files: Record<string, string>) =>
    call<RenameResult & { changed_scenes: SharedSceneText[]; changed_files: Record<string, string> }>("rename_shared", {
      kind, old, new: next, scenes, files,
    }),
  registryEditText: (file: string, text: string, op: "save" | "delete", id: string | null, entry: Record<string, unknown> = {}) =>
    call<{ text: string }>("registry_edit_text", { file, text, op, id, entry }),
  actDelete: (act: number, episode: number | null = null) =>
    call<ProjectView & { trashed_scenes: number }>("act_delete", { act, episode }),
  episodeDelete: (episode: number) => call<ProjectView & { trashed_scenes: number }>("episode_delete", { episode }),
  actRemoveEmpty: (act: number, episode: number | null = null) => call<ProjectView>("act_remove_empty", { act, episode }),
  yamlRemoveAct: (text: string, act: number, series: boolean) =>
    call<{ text: string }>("yaml_remove_act", { text, act, series }),
  yamlRemoveEpisode: (text: string, episode: number) => call<{ text: string }>("yaml_remove_episode", { text, episode }),
  exportScript: (kind: "pdf" | "fountain", episode: number | null, path?: string) =>
    call<{ name: string; pages: number | null; base64?: string; path?: string }>("export", { kind, episode, path }),
  stats: () => call<Stats>("stats"),
  fileRead: (name: string) => call<ProjectFile>("file_read", { name }),
  fileSave: (name: string, text: string, base_version: string) =>
    call<ProjectFile>("file_save", { name, text, base_version }),
  // co-writing
  projectSnapshot: () => call<DiskSnapshotBody>("project_snapshot"),
  actEnsure: (act: number, episode: number | null = null) => call<ProjectView>("act_ensure", { act, episode }),
  sceneCreateWithId: (act: number, title: string, id: string, episode: number | null = null) =>
    call<ProjectView>("scene_create", { act, after: null, title, id, episode }),
  sceneUpdateMeta: (id: string, patch: Record<string, unknown>, base_version: string) =>
    call<SceneDoc>("scene_update_meta", { id, patch, base_version }),
  sceneSave: (id: string, body: string, base_version: string) => call<SceneDoc>("scene_save", { id, body, base_version }),
  collabRead: () => call<{ config: CollabConfig | null; state: string | null; base: unknown }>("collab_read"),
  collabWrite: (args: { config?: CollabConfig | null; state?: string; base?: unknown }) => call<null>("collab_write", args),
  projectJoin: (root: string, files: Record<string, string>, config: CollabConfig) =>
    call<ProjectView>("project_join", { root, files, config }),
};
