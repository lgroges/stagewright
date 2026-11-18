// The context web in the app: notes on characters and places, research notes,
// beat sheets, and the links between them and the scenes.
//
// Pages and sheets are ordinary project files: read and saved through the API
// (with a version, so a change made elsewhere isn't overwritten) and, when
// co-writing, put in the shared document like the other project files.

import { api, ApiError, errorText, type Beat, type BeatSheet, type ContextIndex, type ContextRecord, type ProjectView, type RecordKind } from "./api";

/** What the notes state needs from the app. */
interface Host {
  project: ProjectView | null;
  view: { kind: string };
  collab: { setFile(name: string, text: string): void } | null;
  scene: { id: string } | null;
  createScene(act: number, after: string | null, episode?: number | null): Promise<void>;
  updateMeta(patch: { title?: string; synopsis?: string | null }): Promise<void>;
  openScene(id: string): Promise<void>;
  showView(kind: "notes" | "beats"): void;
}

export class NotesState {
  /** Every record, the sheets and what's broken; null until loaded. */
  index = $state<ContextIndex | null>(null);
  /** The record open in the Notes view. */
  selected = $state<string | null>(null);
  /** Bumps whenever a note or sheet is saved, so open views reload. */
  version = $state(0);
  error = $state<string | null>(null);

  constructor(private app: Host) {}

  reset() {
    this.index = null;
    this.selected = null;
    this.error = null;
  }

  async load() {
    if (!this.app.project) return;
    try {
      this.index = await api.contextIndex();
      this.error = null;
    } catch (e) {
      this.error = errorText(e);
    }
  }

  record(id: string): ContextRecord | undefined {
    return this.index?.records.find((r) => r.id === id);
  }

  /** Show a record where it lives: a scene in the script, a beat in Beats, anything else in Notes. */
  show(r: { id: string; kind: RecordKind }) {
    if (r.kind === "scene") void this.app.openScene(r.id);
    else if (r.kind === "beat") this.app.showView("beats");
    else {
      this.selected = r.id;
      this.app.showView("notes");
    }
  }

  /** Read a page or sheet: its text and version ("" if it doesn't exist yet). */
  read(file: string) {
    return api.fileRead(file);
  }

  /**
   * Save a page or sheet; `version` is what was read. Returns the new version,
   * or null if it couldn't be saved (the reason is in `error`).
   */
  async save(file: string, text: string, version: string): Promise<string | null> {
    try {
      const r = await api.fileSave(file, text, version);
      this.app.collab?.setFile(file, text);
      this.error = null;
      this.version++;
      await this.load();
      return r.version;
    } catch (e) {
      this.error =
        e instanceof ApiError && e.code === "conflict"
          ? "This changed elsewhere since it was opened. Reload it to see the change."
          : errorText(e);
      return null;
    }
  }

  /** A new research note, opened. */
  async newResearch(title: string) {
    const t = title.trim();
    if (!t) return;
    const n = await api.researchNew(t);
    const empty = await api.fileRead(n.file);
    if ((await this.save(n.file, n.text, empty.version)) !== null) this.show({ id: n.id, kind: "research" });
  }

  /** Delete a page (to the project's trash). Not shared when co-writing yet. */
  async trash(file: string) {
    if (this.app.collab) {
      this.error = "Deleting notes isn't shared when co-writing yet: empty the page instead.";
      return;
    }
    await api.fileTrash(file);
    this.selected = null;
    this.version++;
    await this.load();
  }

  // ------------------------------------------------------------ beat sheets

  sheet(episode: number | null): BeatSheet | undefined {
    return this.index?.sheets.find((s) => s.episode === episode);
  }

  /** Save a sheet's beats (where they land is worked out again, not saved). */
  async saveSheet(episode: number | null, template: string | null, beats: Beat[]) {
    const { file, text } = await api.sheetText({ template, beats }, episode);
    const current = await api.fileRead(file);
    return (await this.save(file, text, current.version)) !== null;
  }

  /** Start a sheet from a template (or empty). */
  async startSheet(episode: number | null, template: string) {
    const { sheet } = await api.sheetTemplate(template, episode);
    await this.saveSheet(episode, sheet.template, sheet.beats);
  }

  /**
   * Make an outline scene for a beat that has none, at the end of the last act
   * (of its episode), titled after it, and link the beat to it.
   */
  async makeScene(episode: number | null, beat: Beat) {
    const acts = (this.app.project?.acts ?? []).filter((a) => (a.episode ?? null) === episode);
    const act = acts.at(-1);
    if (!act) {
      this.error = "Add an act first.";
      return;
    }
    const last = act.scenes.at(-1)?.id ?? null;
    await this.app.createScene(act.number, last, episode);
    const id = this.app.scene?.id;
    if (!id || id === last) return;
    await this.app.updateMeta({ title: beat.title, synopsis: beat.summary || null });
    const sheet = this.sheet(episode);
    if (!sheet) return;
    const beats = sheet.beats.map((b) => (b.id === beat.id ? { ...b, scenes: [...b.scenes, id] } : b));
    await this.saveSheet(episode, sheet.template, beats);
  }
}
