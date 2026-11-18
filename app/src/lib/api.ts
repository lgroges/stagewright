import type { Issue } from "./editor/check";
// Client for the stagewright-api dispatcher. In the desktop app calls go through
// Tauri's `api` command; in a browser they go to `stagewright serve` over HTTP.
// Both reach the same Rust code.

interface ApiErrorBody {
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
export type Layout = "standard" | "uk-stage" | "bbc-studio";

/** A comment thread, pinned to a passage of one scene (comments.yaml). */
export interface CommentMessage {
  author: string;
  at: string;
  text: string;
  edited?: string | null;
}

export interface CommentAnchor {
  id: string;
  quote: string;
  before: string;
  after: string;
  line: number;
}

export interface Comment extends CommentAnchor {
  scene: string;
  resolved: boolean;
  messages: CommentMessage[];
}

export type CommentOp =
  | { op: "add"; id: string; scene: string; quote: string; before: string; after: string; line: number; author: string; at: string; text: string }
  | { op: "reply"; id: string; author: string; at: string; text: string }
  | { op: "edit"; id: string; index: number; text: string; at: string }
  | { op: "delete_message"; id: string; index: number }
  | { op: "delete"; id: string }
  | { op: "resolve"; id: string; resolved: boolean }
  | { op: "anchor"; anchors: CommentAnchor[] };

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
  /** What the action calls them after the introduction ("Joanna"). */
  short?: string | null;
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
  /** A shooting script (project.yaml shooting_script); otherwise a spec script. */
  shooting_script: boolean;
  /** The AI review may send this project's text to Anthropic (project.yaml ai.cloud). */
  ai_cloud: boolean;
  /** How exports lay the script out, and the layouts this format offers. */
  layout: Layout;
  layouts: { id: Layout; name: string }[];
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

type Entry = Record<string, unknown> & { id: string };

export interface CastCharacter {
  entry: Entry & { cue: string; name?: string; short?: string; description?: string; track?: string | null };
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
function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
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

/** Something the AI review found: `quote` is the words, as they were in the scene. */
export interface AiFinding {
  quote: string;
  line: number;
  message: string;
  suggestion: string;
}

export type AiReview =
  | { ok: true; findings: AiFinding[]; cached: boolean; model: string | null; cost_usd?: number | null }
  | { ok: false; reason: string; off?: boolean; needs_consent?: boolean };

/** Where the AI runs: Ollama on this computer, or Claude (Anthropic). */
export type AiProvider = "ollama" | "claude";

/** Whether the AI is ready: Ollama's models, or whether a Claude key is there and works. */
export interface AiStatus {
  ok: boolean;
  reason?: string;
  /** Ollama: the installed models. */
  models?: string[];
  /** Claude: a key is set, and it's the saved one (not ANTHROPIC_API_KEY). */
  has_key?: boolean;
  stored?: boolean;
}

const aiFailed = (e: unknown): { ok: false; reason: string } => ({ ok: false, reason: String((e as Error)?.message ?? e) });

/**
 * The AI review. These don't queue behind saves (a review can take minutes),
 * and status and review never throw: a failure is `ok: false`.
 */
export const ai = {
  status: (provider: AiProvider, url: string) =>
    (transport("ai_status", { provider, url }) as Promise<AiStatus>).catch((e): AiStatus => aiFailed(e)),
  review: (args: { root: string; scene: string; body: string; provider: AiProvider; url: string; model: string }) =>
    (transport("ai_review", args) as Promise<AiReview>).catch((e): AiReview => aiFailed(e)),
  /** Save the Claude API key in the system's credential store ("" removes it). */
  setKey: (key: string) => transport("ai_key_set", { key }) as Promise<{ ok: true; has_key: boolean }>,
};

// ------------------------------------------------------------ the context web

export type RecordKind = "character" | "location" | "scene" | "beat" | "research";

/** A record in the context web (a character, place, scene, beat or research note). */
export interface ContextRecord {
  id: string;
  kind: RecordKind;
  name: string;
  /** A scene's label ("1.2"). */
  label: string | null;
  /** Its page's file, if it can have one ("characters/dee.md"). */
  file: string | null;
  has_page: boolean;
}

/** A labelled link stored on a page. */
export interface PageLink {
  to: string;
  label?: string;
  back?: string;
}

export interface Beat {
  id: string;
  title: string;
  summary?: string | null;
  purpose?: string | null;
  /** Where it should land: a share of the sheet's length (0–1). */
  target?: number | null;
  scenes: string[];
  episodes: number[];
  /** Where it lands now (from the index; not saved). */
  lands?: number | null;
  lands_at?: string | null;
}

export interface BeatSheet {
  episode: number | null;
  template: string | null;
  beats: Beat[];
  file: string;
}

export interface ContextIndex {
  records: ContextRecord[];
  sheets: BeatSheet[];
  templates: { id: string; name: string }[];
  broken: { from: string; to: string }[];
  duplicates: string[];
}

/** A record linked to another, seen from that other. */
export interface Neighbour {
  id: string;
  kind: RecordKind;
  name: string;
  label: string | null;
  /** The other record made the link (a backlink). */
  incoming: boolean;
  how: "link" | "text" | "plan";
}

/** Something around a scene, with its page's text. */
export interface ContextItem {
  id: string;
  kind: RecordKind;
  name: string;
  label: string | null;
  text: string | null;
  /** Research reached through a character, place or beat: its name. */
  via: string | null;
  summary?: string | null;
  purpose?: string | null;
}

export interface CollabConfig {
  relay: string;
  room: string;
  name: string;
  color: string;
}

interface DiskSnapshotBody {
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
  /** `ai`: findings from an AI review of this scene, to show with the rule issues. */
  checkScene: (id: string, body: string, ai: AiFinding[] = []) => call<{ issues: Issue[] }>("check_scene", { id, body, ai }),
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
  commentsList: () => call<{ comments: Comment[] }>("comments_list"),
  commentEdit: (op: CommentOp) => call<{ comments: Comment[]; text: string }>("comment_edit", { op }),
  commentsParse: (text: string) => call<{ comments: Comment[] }>("comments_parse", { text }),
  commentEditText: (text: string, op: CommentOp) => call<{ comments: Comment[]; text: string }>("comment_edit_text", { text, op }),
  fileRead: (name: string) => call<ProjectFile>("file_read", { name }),
  fileTrash: (name: string) => call<{ ok: true }>("file_trash", { name }),
  contextIndex: () => call<ContextIndex>("context_index"),
  contextRecord: (id: string) =>
    call<{ id: string; kind: RecordKind; name: string; file: string | null; links: Neighbour[] }>("context_record", { id }),
  sceneContext: (id: string) => call<{ scene: string; items: ContextItem[] }>("scene_context", { id }),
  pageParse: (text: string) =>
    call<{ meta: { title?: string; tags?: string[]; sources?: string[]; links?: PageLink[] }; body: string }>("page_parse", { text }),
  pageSet: (text: string, key: "links" | "title" | "tags" | "sources" | "body", value: unknown) =>
    call<{ text: string }>("page_set", { text, key, value }),
  researchNew: (title: string) => call<{ id: string; file: string; text: string }>("research_new", { title }),
  sheetText: (sheet: Omit<BeatSheet, "file" | "episode">) => call<{ text: string }>("sheet_text", { sheet }),
  sheetTemplate: (template: string, episode: number | null) =>
    call<{ sheet: Omit<BeatSheet, "file" | "episode"> }>("sheet_template", { template, episode }),
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
