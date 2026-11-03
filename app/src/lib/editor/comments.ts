// Comments in the editor: highlighted passages that follow the text as it is
// edited, and the anchoring that finds a stored passage again in a scene.
//
// A comment is stored (comments.yaml) as the quoted text plus a little context
// either side and a line hint. When a scene opens, each comment is located;
// while it's open, CodeMirror maps the highlight through every edit (local or
// a co-writer's), and the editor stores the new quote when it has moved. If
// the passage is deleted entirely the highlight disappears and the comment is
// shown as detached, keeping its last quote.

import { RangeSet, StateEffect, StateField, type EditorState, type Range } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import DiffMatchPatch from "diff-match-patch";
import type { CommentAnchor } from "../api";

const CONTEXT = 40;

/** Where a passage is in a scene body (character offsets). */
export interface Span {
  from: number;
  to: number;
}

function lineStart(text: string, line: number): number {
  let pos = 0;
  for (let i = 0; i < line; i++) {
    const n = text.indexOf("\n", pos);
    if (n < 0) return text.length;
    pos = n + 1;
  }
  return pos;
}

function lineOf(text: string, pos: number): number {
  let n = 0;
  for (let i = text.indexOf("\n"); i >= 0 && i < pos; i = text.indexOf("\n", i + 1)) n++;
  return n;
}

/** How many characters `a` and `b` share at their ends (suffix) or starts (prefix). */
function sharedEnd(a: string, b: string): number {
  let n = 0;
  while (n < a.length && n < b.length && a[a.length - 1 - n] === b[b.length - 1 - n]) n++;
  return n;
}
function sharedStart(a: string, b: string): number {
  let n = 0;
  while (n < a.length && n < b.length && a[n] === b[n]) n++;
  return n;
}

/** The stored description of the passage `from`..`to` of `text`. */
export function anchorAt(text: string, from: number, to: number): Omit<CommentAnchor, "id"> {
  return {
    quote: text.slice(from, to),
    before: text.slice(Math.max(0, from - CONTEXT), from),
    after: text.slice(to, to + CONTEXT),
    line: lineOf(text, from),
  };
}

const dmp = new DiffMatchPatch();

/**
 * Find a stored passage in `text`: the exact quote whose surroundings match
 * best (nearest the line hint on a tie), or, if the quote itself was edited
 * outside the editor, a close match of its start and end. Null if it's gone.
 */
export function locate(text: string, a: Omit<CommentAnchor, "id">): Span | null {
  const q = a.quote;
  if (!q) return null;
  let best: { at: number; score: number; dist: number } | null = null;
  for (let at = text.indexOf(q); at >= 0; at = text.indexOf(q, at + 1)) {
    const score =
      sharedEnd(text.slice(Math.max(0, at - a.before.length), at), a.before) +
      sharedStart(text.slice(at + q.length, at + q.length + a.after.length), a.after);
    const dist = Math.abs(lineOf(text, at) - a.line);
    if (!best || score > best.score || (score === best.score && dist < best.dist)) best = { at, score, dist };
  }
  if (best) return { from: best.at, to: best.at + q.length };
  // Fuzzy: find the start, then the end, near where the passage was.
  dmp.Match_Threshold = 0.35;
  dmp.Match_Distance = 2000;
  const head = q.slice(0, 32);
  const expected = Math.min(text.length, lineStart(text, a.line) + Math.max(0, a.before.length - CONTEXT));
  const start = dmp.match_main(text, head, expected);
  if (start < 0) return null;
  if (q.length <= 32) return { from: start, to: Math.min(text.length, start + q.length) };
  const tail = q.slice(-32);
  const end = dmp.match_main(text, tail, start + q.length - tail.length);
  if (end < 0 || end + tail.length <= start || end + tail.length - start > q.length * 2 + 40) {
    return { from: start, to: Math.min(text.length, start + head.length) };
  }
  return { from: start, to: end + tail.length };
}

// ------------------------------------------------------------------ the editor extension

export interface CommentMark extends Span {
  id: string;
  resolved: boolean;
}

/** Replace the highlighted passages (after loading or when comments change). */
export const setCommentMarks = StateEffect.define<CommentMark[]>();
/** Emphasise one comment's passage (or none). */
export const setActiveComment = StateEffect.define<string | null>();
/** Show or hide resolved comments' highlights (they're tracked either way). */
export const setShowResolved = StateEffect.define<boolean>();

interface MarkSpec {
  id: string;
  resolved: boolean;
}

interface CommentsState {
  marks: RangeSet<Decoration>;
  active: string | null;
  showResolved: boolean;
}

function mark(m: MarkSpec, active: string | null, showResolved: boolean): Decoration {
  const cls = [
    "sw-comment",
    m.resolved ? (showResolved ? "sw-comment-resolved" : "sw-comment-hidden") : "",
    m.id === active ? "sw-comment-active" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return Decoration.mark({ class: cls, attributes: { "data-comment-id": m.id }, spec: m });
}

function rebuild(set: RangeSet<Decoration>, active: string | null, showResolved: boolean): RangeSet<Decoration> {
  const out: Range<Decoration>[] = [];
  const it = set.iter();
  for (; it.value; it.next()) out.push(mark(it.value.spec.spec as MarkSpec, active, showResolved).range(it.from, it.to));
  return RangeSet.of(out, true);
}

export const commentsField = StateField.define<CommentsState>({
  create: () => ({ marks: Decoration.none, active: null, showResolved: false }),
  update(v, tr) {
    let { marks, active, showResolved } = v;
    marks = marks.map(tr.changes);
    let restyle = false;
    for (const e of tr.effects) {
      if (e.is(setCommentMarks)) {
        const len = tr.state.doc.length;
        const ranges = e.value
          .map((m) => ({ ...m, from: Math.max(0, Math.min(m.from, len)), to: Math.max(0, Math.min(m.to, len)) }))
          .filter((m) => m.to > m.from)
          .map((m) => mark(m, active, showResolved).range(m.from, m.to));
        marks = RangeSet.of(ranges, true);
      } else if (e.is(setActiveComment)) {
        active = e.value;
        restyle = true;
      } else if (e.is(setShowResolved)) {
        showResolved = e.value;
        restyle = true;
      }
    }
    if (restyle) marks = rebuild(marks, active, showResolved);
    return { marks, active, showResolved };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.marks as DecorationSet),
});

/** Where each comment's passage is now. A comment whose text was deleted is absent. */
export function commentSpans(state: EditorState): Map<string, Span> {
  const out = new Map<string, Span>();
  const it = state.field(commentsField, false)?.marks.iter();
  if (!it) return out;
  for (; it.value; it.next()) {
    const id = (it.value.spec.spec as MarkSpec).id;
    const cur = out.get(id);
    // Marks can be split by edits inside them; take the whole extent.
    out.set(id, cur ? { from: Math.min(cur.from, it.from), to: Math.max(cur.to, it.to) } : { from: it.from, to: it.to });
  }
  return out;
}

/** The comment highlighted at `pos`, the innermost (shortest) if several overlap. */
function commentAt(state: EditorState, pos: number): string | null {
  let best: { id: string; len: number } | null = null;
  for (const [id, s] of commentSpans(state)) {
    if (s.from <= pos && pos <= s.to && (!best || s.to - s.from < best.len)) best = { id, len: s.to - s.from };
  }
  return best?.id ?? null;
}

const commentTheme = EditorView.baseTheme({
  ".sw-comment": {
    backgroundColor: "color-mix(in srgb, #f2c94c 30%, transparent)",
    borderBottom: "2px solid color-mix(in srgb, #e0a800 70%, transparent)",
    cursor: "pointer",
  },
  ".sw-comment-active": {
    backgroundColor: "color-mix(in srgb, #f2c94c 60%, transparent)",
  },
  ".sw-comment-resolved": {
    backgroundColor: "transparent",
    borderBottom: "1px dashed color-mix(in srgb, #e0a800 50%, transparent)",
  },
  ".sw-comment-hidden": {
    backgroundColor: "transparent",
    borderBottom: "none",
    cursor: "text",
  },
});

/**
 * The extension: the highlights, and a click on one selects its comment.
 * `onPick` gets the comment under the cursor whenever the selection moves.
 */
export function comments(onPick: (id: string | null) => void) {
  return [
    commentsField,
    commentTheme,
    EditorView.updateListener.of((u) => {
      if (!u.selectionSet || !u.view.hasFocus) return;
      const sel = u.state.selection.main;
      if (!sel.empty) return;
      const id = commentAt(u.state, sel.head);
      const shown = id && !isHiddenResolved(u.state, id) ? id : null;
      if (shown !== u.state.field(commentsField).active) onPick(shown);
    }),
  ];
}

function isHiddenResolved(state: EditorState, id: string): boolean {
  const it = state.field(commentsField).marks.iter();
  for (; it.value; it.next()) {
    const m = it.value.spec.spec as MarkSpec;
    if (m.id === id) return m.resolved && !state.field(commentsField).showResolved;
  }
  return false;
}
