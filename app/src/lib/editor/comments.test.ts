import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { anchorAt, commentSpans, commentsField, locate, setCommentMarks } from "./comments";

const scene = "A fire, a kettle, a fiddle.\n\nHETTIE\nA kettle? A kettle.\n\nShe fills a kettle.";

describe("anchoring", () => {
  it("finds the passage by its surroundings when the words repeat", () => {
    const at = scene.lastIndexOf("a kettle");
    const a = anchorAt(scene, at, at + "a kettle".length);
    expect(a.line).toBe(5);
    expect(locate(scene, a)).toEqual({ from: at, to: at + 8 });
    const first = scene.indexOf("a kettle");
    expect(locate(scene, anchorAt(scene, first, first + 8))).toEqual({ from: first, to: first + 8 });
  });

  it("follows the passage after edits elsewhere", () => {
    const at = scene.indexOf("She fills");
    const a = anchorAt(scene, at, at + 9);
    const edited = "INT. COTTAGE - DAY\n\n" + scene.replace("A fire", "A roaring fire");
    const s = locate(edited, a)!;
    expect(edited.slice(s.from, s.to)).toBe("She fills");
  });

  it("finds a passage that was itself edited outside the editor, or gives up", () => {
    const long = "The beam sweeps over the whole town gathered with lanterns at the foot of the lighthouse.";
    const a = anchorAt(long, 0, long.length);
    const edited = long.replace("whole town", "entire town");
    const s = locate(edited, a)!;
    expect(s.from).toBe(0);
    expect(s.to).toBe(edited.length);
    expect(locate("Nothing like it here.", a)).toBeNull();
  });
});

describe("highlights", () => {
  it("move with the text and vanish when it is deleted", () => {
    let st = EditorState.create({ doc: scene, extensions: [commentsField] });
    const at = scene.indexOf("a fiddle");
    st = st.update({ effects: setCommentMarks.of([{ id: "c1", from: at, to: at + 8, resolved: false }]) }).state;
    st = st.update({ changes: { from: 0, insert: "Night. " } }).state;
    expect(commentSpans(st).get("c1")).toEqual({ from: at + 7, to: at + 15 });
    st = st.update({ changes: { from: at + 7, to: at + 15 } }).state;
    expect(commentSpans(st).has("c1")).toBe(false);
  });
});
