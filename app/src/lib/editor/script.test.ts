import { describe, expect, it } from "vitest";
import { EditorState, type TransactionSpec } from "@codemirror/state";
import { enter, info, isCueEntry, kindAt, scriptField, shiftTab, stanzaBreak, startSpeech, tab, toElement, type ElementType } from "./script";
import { classifyLines } from "../classify";

/** Build a state from text with `|` marking the cursor. */
function st(marked: string): EditorState {
  const at = marked.indexOf("|");
  const doc = marked.replace("|", "");
  return EditorState.create({ doc, selection: { anchor: at }, extensions: [scriptField] });
}

function show(s: EditorState): string {
  const d = s.doc.toString();
  const h = s.selection.main.head;
  return d.slice(0, h) + "|" + d.slice(h);
}

function apply(s: EditorState, fn: (s: EditorState) => TransactionSpec | null): EditorState {
  const spec = fn(s);
  if (!spec) throw new Error("command did not apply");
  return s.update(spec).state;
}

function type(s: EditorState, text: string): EditorState {
  const h = s.selection.main.head;
  // mimic the editor's cue upper-casing input handler
  const upper = kindAt(s) === "cue" ? text.toUpperCase() : text;
  return s.update({ changes: { from: h, insert: upper }, selection: { anchor: h + upper.length } }).state;
}

const el = (t: ElementType) => (s: EditorState) => toElement(s, t);

describe("Enter", () => {
  it("after a cue starts the dialogue line", () => {
    expect(show(apply(st("Rain.\n\nMORDECAI|"), (s) => toElement(s, "cue")).update({}).state)).toBe("Rain.\n\nMORDECAI|");
    const s = apply(apply(st("Rain.\n\nmordecai|"), el("cue")), enter);
    expect(show(s)).toBe("Rain.\n\nMORDECAI\n|");
  });
  it("at the end of the last dialogue line opens a new paragraph", () => {
    expect(show(apply(st("KING\nHello.|"), enter))).toBe("KING\nHello.\n\n|");
  });
  it("in the middle of a speech adds a line to it", () => {
    expect(show(apply(st("KING\nHello.|\n(beat)\nAgain."), enter))).toBe("KING\nHello.\n|\n(beat)\nAgain.");
  });
  it("after a parenthetical goes to dialogue", () => {
    expect(show(apply(st("KING\n(quietly)|"), enter))).toBe("KING\n(quietly)\n|");
  });
  it("after a direction opens a new paragraph", () => {
    expect(show(apply(st("Rain falls.|"), enter))).toBe("Rain falls.\n\n|");
  });
  it("mid-line is left to the default newline", () => {
    expect(enter(st("Rain| falls."))).toBeNull();
  });
  it("continues a song, and an empty lyric line ends it", () => {
    let s = apply(st('HETTIE\n~ SONG "X" [HETTIE]\n~ Oh lantern|'), enter);
    expect(show(s)).toBe('HETTIE\n~ SONG "X" [HETTIE]\n~ Oh lantern\n~ |');
    s = apply(s, enter);
    expect(show(s)).toBe('HETTIE\n~ SONG "X" [HETTIE]\n~ Oh lantern\n~ END SONG\n\n|');
    expect(classifyLines(s.doc.toString().split("\n"))).toEqual(["cue", "song_start", "lyric", "song_end", "blank", "blank"]);
  });
});

describe("Enter in a song with its END line already present", () => {
  it("does not add a second END SONG", () => {
    const s = apply(st('HETTIE\n~ SONG "X" [HETTIE]\n~ one\n~ |\n~ END SONG'), enter);
    expect(show(s)).toBe('HETTIE\n~ SONG "X" [HETTIE]\n~ one\n~ END SONG\n\n|');
  });
});

describe("Tab", () => {
  it("on an empty line starts a speech, with a blank line above", () => {
    const s = apply(st("Rain falls.\n|"), tab);
    expect(show(s)).toBe("Rain falls.\n\n|");
    expect(kindAt(s)).toBe("cue");
    expect(info(s).pending.length).toBe(1);
  });
  it("on a typed direction turns it into a cue", () => {
    const s = apply(st("Rain.\n\nmordecai|"), tab);
    expect(show(s)).toBe("Rain.\n\nMORDECAI|");
    expect(kindAt(s)).toBe("cue");
  });
  it("toggles dialogue and parenthetical", () => {
    let s = apply(st("KING\nquietly|"), tab);
    expect(show(s)).toBe("KING\n(quietly)|");
    s = apply(s, tab);
    expect(show(s)).toBe("KING\nquietly|");
  });
  const underCue = () => apply(type(apply(st("|"), tab), "king"), enter); // "KING\n|" with KING pending
  it("on the empty dialogue line under a cue makes an empty parenthetical", () => {
    expect(show(underCue())).toBe("KING\n|");
    expect(kindAt(underCue())).toBe("dialogue");
    const s = apply(underCue(), tab);
    expect(show(s)).toBe("KING\n(|)");
    expect(kindAt(s)).toBe("parenthetical");
  });
  it("Enter on the empty dialogue line under a cue leaves the speech", () => {
    expect(show(apply(underCue(), enter))).toBe("KING\n\n|");
  });
});

describe("pending cues", () => {
  it("stop being pending once their dialogue is written", () => {
    let s = apply(st("|"), tab);
    s = type(s, "king");
    s = apply(s, enter);
    expect(info(s).pending.length).toBe(1);
    s = type(s, "Hello.");
    expect(info(s).pending.length).toBe(0);
    expect(kindAt(s, 0)).toBe("cue");
  });
});

describe("cue entry (upper-casing and name completion)", () => {
  it("is off for a lone capital typed above another paragraph", () => {
    const s = st("Rain.\n\nT|\nThe lighthouse.");
    expect(kindAt(s)).toBe("cue"); // the parser's view, briefly
    expect(isCueEntry(s, s.selection.main.head)).toBe(false);
  });
  it("is on for a started speech and for editing an existing cue", () => {
    expect(isCueEntry(apply(st("|"), tab), 0)).toBe(true);
    expect(isCueEntry(st("Rain.\n\nKI|NG\nHello."), 9)).toBe(true);
  });
});

describe("Shift-Tab", () => {
  it("turns a parenthetical back into dialogue", () => {
    expect(show(apply(st("KING\n(aside)|"), shiftTab))).toBe("KING\naside|");
  });
  it("turns a cue back into a direction", () => {
    const s = apply(st("Rain.\n\nKING|\nHello."), shiftTab);
    expect(s.doc.toString()).toBe("Rain.\n\nKing\n\nHello.");
  });
});

describe("element toolbar", () => {
  it("dialogue outside a speech asks for a cue above it", () => {
    const s = apply(st("Rain.\n\nWhere is everyone?|"), el("dialogue"));
    expect(show(s)).toBe("Rain.\n\n|\nWhere is everyone?");
    expect(kindAt(s)).toBe("cue");
    const typed = type(s, "rosalind");
    expect(typed.doc.toString()).toBe("Rain.\n\nROSALIND\nWhere is everyone?");
    expect(classifyLines(typed.doc.toString().split("\n"))).toEqual(["direction", "blank", "cue", "dialogue"]);
  });
  it("direction inside a speech splits it off with blank lines", () => {
    const s = apply(st("KING\nHello.\nHe sits.|\nGoodbye."), el("direction"));
    expect(s.doc.toString()).toBe("KING\nHello.\n\nHe sits.\n\nGoodbye.");
  });
  it("lyric outside a song wraps it in a song for the current singer", () => {
    const s = apply(st("HETTIE\nOh lantern|"), el("lyric"));
    expect(s.doc.toString()).toBe('HETTIE\n~ SONG "" [HETTIE]\n~ Oh lantern\n~ END SONG');
    expect(classifyLines(s.doc.toString().split("\n"))).toEqual(["cue", "song_start", "lyric", "song_end"]);
  });
  it("song inserts a whole number with the cursor in the title", () => {
    const s = apply(st("HETTIE\n(singing)|"), el("song"));
    expect(show(s)).toBe('HETTIE\n(singing)\n~ SONG "|" [HETTIE]\n~ \n~ END SONG');
  });
  it("song on an empty line outside a speech stands alone", () => {
    const s = apply(st("Music.\n|"), el("song"));
    expect(show(s)).toBe('Music.\n\n~ SONG "|" []\n~ \n~ END SONG');
  });
  it("transition is upper-cased and fenced off", () => {
    expect(apply(st("Rain.\nblackout|\nMore."), el("transition")).doc.toString()).toBe("Rain.\n\n> BLACKOUT\n\nMore.");
  });
  it("parenthetical is refused outside a speech", () => {
    expect(toElement(st("Rain.|"), "parenthetical")).toBeNull();
  });
  it("stanza break inside a song", () => {
    const s = apply(st('~ SONG "X" []\n~ line one|'), stanzaBreak);
    expect(show(s)).toBe('~ SONG "X" []\n~ line one\n~\n~ |');
  });
});

describe("scene headings", () => {
  it("heading on an empty line starts INT. with blank lines around", () => {
    const s = apply(st("Rain.\n|"), el("heading"));
    expect(show(s)).toBe("Rain.\n\nINT. |");
    expect(kindAt(s)).toBe("scene_heading");
  });
  it("heading keeps a typed prefix and upper-cases", () => {
    expect(apply(st("ext. beach - day|"), el("heading")).doc.toString()).toBe("EXT. BEACH - DAY");
    expect(apply(st("kitchen - night|"), el("heading")).doc.toString()).toBe("INT. KITCHEN - NIGHT");
  });
  it("Enter after a heading opens the action paragraph", () => {
    expect(show(apply(st("INT. HALL - DAY|"), enter))).toBe("INT. HALL - DAY\n\n|");
  });
  it("Shift-Tab turns a heading back into action", () => {
    const s = apply(st("\nINT. HALL|"), shiftTab);
    expect(s.doc.toString()).toBe("\n!INT. HALL");
    expect(kindAt(s)).toBe("direction");
  });
});

describe("drafting a whole scene with keys only", () => {
  it("produces a well-formed scene", () => {
    let s = st("|");
    s = type(s, "The lighthouse. Rain.");
    s = apply(s, enter); // new paragraph
    s = apply(s, tab); // start speech
    s = type(s, "mordecai"); // upper-cased as a cue
    s = apply(s, enter); // to dialogue
    s = type(s, "reading");
    s = apply(s, tab); // -> (reading)
    s = apply(s, enter); // dialogue line
    s = type(s, "The Admiral sends his regards.");
    s = apply(s, enter); // end speech
    s = apply(s, startSpeech);
    s = type(s, "rosalind");
    s = apply(s, enter);
    s = type(s, "And no more ships?");
    s = apply(s, enter);
    s = type(s, "blackout");
    s = apply(s, el("transition"));
    const doc = s.doc.toString();
    expect(doc).toBe(
      "The lighthouse. Rain.\n\nMORDECAI\n(reading)\nThe Admiral sends his regards.\n\nROSALIND\nAnd no more ships?\n\n> BLACKOUT",
    );
    expect(classifyLines(doc.split("\n"))).toEqual([
      "direction", "blank", "cue", "parenthetical", "dialogue", "blank", "cue", "dialogue", "blank", "transition",
    ]);
  });
});
