import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { fixChanges, toDiagnostics, type Issue } from "./check";

const doc = (s: string) => EditorState.create({ doc: s }).doc;
const apply = (s: string, changes: ReturnType<typeof fixChanges>) =>
  EditorState.create({ doc: s }).update({ changes: changes! }).state.doc.toString();

describe("script check fixes", () => {
  it("replaces, inserts and deletes by line", () => {
    const s = "Rain.\n\nHETTIE\n\nWhere is everyone?";
    expect(apply(s, fixChanges(doc(s), [{ op: "delete", line: 3 }], new Map()))).toBe("Rain.\n\nHETTIE\nWhere is everyone?");
    expect(apply(s, fixChanges(doc(s), [{ op: "replace", line: 0, text: "Snow." }], new Map()))).toBe(
      "Snow.\n\nHETTIE\n\nWhere is everyone?",
    );
    expect(apply(s, fixChanges(doc(s), [{ op: "insert_after", line: 4, text: "~ END SONG" }], new Map()))).toBe(
      s + "\n~ END SONG",
    );
    expect(apply(s, fixChanges(doc(s), [{ op: "insert_before", line: 0, text: "~ SONG \"\" []" }], new Map()))).toBe(
      "~ SONG \"\" []\n" + s,
    );
    // Deleting the last line takes the newline before it.
    expect(apply("A\nB", fixChanges(doc("A\nB"), [{ op: "delete", line: 1 }], new Map()))).toBe("A");
  });

  it("won't apply a fix to lines that changed since the check", () => {
    const s = "\nHETIE\nHi.";
    const expected = new Map([[1, "HETIE"]]);
    expect(fixChanges(doc(s), [{ op: "replace", line: 1, text: "HETTIE" }], expected)).not.toBeNull();
    expect(fixChanges(doc("\nHETTIE\nHi."), [{ op: "replace", line: 1, text: "HETTIE" }], expected)).toBeNull();
  });

  it("underlines the exact words, counting characters not bytes", () => {
    const issue: Issue = {
      code: "cue-unknown",
      severity: "warning",
      line: 1,
      from: 9,
      to: 14,
      message: "HETIE isn't in the cast list.",
      fix: { label: "Change to HETTIE", edits: [{ op: "replace", line: 1, text: "TOBÍAS & HETTIE" }] },
    };
    const d = doc("Rain.\nTOBÍAS & HETIE\nHi.");
    const [diag] = toDiagnostics(d, [issue], () => {});
    expect(d.sliceString(diag.from, diag.to)).toBe("HETIE");
    expect(diag.severity).toBe("warning");
    expect(diag.actions!.map((a) => a.name)).toEqual(["Change to HETTIE", "Don't check for this"]);
    // A whole-line issue covers the line.
    const [whole] = toDiagnostics(d, [{ ...issue, from: 0, to: 0, fix: null }], () => {});
    expect(d.sliceString(whole.from, whole.to)).toBe("TOBÍAS & HETIE");
    expect(whole.actions!.map((a) => a.name)).toEqual(["Don't check for this"]);
  });
});
