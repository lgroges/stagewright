// App state: the open project, the open scene, saving, and structural edits.
// Every write goes through the serial API queue and carries the scene's latest
// version, so body saves and metadata edits never trample each other.

import {
  api,
  ApiError,
  callLate,
  isTauri,
  type Format,
  type ProjectView,
  type SceneDoc,
  type SceneMeta,
  type SceneSummary,
} from "./api";
import { CollabSession } from "./collab/session.svelte";

export type SaveStatus = "saved" | "unsaved" | "saving" | "conflict" | "error";
export type MainView = { kind: "script" } | { kind: "production" } | { kind: "file"; name: string };

interface OpenScene {
  id: string;
  version: string;
  meta: SceneMeta;
  summary: SceneSummary;
  /** Body as last loaded from disk. */
  loadedBody: string;
  /**
   * The latest text, including unsaved typing. An editor that is created
   * afresh (after another view was shown, say) must start from this, never
   * from `loadedBody`, or it would show, and then save, an old version.
   */
  body: string;
  /** Bumps when the editor must replace its document (open, reload). */
  loadToken: number;
}

const RECENT_KEY = "stagewright.recent";
const RELAY_KEY = "stagewright.relay";
const NAME_KEY = "stagewright.name";
export const DEFAULT_RELAY = "ws://127.0.0.1:4444";

function readPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* a convenience only */
  }
}
const SAVE_DELAY_MS = 700;

function readRecent(): string[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
  } catch {
    return [];
  }
}

class AppState {
  project = $state<ProjectView | null>(null);
  scene = $state<OpenScene | null>(null);
  view = $state<MainView>({ kind: "script" });
  saveStatus = $state<SaveStatus>("saved");
  conflict = $state<{ diskVersion: string; body: string } | null>(null);
  error = $state<string | null>(null);
  metaOpen = $state(true);
  busy = $state(false);
  recent = $state<string[]>(readRecent());
  /** Live co-writing session, when this project is shared. */
  collab = $state<CollabSession | null>(null);
  relay = $state(readPref(RELAY_KEY, DEFAULT_RELAY));
  writerName = $state(readPref(NAME_KEY, ""));

  private pendingBody: string | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private loadCounter = 0;

  // -------------------------------------------------------------- project

  private remember(root: string) {
    this.recent = [root, ...this.recent.filter((r) => r !== root)].slice(0, 8);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
    } catch {
      /* storage unavailable: recent list is a convenience only */
    }
  }

  forget(root: string) {
    this.recent = this.recent.filter((r) => r !== root);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
    } catch {
      /* ignore */
    }
  }

  private async afterOpen(view: ProjectView, resumeSharing = true) {
    if (resumeSharing && this.collab) {
      await this.collab.stop();
      this.collab = null;
    }
    this.project = view;
    this.remember(view.root);
    this.scene = null;
    this.view = { kind: "script" };
    if (resumeSharing) {
      const saved = await api.collabRead().catch(() => null);
      if (saved?.config) {
        this.collab = await CollabSession.resume(saved.config, saved, (v) => this.onSharedProject(v), () => this.onSharedChange());
      }
    }
    const first = view.acts.flatMap((a) => a.scenes)[0];
    if (first) await this.openScene(first.id);
  }

  // -------------------------------------------------------------- co-writing

  /** The folder was brought in line with the shared play. */
  private onSharedProject(view: ProjectView) {
    this.project = view;
    if (this.scene) {
      const s = view.acts.flatMap((a) => a.scenes).find((x) => x.id === this.scene!.id);
      if (s) this.scene.summary = s;
      else if (this.collab && !this.collab.sceneMeta(this.scene.id)) this.scene = null;
    }
    if (!this.scene && this.view.kind === "script") {
      const first = view.acts.flatMap((a) => a.scenes)[0];
      if (first) void this.openScene(first.id);
    }
  }

  /** Something changed in the shared doc (here or from another writer). */
  private onSharedChange() {
    if (!this.collab || !this.scene) return;
    const m = this.collab.sceneMeta(this.scene.id);
    if (m) this.scene.meta = { ...this.scene.meta, ...(m as Partial<SceneMeta>) };
  }

  private rememberWriter(relay: string, name: string) {
    this.relay = relay;
    this.writerName = name;
    writePref(RELAY_KEY, relay);
    writePref(NAME_KEY, name);
  }

  /** Share the open project. Returns the invite code. */
  async share(relay: string, name: string): Promise<string | null> {
    if (!this.project || this.collab) return null;
    await this.flush();
    this.rememberWriter(relay, name);
    let code: string | null = null;
    await this.guard(async () => {
      this.collab = await CollabSession.share(relay, name, (v) => this.onSharedProject(v), () => this.onSharedChange());
      code = this.collab.inviteCode(this.project!.title);
      const id = this.scene?.id;
      this.scene = null;
      if (id) await this.openScene(id); // reopen bound to the shared text
    });
    return code;
  }

  /** Join a shared play into an empty folder. */
  async join(invite: string, root: string, name: string) {
    await this.guard(async () => {
      const fetched = await CollabSession.fetch(invite, name);
      this.rememberWriter(fetched.config.relay, name);
      let view: ProjectView;
      try {
        view = await api.projectJoin(root, fetched.files, fetched.config);
      } catch (e) {
        fetched.provider.destroy();
        throw e;
      }
      this.collab = CollabSession.adopt(fetched, (v) => this.onSharedProject(v), () => this.onSharedChange());
      await this.afterOpen(view, false);
    });
  }

  /** Stop co-writing on this machine; the files stay as they are. */
  async leaveSharing() {
    if (!this.collab) return;
    await this.collab.reconcile();
    await this.collab.leave();
    this.collab = null;
    const id = this.scene?.id;
    this.scene = null;
    await this.refreshProject();
    if (id) await this.openScene(id);
  }

  inviteCode(): string | null {
    return this.collab && this.project ? this.collab.inviteCode(this.project.title) : null;
  }

  async openProject(root: string) {
    await this.guard(async () => this.afterOpen(await api.projectOpen(root)));
  }

  async createProject(root: string, title: string, format: Format = "stage", series = false) {
    await this.guard(async () => this.afterOpen(await api.projectCreate(root, title, format, series)));
  }

  /** In the browser dev server the project may already be open. */
  async resume(): Promise<boolean> {
    try {
      const p = await api.ping();
      if (p.root) {
        await this.afterOpen(await api.projectView());
        return true;
      }
    } catch {
      /* not open */
    }
    return false;
  }

  async closeProject() {
    await this.flush();
    if (this.collab) {
      await this.collab.stop();
      this.collab = null;
    }
    await api.projectClose().catch(() => undefined);
    this.project = null;
    this.scene = null;
  }

  // -------------------------------------------------------------- scenes

  allScenes(): SceneSummary[] {
    return this.project?.acts.flatMap((a) => a.scenes) ?? [];
  }

  async openScene(id: string) {
    if (this.scene?.id === id) {
      this.view = { kind: "script" };
      return;
    }
    if (this.collab) {
      const shared = this.collab.sceneMeta(id);
      const summary = this.allScenes().find((s) => s.id === id);
      if (!shared && !summary) return;
      const meta = {
        id, title: null, act: null, scene: null, status: null, location: null, duration: null,
        onstage_silent: [], synopsis: null, notes: null, tags: [], ratings: {},
        ...(shared as Partial<SceneMeta>),
      } as SceneMeta;
      this.scene = {
        id,
        version: "",
        meta,
        summary: summary ?? {
          id, title: meta.title ?? "New scene", act: 0, episode: null, label: "", number: null, status: meta.status,
          location: meta.location, file: "(being written)", minutes: 0, manual_duration: false, words: 0,
          eighths: 0, pages: "", headings: [], speakers: [],
        },
        loadedBody: "",
        body: "",
        loadToken: ++this.loadCounter,
      };
      this.collab.setScene(id);
      this.view = { kind: "script" };
      return;
    }
    await this.flush();
    if (this.saveStatus === "conflict" || this.saveStatus === "error") return;
    await this.guard(async () => {
      const doc = await api.sceneRead(id);
      this.setScene(doc, true);
      this.view = { kind: "script" };
    });
  }

  private setScene(doc: SceneDoc, replaceBody: boolean) {
    const token = replaceBody ? ++this.loadCounter : (this.scene?.loadToken ?? ++this.loadCounter);
    this.scene = {
      id: doc.id,
      version: doc.version,
      meta: doc.meta,
      summary: doc.summary,
      loadedBody: replaceBody ? doc.body : (this.scene?.loadedBody ?? doc.body),
      // After a save, keep what the editor holds: typing may have continued.
      body: replaceBody ? doc.body : (this.scene?.id === doc.id ? this.scene.body : doc.body),
      loadToken: token,
    };
    this.patchSummary(doc.summary);
    if (replaceBody) {
      this.saveStatus = "saved";
      this.conflict = null;
    }
  }

  private patchSummary(s: SceneSummary) {
    if (!this.project) return;
    for (const act of this.project.acts) {
      const i = act.scenes.findIndex((x) => x.id === s.id);
      if (i >= 0) act.scenes[i] = s;
    }
  }

  /** Called by the editor on every change; saving is debounced. */
  bodyChanged(body: string) {
    if (this.scene) this.scene.body = body;
    if (this.collab) return; // the editor writes straight into the shared text
    this.pendingBody = body;
    if (this.saveStatus !== "conflict") this.saveStatus = "unsaved";
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.flush(), SAVE_DELAY_MS);
  }

  /** Write any unsaved body now. Safe to call at any time. */
  async flush(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    if (this.pendingBody === null || !this.scene || this.saveStatus === "conflict") return;
    const body = this.pendingBody;
    const id = this.scene.id;
    this.pendingBody = null;
    this.saveStatus = "saving";
    try {
      const doc = await callLate<SceneDoc>("scene_save", () => ({ id, body, base_version: this.scene!.version }));
      if (this.scene?.id === id) this.setScene(doc, false);
      this.saveStatus = this.pendingBody === null ? "saved" : "unsaved";
    } catch (e) {
      if (e instanceof ApiError && e.code === "conflict") {
        this.conflict = { diskVersion: String(e.body.detail?.version ?? ""), body };
        this.saveStatus = "conflict";
      } else {
        this.pendingBody ??= body; // keep it so nothing is lost
        this.saveStatus = "error";
        this.error = e instanceof Error ? e.message : String(e);
      }
    }
  }

  /** Conflict: overwrite the disk copy with what is in the editor. */
  async keepMine() {
    if (!this.scene || !this.conflict) return;
    this.scene.version = this.conflict.diskVersion;
    this.pendingBody = this.conflict.body;
    this.conflict = null;
    this.saveStatus = "unsaved";
    await this.flush();
  }

  /** Conflict (or any time): discard local edits and load the disk copy. */
  async reloadFromDisk() {
    if (!this.scene) return;
    this.pendingBody = null;
    this.conflict = null;
    const id = this.scene.id;
    await this.guard(async () => this.setScene(await api.sceneRead(id), true));
  }

  async retrySave() {
    this.error = null;
    if (this.saveStatus === "error") this.saveStatus = "unsaved";
    await this.flush();
  }

  async updateMeta(patch: Partial<SceneMeta>) {
    if (!this.scene) return;
    if (this.collab) {
      this.collab.updateMeta(this.scene.id, patch as Record<string, unknown>);
      return;
    }
    await this.flush();
    if (this.saveStatus === "conflict") return;
    const id = this.scene.id;
    await this.guard(async () => {
      const doc = await callLate<SceneDoc>("scene_update_meta", () => ({ id, patch, base_version: this.scene!.version }));
      if (this.scene?.id === id) this.setScene(doc, false);
    });
  }

  /** After a structural change the open scene's file (and version) may have changed. */
  private async refreshOpenScene() {
    if (!this.scene || !this.project) return;
    const id = this.scene.id;
    if (!this.allScenes().some((s) => s.id === id)) {
      this.scene = null;
      return;
    }
    const doc = await api.sceneRead(id);
    if (this.scene?.id === id) this.setScene(doc, false);
  }

  private async structural(op: () => Promise<ProjectView>): Promise<ProjectView | null> {
    await this.flush();
    if (this.saveStatus === "conflict") return null;
    let view: ProjectView | null = null;
    await this.guard(async () => {
      view = await op();
      this.project = view;
      await this.refreshOpenScene();
    });
    return view;
  }

  /** The body a new scene starts with: a heading prompt in screen formats. */
  private newSceneBody(): string {
    return this.project && this.project.format !== "stage" ? "INT. " : "";
  }

  async createScene(act: number, after: string | null, episode: number | null = null) {
    if (this.collab) {
      const id = this.collab.createScene(act, after, "New scene", episode, this.newSceneBody());
      await this.openScene(id);
      return;
    }
    const v = await this.structural(() => api.sceneCreate(act, after, "New scene", episode));
    if (v?.created) {
      await this.openScene(v.created);
      const body = this.newSceneBody();
      if (body && this.scene?.id === v.created) {
        // Start the scene with a heading; the editor reloads to show it.
        await this.guard(async () => {
          const doc = await api.sceneSave(v.created!, body, this.scene!.version);
          this.setScene(doc, true);
        });
      }
    }
  }

  async trashScene(id: string) {
    if (this.collab) {
      this.collab.trashScene(id);
      if (this.scene?.id === id) this.scene = null;
      return;
    }
    await this.structural(() => api.sceneTrash(id));
  }

  async moveScene(id: string, toAct: number, index: number, episode: number | null = null) {
    if (!this.project) return;
    if (this.collab) {
      this.collab.moveScene(id, toAct, index, episode);
      return;
    }
    const act = this.project.acts.find((a) => a.number === toAct && (a.episode ?? null) === episode);
    if (!act) return;
    const ids = act.scenes.map((s) => s.id).filter((x) => x !== id);
    ids.splice(Math.max(0, Math.min(index, ids.length)), 0, id);
    const before = act.scenes.map((s) => s.id);
    if (ids.length === before.length && ids.every((x, i) => x === before[i])) return;
    await this.structural(() => api.scenesReorder(toAct, ids, episode));
  }

  async createAct(episode: number | null = null) {
    if (this.collab) {
      this.collab.createAct(episode);
      return;
    }
    await this.structural(() => api.actCreate(episode));
  }

  async createEpisode() {
    await this.structural(() => api.episodeCreate());
    // Shared projects pick the new episode up from the folder on the next pass.
    this.collab?.scheduleReconcile(0);
  }

  /** Delete an act; its scenes go to the project's .trash folder. */
  async deleteAct(act: number, episode: number | null = null) {
    if (this.collab) {
      const series = !!this.project?.series;
      const text = this.collab.fileText("project.yaml");
      await this.guard(async () => {
        const yaml = text === null ? null : (await api.yamlRemoveAct(text, act, series)).text;
        this.collab!.deleteAct(act, episode, yaml);
      });
    } else {
      await this.structural(() => api.actDelete(act, episode));
    }
    if (!this.scene) await this.openFirstScene();
  }

  /** Delete an episode; its scenes go to the project's .trash folder. */
  async deleteEpisode(episode: number) {
    if (this.collab) {
      const text = this.collab.fileText("project.yaml") ?? "";
      await this.guard(async () => {
        this.collab!.deleteEpisode(episode, (await api.yamlRemoveEpisode(text, episode)).text);
      });
    } else {
      await this.structural(() => api.episodeDelete(episode));
    }
    if (!this.scene) await this.openFirstScene();
  }

  private async openFirstScene() {
    const first = this.allScenes()[0];
    if (first) await this.openScene(first.id);
  }

  /** Export the script. Desktop: choose where to save. Browser: download. */
  async exportScript(kind: "pdf" | "fountain", episode: number | null) {
    await this.flush();
    await this.guard(async () => {
      if (isTauri()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const suggested = (await api.exportScript(kind, episode)).name;
        const path = await save({
          title: kind === "pdf" ? "Export PDF" : "Export Fountain",
          defaultPath: suggested,
          filters: [kind === "pdf" ? { name: "PDF", extensions: ["pdf"] } : { name: "Fountain", extensions: ["fountain"] }],
        });
        if (path) await api.exportScript(kind, episode, path);
        return;
      }
      const r = await api.exportScript(kind, episode);
      const bin = atob(r.base64 ?? "");
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const url = URL.createObjectURL(new Blob([bytes], { type: kind === "pdf" ? "application/pdf" : "text/plain" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = r.name;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 5000);
    });
  }

  async refreshProject() {
    await this.guard(async () => {
      this.project = await api.projectView();
      await this.refreshOpenScene();
    });
  }

  // -------------------------------------------------------------- errors

  private async guard(fn: () => Promise<void>) {
    this.busy = true;
    try {
      await fn();
      this.error = null;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.busy = false;
    }
  }
}

export const app = new AppState();
