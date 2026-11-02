// The script check in the editor: issues from the Rust checker become
// underlines, with the fix (if there is one) and "Don't check for this" as
// actions in the tooltip.

import { StateEffect, type EditorState, type Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { forEachDiagnostic, linter, type Action, type Diagnostic } from "@codemirror/lint";

export type Edit =
  | { op: "replace"; line: number; text: string }
  | { op: "insert_before"; line: number; text: string }
  | { op: "insert_after"; line: number; text: string }
  | { op: "delete"; line: number };

export interface Issue {
  code: string;
  severity: "warning" | "info";
  /** Body line, from 0. */
  line: number;
  /** Columns in characters; equal means the whole line. */
  from: number;
  to: number;
  message: string;
  fix: { label: string; edits: Edit[] } | null;
}

export interface CheckSource {
  /** Check this text (the scene as it is in the editor). Null: no scene. */
  check: (body: string) => Promise<Issue[] | null>;
  /** Switch a check off for the project. */
  off: (code: string) => void;
}

/** Character column → offset in a line (UTF-16, as CodeMirror counts). */
function offsetOf(lineText: string, col: number): number {
  return Array.from(lineText).slice(0, col).join("").length;
}

/** The document changes for a fix, or null if the lines it was worked out on have changed since. */
export function fixChanges(doc: Text, edits: Edit[], expected: Map<number, string>) {
  for (const [n, text] of expected) {
    if (n >= doc.lines || doc.line(n + 1).text !== text) return null;
  }
  const changes: { from: number; to?: number; insert?: string }[] = [];
  for (const e of edits) {
    const l = doc.line(e.line + 1);
    switch (e.op) {
      case "replace":
        changes.push({ from: l.from, to: l.to, insert: e.text });
        break;
      case "insert_before":
        changes.push({ from: l.from, insert: e.text + "\n" });
        break;
      case "insert_after":
        changes.push({ from: l.to, insert: "\n" + e.text });
        break;
      case "delete":
        if (l.number < doc.lines) changes.push({ from: l.from, to: l.to + 1 });
        else if (l.number > 1) changes.push({ from: l.from - 1, to: l.to });
        else changes.push({ from: l.from, to: l.to });
        break;
    }
  }
  return changes;
}

/** Issues as CodeMirror diagnostics on `doc`. */
export function toDiagnostics(doc: Text, issues: Issue[], off: (code: string) => void): Diagnostic[] {
  const out: Diagnostic[] = [];
  for (const i of issues) {
    if (i.line >= doc.lines) continue;
    const line = doc.line(i.line + 1);
    const whole = i.from === i.to;
    const from = whole ? line.from : line.from + offsetOf(line.text, i.from);
    const to = whole ? line.to : line.from + offsetOf(line.text, i.to);
    // The fix was worked out on these lines; it only applies while they are unchanged.
    const expected = new Map<number, string>();
    for (const e of i.fix?.edits ?? []) if (e.line < doc.lines) expected.set(e.line, doc.line(e.line + 1).text);
    const actions: Action[] = [];
    if (i.fix) {
      const fix = i.fix;
      actions.push({
        name: fix.label,
        apply: (view: EditorView) => {
          const changes = fixChanges(view.state.doc, fix.edits, expected);
          if (changes) view.dispatch({ changes, userEvent: "input.fix" });
        },
      });
    }
    actions.push({ name: "Don't check for this", apply: () => off(i.code) });
    out.push({
      from,
      to: Math.max(from, to),
      severity: i.severity === "warning" ? "warning" : "info",
      message: i.message,
      source: i.code,
      actions,
    });
  }
  return out;
}

/** Dispatch to check again without an edit (settings or the cast list changed). */
export const recheck = StateEffect.define<null>();

/** The linter extension: checks the scene shortly after typing stops. */
export function scriptCheck(source: CheckSource) {
  return linter(
    async (view: EditorView) => {
      const doc = view.state.doc;
      const issues = await source.check(doc.toString()).catch(() => null);
      // The text may have changed while checking; the next run will catch up.
      if (!issues || view.state.doc !== doc) return [];
      return toDiagnostics(doc, issues, source.off);
    },
    { delay: 600, needsRefresh: (u) => u.transactions.some((t) => t.effects.some((e) => e.is(recheck))) },
  );
}

/** Warnings and notes currently shown, for the status bar. */
export function countIssues(state: EditorState) {
  let warnings = 0;
  let infos = 0;
  forEachDiagnostic(state, (d) => (d.severity === "warning" ? warnings++ : infos++));
  return { warnings, infos };
}
