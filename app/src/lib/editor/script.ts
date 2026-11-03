// Script-aware editing for the .scene body: element detection, the "pending cue"
// state, and the commands behind Enter / Tab / the element toolbar.
//
// Commands are pure: (state) => TransactionSpec | null, so they can be unit
// tested without a DOM. `asCommand` adapts them for keymaps and buttons.

import { EditorState, StateEffect, StateField, type TransactionSpec, type Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { classifyLines, isCueText, isHeadingText, type Kind } from "../classify";

export type ElementType = "direction" | "cue" | "parenthetical" | "dialogue" | "lyric" | "song" | "transition" | "heading";

// ------------------------------------------------------------------ state

/** Marks a line the writer has started as a character cue but not finished. */
export const setPending = StateEffect.define<{ pos: number; on: boolean }>();

interface ScriptInfo {
  /** Line-start positions of pending cues. */
  pending: number[];
  /** Display kind of every line (index = line number - 1). */
  kinds: Kind[];
}

function docLines(doc: Text): string[] {
  const out: string[] = [];
  for (let i = 1; i <= doc.lines; i++) out.push(doc.line(i).text);
  return out;
}

function compute(doc: Text, pending: number[]): ScriptInfo {
  const kinds = classifyLines(docLines(doc));
  // A pending cue only matters until the parser sees a real cue there
  // (i.e. once its dialogue is written); then it is dropped.
  const still: number[] = [];
  for (const p of pending) {
    const n = doc.lineAt(p).number - 1;
    if (kinds[n] === "direction" || kinds[n] === "blank") {
      kinds[n] = "cue";
      still.push(p);
    }
  }
  pending = still;
  // An empty line straight after a cue or parenthetical is where dialogue goes.
  for (let n = 1; n < kinds.length; n++) {
    if (kinds[n] === "blank" && (kinds[n - 1] === "cue" || kinds[n - 1] === "parenthetical")) {
      kinds[n] = "dialogue";
    }
  }
  return { pending, kinds };
}

export const scriptField = StateField.define<ScriptInfo>({
  create: (state) => compute(state.doc, []),
  update(value, tr) {
    let pending = value.pending;
    let touched = false;
    if (tr.docChanged) {
      pending = pending.map((p) => tr.newDoc.lineAt(tr.changes.mapPos(p, -1)).from);
      touched = true;
    }
    for (const e of tr.effects) {
      if (e.is(setPending)) {
        const pos = tr.newDoc.lineAt(e.value.pos).from;
        pending = e.value.on ? [...pending, pos] : pending.filter((p) => p !== pos);
        touched = true;
      }
    }
    if (!touched) return value;
    // A pending cue survives only while its line could still be a cue.
    pending = [...new Set(pending)].filter((p) => {
      const t = tr.newDoc.lineAt(p).text;
      return t === "" || isCueText(t);
    });
    return compute(tr.newDoc, pending);
  },
});

export function info(state: EditorState): ScriptInfo {
  return state.field(scriptField);
}

export function kindAt(state: EditorState, pos = state.selection.main.head): Kind {
  return info(state).kinds[state.doc.lineAt(pos).number - 1] ?? "blank";
}

const SPEECH: Kind[] = ["cue", "parenthetical", "dialogue"];
const SONG_PARTS: Kind[] = ["song_start", "lyric", "lyric_break"];

/** The cue that owns line `n` (1-based), if `n` is inside a speech. */
function speechCue(state: EditorState, n: number): string | null {
  const { kinds } = info(state);
  for (let i = n; i >= 1; i--) {
    const k = kinds[i - 1];
    if (k === "blank") return null;
    if (k === "cue") return state.doc.line(i).text.replace(/^@/, "").replace(/ \([^)]*\)$/, "");
    if (k === "direction" || k === "transition") return null;
  }
  return null;
}

/** Is line `n` inside a song that has started but not ended? */
function inOpenSong(state: EditorState, n: number): boolean {
  const { kinds } = info(state);
  for (let i = n - 1; i >= 1; i--) {
    const k = kinds[i - 1];
    if (k === "song_start") return true;
    if (!SONG_PARTS.includes(k)) return false;
  }
  return false;
}

/**
 * Should typing on the line at `pos` be upper-cased (and offer cue completion)?
 * Only on a cue the writer started (pending), or on an existing cue with at
 * least two characters. A lone capital ("T" while starting "The lighthouse…" above
 * another paragraph) briefly looks like a cue and must not trigger it.
 */
export function isCueEntry(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  const { pending, kinds } = info(state);
  if (pending.includes(line.from)) return true;
  return kinds[line.number - 1] === "cue" && line.text.length >= 2;
}

// ------------------------------------------------------------------ helpers

function line(state: EditorState) {
  return state.doc.lineAt(state.selection.main.head);
}

function sentenceCase(s: string): string {
  const lower = s.toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

/** Strip any element marker so the text can be re-typed as something else. */
function bareText(text: string, kind: Kind): string {
  switch (kind) {
    case "lyric":
      return text.replace(/^~ ?/, "");
    case "transition":
      return text.replace(/^> /, "");
    case "parenthetical":
      return text.replace(/^\(/, "").replace(/\)$/, "");
    case "cue":
      return text.replace(/^@/, "");
    case "direction":
      return text.replace(/^!/, "");
    case "scene_heading":
      return text.replace(/^\./, "");
    case "centered":
      return text.replace(/^>\s*/, "").replace(/\s*<$/, "");
    default:
      return text;
  }
}

type Spec = TransactionSpec;

function single(state: EditorState): boolean {
  return state.selection.ranges.length === 1;
}

// ------------------------------------------------------------------ Enter

export function enter(state: EditorState): Spec | null {
  if (!single(state) || !state.selection.main.empty) return null;
  const ln = line(state);
  const head = state.selection.main.head;
  if (head !== ln.to) return null; // mid-line: ordinary newline
  const kind = kindAt(state);
  const next = ln.number < state.doc.lines ? state.doc.line(ln.number + 1) : null;
  const nextKind = next ? info(state).kinds[next.number - 1] : null;
  const insert = (text: string, cursorOffset = text.length): Spec => ({
    changes: { from: head, insert: text },
    selection: { anchor: head + cursorOffset },
    scrollIntoView: true,
    userEvent: "input",
  });

  switch (kind) {
    case "cue": {
      if (ln.text.trim() === "") return null;
      if (next && next.text !== "") {
        return { selection: { anchor: next.to }, scrollIntoView: true };
      }
      return insert("\n");
    }
    case "parenthetical":
      return insert("\n");
    case "dialogue":
      // Empty dialogue slot: leave the speech.
      if (ln.text === "") return insert("\n");
      // More of this speech below: add a line inside it. Last line: new paragraph.
      if (nextKind && SPEECH.includes(nextKind)) return insert("\n");
      return insert("\n\n");
    case "lyric":
      if (ln.text === "~ " || ln.text === "~") {
        // The song already has its END line below: drop this empty lyric and step past it.
        if (next && nextKind === "song_end") {
          return {
            changes: { from: ln.from, to: next.to, insert: "~ END SONG\n\n" },
            selection: { anchor: ln.from + "~ END SONG\n\n".length },
            scrollIntoView: true,
            userEvent: "input",
          };
        }
        if (inOpenSong(state, ln.number)) {
          return {
            changes: { from: ln.from, to: ln.to, insert: "~ END SONG\n\n" },
            selection: { anchor: ln.from + "~ END SONG\n\n".length },
            scrollIntoView: true,
            userEvent: "input",
          };
        }
        return { changes: { from: ln.from, to: ln.to, insert: "" }, userEvent: "input" };
      }
      return insert("\n~ ");
    case "lyric_break":
    case "song_start":
      return insert("\n~ ");
    case "song_end":
    case "transition":
    case "scene_heading":
    case "centered":
      return insert("\n\n");
    case "direction":
      if (ln.text === "") return null;
      return insert("\n\n");
    default:
      return null;
  }
}

// ------------------------------------------------------------------ element conversion

interface Rewrite {
  /** Ensure a blank line separates this line from the one above. */
  blankBefore?: boolean;
  /** Ensure a blank line separates this line from the one below. */
  blankAfter?: boolean;
  /** Cursor offset within `body` (default: end). */
  cursor?: number;
  /** Mark (true) or unmark (false) the rewritten line as a pending cue. */
  pending?: boolean;
}

/** Replace the cursor line with `body` as one change, adding separating blank lines as asked. */
function rewriteLine(state: EditorState, body: string, opts: Rewrite = {}): Spec {
  const ln = line(state);
  const prev = ln.number > 1 ? state.doc.line(ln.number - 1) : null;
  const next = ln.number < state.doc.lines ? state.doc.line(ln.number + 1) : null;
  const before = !!opts.blankBefore && !!prev && prev.text !== "";
  const after = !!opts.blankAfter && !!next && next.text !== "";
  const insert = (before ? "\n" : "") + body + (after ? "\n" : "");
  const start = ln.from + (before ? 1 : 0);
  const spec: Spec = {
    changes: { from: ln.from, to: ln.to, insert },
    selection: { anchor: start + (opts.cursor ?? body.length) },
    scrollIntoView: true,
    userEvent: "input",
  };
  if (opts.pending !== undefined) spec.effects = setPending.of({ pos: start, on: opts.pending });
  return spec;
}

/** Start a new speech on the current line: the cue is entered next. */
export function startSpeech(state: EditorState): Spec {
  return rewriteLine(state, "", { blankBefore: true, pending: true });
}

export function toElement(state: EditorState, target: ElementType): Spec | null {
  if (!single(state)) return null;
  const ln = line(state);
  const kind = kindAt(state);
  const text = bareText(ln.text, kind);
  const cue = speechCue(state, ln.number);
  const inSpeech = SPEECH.includes(kind);

  switch (target) {
    case "cue":
      if (text.trim() === "") return startSpeech(state);
      return rewriteLine(state, text.toUpperCase(), { blankBefore: true, pending: true });
    case "direction":
      // A direction cannot sit inside a speech, so it is fenced off by blank lines.
      // A heading stays a heading whatever its case (INT. is case-blind), so force it with "!".
      const body = kind === "cue" ? sentenceCase(text) : kind === "scene_heading" || isHeadingText(text) ? `!${text}` : text;
      return rewriteLine(state, body, {
        blankBefore: inSpeech || kind === "lyric",
        blankAfter: inSpeech,
        pending: false,
      });
    case "parenthetical":
      if (!cue || kind === "cue") return null; // only meaningful inside a speech
      return text === "" ? rewriteLine(state, "()", { cursor: 1 }) : rewriteLine(state, `(${text})`);
    case "dialogue":
      if (cue && kind !== "cue") return rewriteLine(state, text);
      // Not in a speech: ask who says it, on a new cue line above.
      return rewriteLine(state, "\n" + text, { blankBefore: true, cursor: 0, pending: true });
    case "lyric": {
      const lyric = `~ ${text}`;
      if (kind === "lyric" || inOpenSong(state, ln.number)) return rewriteLine(state, lyric);
      // Open (and close) a song around this line.
      const head = `~ SONG "" [${cue ?? ""}]\n`;
      const next = ln.number < state.doc.lines ? state.doc.line(ln.number + 1) : null;
      const nextIsLyric = next ? ["lyric", "lyric_break"].includes(info(state).kinds[next.number - 1]) : false;
      return rewriteLine(state, head + lyric + (nextIsLyric ? "" : "\n~ END SONG"), {
        blankBefore: !cue,
        cursor: head.length + lyric.length,
      });
    }
    case "song": {
      // A complete number; the cursor goes into the title.
      const block = `~ SONG "" [${cue ?? ""}]\n~ \n~ END SONG`;
      const titleAt = '~ SONG "'.length;
      if (ln.text === "") return rewriteLine(state, block, { blankBefore: !cue, cursor: titleAt });
      const sep = cue ? "\n" : "\n\n";
      return rewriteLine(state, ln.text + sep + block, { cursor: ln.text.length + sep.length + titleAt });
    }
    case "transition":
      return rewriteLine(state, `> ${text.toUpperCase()}`, { blankBefore: true, blankAfter: true, pending: false });
    case "heading": {
      // INT./EXT. … : keep a prefix the writer typed, otherwise start with INT.
      const t = text.trim().toUpperCase();
      const heading = t === "" ? "INT. " : isHeadingText(t) ? t : `INT. ${t}`;
      return rewriteLine(state, heading, { blankBefore: true, blankAfter: true, pending: false });
    }
  }
}

export function stanzaBreak(state: EditorState): Spec | null {
  const ln = line(state);
  const kind = kindAt(state);
  if (!["lyric", "lyric_break", "song_start"].includes(kind)) return null;
  return {
    changes: { from: ln.to, insert: "\n~\n~ " },
    selection: { anchor: ln.to + 5 },
    scrollIntoView: true,
    userEvent: "input",
  };
}

// ------------------------------------------------------------------ Tab

export function tab(state: EditorState): Spec | null {
  if (!single(state)) return null;
  const ln = line(state);
  const kind = kindAt(state);
  const pending = info(state).pending.includes(ln.from);
  if (ln.text === "" && kind === "dialogue") return toElement(state, "parenthetical");
  if (ln.text === "" && !pending) return startSpeech(state);
  switch (kind) {
    case "direction":
      return toElement(state, "cue");
    case "cue":
      // Tab on a finished cue moves on to its dialogue, like Enter.
      if (state.selection.main.head !== ln.to) return { selection: { anchor: ln.to } };
      return enter(state);
    case "dialogue":
      return toElement(state, "parenthetical");
    case "parenthetical":
      return toElement(state, "dialogue");
    default:
      return null;
  }
}

export function shiftTab(state: EditorState): Spec | null {
  if (!single(state)) return null;
  const kind = kindAt(state);
  switch (kind) {
    case "parenthetical":
    case "lyric":
      return toElement(state, "dialogue");
    case "cue":
    case "dialogue":
    case "scene_heading":
      return toElement(state, "direction");
    default:
      return null;
  }
}

// ------------------------------------------------------------------ adapters

export function asCommand(fn: (s: EditorState) => Spec | null, after?: (v: EditorView) => void) {
  return (view: EditorView): boolean => {
    const spec = fn(view.state);
    if (!spec) return false;
    view.dispatch(spec);
    after?.(view);
    return true;
  };
}

/** The element the toolbar should show as active for the cursor line. */
export function toolbarElement(state: EditorState): ElementType | null {
  const k = kindAt(state);
  switch (k) {
    case "cue":
    case "parenthetical":
    case "dialogue":
    case "direction":
    case "transition":
      return k;
    case "lyric":
    case "lyric_break":
      return "lyric";
    case "song_start":
    case "song_end":
      return "song";
    case "scene_heading":
      return "heading";
    case "centered":
    case "page_break":
      return "direction";
    default:
      return null;
  }
}
