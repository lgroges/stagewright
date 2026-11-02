// CodeMirror extensions for the script editor: stage-play layout, element
// keys, cue autocompletion and upper-casing.

import { EditorState, Prec, RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  drawSelection,
  highlightActiveLine,
  keymap,
  placeholder,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, insertNewline } from "@codemirror/commands";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import {
  acceptCompletion,
  autocompletion,
  completionStatus,
  startCompletion,
  type Completion,
  type CompletionContext,
} from "@codemirror/autocomplete";
import {
  asCommand,
  enter,
  info,
  isCueEntry,
  kindAt,
  scriptField,
  setPending,
  shiftTab,
  stanzaBreak,
  tab,
  toElement,
  type ElementType,
} from "./script";
import { isHeadingText, type Kind } from "../classify";
import { yCollab, yUndoManagerKeymap } from "y-codemirror.next";
import type * as Y from "yjs";
import type { Awareness } from "y-protocols/awareness";

// ------------------------------------------------------------------ layout

const lineDeco = {} as Record<Kind, Decoration>;
for (const k of [
  "blank", "cue", "parenthetical", "dialogue", "direction", "song_start", "lyric", "lyric_break", "song_end", "transition",
  "scene_heading", "centered", "page_break",
] as Kind[]) {
  lineDeco[k] = Decoration.line({ class: `sw-${k.replace("_", "-")}` });
}
const pendingDeco = Decoration.line({ class: "sw-pending" });

class HintWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  eq(other: HintWidget) {
    return other.text === this.text;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = "sw-hint";
    el.textContent = this.text;
    return el;
  }
  ignoreEvent() {
    return false;
  }
}
const cueHint = Decoration.widget({ widget: new HintWidget("Character name…"), side: 1 });
const markerDeco = Decoration.mark({ class: "sw-marker" });
const emphasisDeco = Decoration.mark({ class: "sw-em" });
const EMPHASIS = /\*([^*\n]+)\*/g;
const EMPHASIS_KINDS: Kind[] = ["dialogue", "direction", "lyric", "parenthetical"];

const MARKERS: Partial<Record<Kind, RegExp>> = {
  lyric: /^~ ?/,
  lyric_break: /^~/,
  song_start: /^~ SONG/,
  song_end: /^~ END SONG/,
  transition: /^> /,
  direction: /^!/,
  cue: /^@/,
  scene_heading: /^\./,
  centered: /^>/,
};

function buildDecorations(view: EditorView): DecorationSet {
  const b = new RangeSetBuilder<Decoration>();
  const { kinds, pending } = info(view.state);
  for (const { from, to } of view.visibleRanges) {
    let pos = from;
    while (pos <= to) {
      const line = view.state.doc.lineAt(pos);
      const k = kinds[line.number - 1] ?? "blank";
      b.add(line.from, line.from, lineDeco[k]);
      if (pending.includes(line.from)) {
        b.add(line.from, line.from, pendingDeco);
        if (line.text === "") b.add(line.from, line.from, cueHint);
      }
      const m = MARKERS[k]?.exec(line.text);
      const prefix = m ? m[0].length : 0;
      if (prefix) b.add(line.from, line.from + prefix, markerDeco);
      if (k === "centered" && line.text.endsWith("<")) b.add(line.to - 1, line.to, markerDeco);
      if (EMPHASIS_KINDS.includes(k) && line.text.includes("*")) {
        // *emphasis*: dim the asterisks, italicise (or un-italicise) the words
        for (const e of line.text.matchAll(EMPHASIS)) {
          const start = line.from + e.index!;
          if (start < line.from + prefix) continue;
          const end = start + e[0].length;
          b.add(start, start + 1, markerDeco);
          b.add(start + 1, end - 1, emphasisDeco);
          b.add(end - 1, end, markerDeco);
        }
      }
      pos = line.to + 1;
    }
  }
  return b.finish();
}

const layout = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildDecorations(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || u.startState.field(scriptField) !== u.state.field(scriptField)) {
        this.decorations = buildDecorations(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

// ------------------------------------------------------------------ typing

/** Cue lines are always upper case; `(` on an empty dialogue line closes itself. */
const typing = EditorView.inputHandler.of((view, from, to, text) => {
  if (text.includes("\n")) return false;
  const kind = kindAt(view.state, from);
  const line = view.state.doc.lineAt(from);
  // The keystroke that makes a line a scene heading ("int" + " ") upper-cases
  // what was typed before it, so the file says INT. and not int.
  const prevBlank = line.number === 1 || view.state.doc.line(line.number - 1).text === "";
  const after = line.text.slice(0, from - line.from) + text + line.text.slice(to - line.from);
  if (to <= line.to && prevBlank && kind !== "scene_heading" && isHeadingText(after) && after !== after.toUpperCase()) {
    view.dispatch({
      changes: { from: line.from, to: line.to, insert: after.toUpperCase() },
      selection: { anchor: from + text.length },
      userEvent: "input.type",
    });
    return true;
  }
  if ((isCueEntry(view.state, from) || kind === "scene_heading") && text !== text.toUpperCase()) {
    view.dispatch({
      changes: { from, to, insert: text.toUpperCase() },
      selection: { anchor: from + text.length },
      userEvent: "input.type",
    });
    return true;
  }
  if (text === "(" && kind === "dialogue" && line.text === "") {
    view.dispatch({ changes: { from, insert: "()" }, selection: { anchor: from + 1 }, userEvent: "input.type" });
    return true;
  }
  return false;
});

// ------------------------------------------------------------------ cue completion

export interface CueSource {
  /** Character cues from the project (characters.yaml + every scene). */
  cues: () => string[];
  /** Locations used in scene headings across the project. */
  places?: () => string[];
}

const SETTINGS = ["INT. ", "EXT. ", "INT./EXT. "];
const TIMES = ["DAY", "NIGHT", "MORNING", "AFTERNOON", "EVENING", "DAWN", "DUSK", "CONTINUOUS", "LATER", "MOMENTS LATER"];

/** Locations from headings in this scene. */
function docPlaces(state: EditorState, skipLine: number): string[] {
  const { kinds } = info(state);
  const out: string[] = [];
  for (let i = 0; i < kinds.length; i++) {
    if (kinds[i] !== "scene_heading" || i + 1 === skipLine) continue;
    const m = state.doc.line(i + 1).text.match(/^\.?(?:INT\.?\/EXT|INT\/EXT|I\/E|INT|EXT|EST)\.?\s+(.*?)(?:\s+-\s+.*)?$/i);
    if (m && m[1]) out.push(m[1].toUpperCase());
  }
  return out;
}

/** INT./EXT., then places used before, then a time of day after " - ". */
function headingCompletion(source: CueSource) {
  return (ctx: CompletionContext) => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    if (kindAt(ctx.state, ctx.pos) !== "scene_heading") return null;
    const before = line.text.slice(0, ctx.pos - line.from);
    const dash = before.lastIndexOf(" - ");
    if (dash >= 0) {
      const from = line.from + dash + 3;
      return { from, options: TIMES.map((t) => ({ label: t, type: "keyword" })), validFor: /^[A-Z ]*$/ };
    }
    const m = before.match(/^(\.?(?:INT\.?\/EXT|INT\/EXT|I\/E|INT|EXT|EST)\.?\s+)/i);
    if (!m) {
      return { from: line.from, options: SETTINGS.map((p) => ({ label: p.trim(), apply: p, type: "keyword" })) };
    }
    const from = line.from + m[1].length;
    const places = [...new Set([...(source.places?.() ?? []), ...docPlaces(ctx.state, line.number)])].filter(Boolean).sort();
    if (!places.length) return null;
    return {
      from,
      options: places.map((p) => ({ label: p, apply: `${p} - `, type: "variable" })),
      // Typing on past " - " moves on to the time of day, so the place list is done.
      validFor: (text: string) => /^[A-Z0-9 '’.&/-]*$/.test(text) && !text.includes(" - "),
    };
  };
}

/**
 * Character names in action / stage directions, where they are written in
 * capitals ("HETTIE crosses to the window"). Starts after two capitals; a name
 * typed in full closes the list, so Enter still starts a new paragraph.
 */
function actionNameCompletion(source: CueSource) {
  return (ctx: CompletionContext) => {
    if (kindAt(ctx.state, ctx.pos) !== "direction") return null;
    const line = ctx.state.doc.lineAt(ctx.pos);
    const before = line.text.slice(0, ctx.pos - line.from);
    const m = before.match(/(?:^|[^\p{L}\p{N}_])([A-Z][A-Z0-9'’.-]+)$/u);
    if (!m) return null;
    const typed = m[1];
    const names = [...new Set([...source.cues(), ...docCues(ctx.state, -1)])].filter(Boolean);
    if (names.includes(typed)) return null;
    const options = names
      .filter((n) => n.startsWith(typed))
      .sort()
      .map((label) => ({ label, type: "variable" }));
    if (!options.length) return null;
    return {
      from: ctx.pos - typed.length,
      options,
      // Keep the list while the typing still could be one of these names.
      validFor: (text: string) => /^[A-Z0-9'’. -]*$/.test(text) && !names.includes(text) && names.some((n) => n.startsWith(text)),
    };
  };
}

/** Character names on cue lines and in action, places and times on scene headings. */
function scriptCompletion(source: CueSource) {
  const cue = cueCompletion(source);
  const heading = headingCompletion(source);
  const action = actionNameCompletion(source);
  return (ctx: CompletionContext) => cue(ctx) ?? heading(ctx) ?? action(ctx);
}

/** Cues already used in this scene, except on `skipLine` (the one being typed). */
function docCues(state: EditorState, skipLine: number): string[] {
  const { kinds } = info(state);
  const out: string[] = [];
  for (let i = 0; i < kinds.length; i++) {
    if (kinds[i] !== "cue" || i + 1 === skipLine) continue;
    const t = state.doc.line(i + 1).text.replace(/^@/, "").replace(/ \([^)]*\)$/, "").trim();
    if (t) for (const n of t.split("&")) out.push(n.trim());
  }
  return out;
}

function cueCompletion(source: CueSource) {
  return (ctx: CompletionContext) => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    if (!isCueEntry(ctx.state, ctx.pos)) return null;
    const typed = line.text.slice(0, ctx.pos - line.from);
    if (!ctx.explicit && typed === "") return null;
    const names = [...new Set([...source.cues(), ...docCues(ctx.state, line.number)])].filter(Boolean);
    const options: Completion[] = names.sort().map((name) => ({
      label: name,
      type: "variable",
      apply: (view: EditorView) => {
        const l = view.state.doc.lineAt(ctx.pos);
        const next = l.number < view.state.doc.lines ? view.state.doc.line(l.number + 1) : null;
        if (next && next.text !== "") {
          view.dispatch({ changes: { from: l.from, to: l.to, insert: name }, selection: { anchor: l.from + name.length + 1 + next.length } });
        } else {
          view.dispatch({
            changes: { from: l.from, to: l.to, insert: name + "\n" },
            selection: { anchor: l.from + name.length + 1 },
          });
        }
      },
    }));
    return { from: line.from, options, validFor: /^[A-Z0-9 &.'’-]*$/ };
  };
}

// ------------------------------------------------------------------ keys

export const ELEMENT_KEYS: { el: ElementType; key: string; label: string }[] = [
  { el: "direction", key: "Mod-1", label: "Direction" },
  { el: "cue", key: "Mod-2", label: "Character" },
  { el: "parenthetical", key: "Mod-3", label: "Parenthetical" },
  { el: "dialogue", key: "Mod-4", label: "Dialogue" },
  { el: "lyric", key: "Mod-5", label: "Lyric" },
  { el: "song", key: "Mod-6", label: "Song" },
  { el: "transition", key: "Mod-7", label: "Transition" },
  { el: "heading", key: "Mod-8", label: "Heading" },
];

/** Run an element conversion on a view (toolbar buttons use this too). */
export function applyElement(view: EditorView, el: ElementType): boolean {
  const ok = asCommand((s) => toElement(s, el))(view);
  if (ok && (el === "cue" || el === "dialogue") && kindAt(view.state) === "cue") startCompletion(view);
  if (ok && el === "heading") startCompletion(view);
  view.focus();
  return ok;
}

const openCuesIfPending = (v: EditorView) => {
  if (kindAt(v.state) === "cue") startCompletion(v);
};

const scriptKeys = Prec.high(
  keymap.of([
    { key: "Tab", run: (v) => (completionStatus(v.state) === "active" ? acceptCompletion(v) : asCommand(tab, openCuesIfPending)(v) || true) },
    { key: "Shift-Tab", run: (v) => asCommand(shiftTab)(v) || true },
    { key: "Enter", run: (v) => (completionStatus(v.state) === "active" ? false : asCommand(enter)(v)) },
    { key: "Shift-Enter", run: insertNewline },
    { key: "Mod-Shift-l", run: asCommand(stanzaBreak) },
    ...ELEMENT_KEYS.map(({ el, key }) => ({ key, run: (v: EditorView) => applyElement(v, el), preventDefault: true })),
    {
      key: "Escape",
      run: (v) => {
        const l = v.state.doc.lineAt(v.state.selection.main.head);
        if (!info(v.state).pending.includes(l.from)) return false;
        v.dispatch({ effects: setPending.of({ pos: l.from, on: false }) });
        return true;
      },
    },
  ]),
);

// ------------------------------------------------------------------ theme

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "var(--script-size, 15px)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--script-font)", lineHeight: "1.6", overflow: "auto" },
  ".cm-content": {
    maxWidth: "62ch",
    margin: "0 auto",
    padding: "48px 0 40vh",
    caretColor: "var(--cursor)",
  },
  // CodeMirror draws its own cursor (by default near-black, lost on a dark page).
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--cursor)", borderLeftWidth: "2px" },
  ".cm-line": { padding: "0 2px" },
  ".cm-activeLine": { backgroundColor: "var(--active-line)" },
  ".sw-direction": { fontStyle: "italic", color: "var(--ink-2)" },
  ".sw-cue": { textAlign: "center", fontWeight: "700", letterSpacing: "0.04em", paddingTop: "0.35em" },
  ".sw-pending": { color: "var(--accent)" },
  ".sw-hint": { opacity: "0.45", fontWeight: "400", letterSpacing: "0", pointerEvents: "none" },
  ".sw-parenthetical": { textAlign: "center", fontStyle: "italic", color: "var(--ink-2)" },
  ".sw-dialogue": { paddingLeft: "12%", paddingRight: "12%" },
  ".sw-lyric, .sw-lyric-break": {
    paddingLeft: "18%",
    fontStyle: "italic",
    boxShadow: "inset 3px 0 0 var(--song)",
  },
  ".sw-song-start, .sw-song-end": {
    textAlign: "center",
    color: "var(--song)",
    fontSize: "0.85em",
    letterSpacing: "0.03em",
  },
  ".sw-transition": { textAlign: "right", fontWeight: "600", letterSpacing: "0.05em" },
  ".sw-scene-heading": { fontWeight: "700", textTransform: "uppercase", paddingTop: "0.6em", letterSpacing: "0.02em" },
  ".sw-centered": { textAlign: "center" },
  ".sw-page-break": { textAlign: "center", color: "var(--ink-3)" },
  ".sw-marker": { opacity: "0.35" },
  ".sw-em": { fontStyle: "italic" },
  ".sw-direction .sw-em, .sw-lyric .sw-em, .sw-parenthetical .sw-em": { fontStyle: "normal", fontWeight: "600" },
  ".cm-placeholder": { fontStyle: "italic" },
  ".cm-tooltip-autocomplete": { fontFamily: "var(--ui-font)" },
  ".cm-ySelectionInfo": { fontFamily: "var(--ui-font)", fontSize: "11px", padding: "1px 4px", borderRadius: "3px", opacity: "1" },
});

export interface SharedText {
  text: Y.Text;
  awareness: Awareness;
  undoManager: Y.UndoManager;
}

export function scriptExtensions(opts: {
  cues: CueSource;
  onChange: (state: EditorState) => void;
  onCursor: (state: EditorState) => void;
  onSave: () => void;
  /** When co-writing: bind to the shared text (and use its undo history). */
  shared?: SharedText | null;
}): Extension[] {
  const shared = opts.shared;
  return [
    scriptField,
    shared ? [yCollab(shared.text, shared.awareness, { undoManager: shared.undoManager }), keymap.of(yUndoManagerKeymap)] : history(),
    drawSelection(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    search({ top: true }),
    EditorView.lineWrapping,
    placeholder("Start with a stage direction, or press Tab to start a speech."),
    // interactionDelay 0: a fast "name + Enter" must accept the name, not split the line.
    // One source, not two: with several sources CodeMirror re-queries the idle
    // one on every keystroke, leaving the list "pending" while it filters.
    autocompletion({
      override: [scriptCompletion(opts.cues)],
      icons: false,
      activateOnTyping: true,
      interactionDelay: 0,
      // After picking a location ("KITCHEN - "), offer the time of day straight away.
      activateOnCompletion: (c) => typeof c.apply === "string" && c.apply.endsWith(" - "),
    }),
    typing,
    layout,
    scriptKeys,
    keymap.of([
      { key: "Mod-s", run: () => (opts.onSave(), true), preventDefault: true },
      ...(shared ? [] : historyKeymap),
      ...searchKeymap,
      ...defaultKeymap,
    ]),
    theme,
    EditorView.updateListener.of((u) => {
      if (u.docChanged) opts.onChange(u.state);
      if (u.docChanged || u.selectionSet) opts.onCursor(u.state);
    }),
  ];
}
