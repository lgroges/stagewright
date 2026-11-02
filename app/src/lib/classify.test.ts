import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { bodyLines, classifyLines, continuedCues, liveStats, spokenWords } from "./classify";
import snapshot from "./__fixtures__/classify.json";

// classify.json is produced by the Rust parser (`npm run parity`). CI regenerates
// it and fails if it changes, so this test pins the TS classifier to Rust.

const root = join(__dirname, "../../..");
function sceneFiles(): string[] {
  const out: string[] = [];
  const acts = join(root, "fixtures/stage-sample/acts");
  for (const a of readdirSync(acts)) for (const f of readdirSync(join(acts, a))) out.push(join(acts, a, f));
  for (const f of readdirSync(join(__dirname, "__fixtures__"))) if (f.endsWith(".scene")) out.push(join(__dirname, "__fixtures__", f));
  return out;
}

function body(path: string): string {
  const src = readFileSync(path, "utf8");
  const end = src.indexOf("\n---\n", 4);
  return src.slice(end + 5);
}

describe("classifier parity with the Rust parser", () => {
  const snap = snapshot as Record<string, string[]>;
  for (const path of sceneFiles()) {
    const name = path.split("/").pop()!;
    it(name, () => {
      expect(snap[name], `missing from classify.json — run npm run parity`).toBeDefined();
      expect(classifyLines(bodyLines(body(path)))).toEqual(snap[name]);
    });
  }
});

describe("live stats", () => {
  it("counts spoken words like analysis.rs", () => {
    expect(spokenWords("Very well… (turning to the villagers) People of *Briton*!")).toBe(5);
  });
  it("counts speeches per speaker, splitting duets and ignoring ALL", () => {
    const lines = bodyLines("A.\n\nTOBIAS & HETTIE\nAmen.\n\nALL\nAmen.\n\nTOBIAS\nHi.\nAgain.\n");
    const s = liveStats(lines, classifyLines(lines));
    expect(Object.fromEntries(s.speakers)).toEqual({ TOBIAS: 3, HETTIE: 1 });
    expect(s.words).toBe(4);
  });
});

describe("continued cues (same cases as the Rust test)", () => {
  it("marks the same speaker after action, as Rust does", () => {
    const body =
      "HETTIE\nHello.\n\nShe turns.\n\nHETTIE\nStill here.\n\nTOBIAS\nYes.\n\nHETTIE\nWell.\n\nHETTIE (V.O.)\nAnd again.\n\nHETTIE (CONT'D)\nTyped.\n\nShe sits.\n\nHETTIE (CONT.)\nTyped the other way.\n\nINT. HALL - DAY\n\nHETTIE\nNew scene.\n\nALL\nYes!\n\nALL\nNo!\n\nTOBIAS & HETTIE\nTogether.\n\nTOBIAS & HETTIE\nAgain.\n\nTOBIAS ^\nAt once.";
    const lines = body.split("\n");
    const marked = [...continuedCues(lines, classifyLines(lines))].map((i) => lines[i]);
    expect(marked).toEqual(["HETTIE", "HETTIE (V.O.)", "TOBIAS & HETTIE"]);
  });
});
