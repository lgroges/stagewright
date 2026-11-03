// Line classifier for the .scene body. This MUST match
// crates/stagewright-core/src/scene.rs (parse_body). classify.test.ts checks it
// line-for-line against the Rust parser's output on every fixture.

export type Kind =
  | "blank"
  | "cue"
  | "parenthetical"
  | "dialogue"
  | "direction"
  | "song_start"
  | "lyric"
  | "lyric_break"
  | "song_end"
  | "transition"
  | "scene_heading"
  | "centered"
  | "page_break";

const LOWER = /\p{Lowercase}/u;
const encoder = new TextEncoder();

/** `ROSALIND (CONT'D)` -> ["ROSALIND", "CONT'D"]. */
function stripExtension(line: string): [string, string | null] {
  const t = line.trimEnd();
  if (t.endsWith(")")) {
    const open = t.lastIndexOf(" (");
    if (open >= 0) {
      const ext = t.slice(open + 2, -1);
      if (!LOWER.test(ext)) return [t.slice(0, open), ext];
    }
  }
  return [t, null];
}

/** Could this line be a character cue (ignoring its neighbours)? */
export function isCueText(line: string): boolean {
  const core = stripExtension(line)[0];
  return (
    core.length > 0 &&
    encoder.encode(core).length <= 60 && // Rust measures bytes
    /[A-Z]/.test(core) &&
    !LOWER.test(core) &&
    !/^[(~>!#]/.test(core)
  );
}

// Longest first, so INT./EXT wins over INT.
const HEADING_PREFIXES = ["INT./EXT", "INT/EXT", "I/E", "INT", "EXT", "EST"];

/** `INT. LIGHTHOUSE - NIGHT`, `ext. car`, or a forced `.FLASHBACK`. */
export function isHeadingText(line: string): boolean {
  if (line.startsWith(".")) return /^[\p{L}\p{N}]/u.test(line.slice(1));
  const head = [...line].slice(0, 9).join("").toUpperCase();
  return HEADING_PREFIXES.some((p) => head.startsWith(p) && (head[p.length] === "." || head[p.length] === " "));
}

function isTransitionText(line: string): boolean {
  const t = line.trimEnd();
  return t.endsWith("TO:") && /[A-Z]/.test(t) && !LOWER.test(t);
}

/** A parenthetical opened on line `i` closes within the next three lines of the speech. Mirrors scene.rs. */
function closesSoon(lines: readonly string[], i: number): boolean {
  for (const l of lines.slice(i + 1, i + 4)) {
    if (l === "" || l.startsWith("(")) return false;
    if (l.endsWith(")")) return true;
  }
  return false;
}

export function classifyLines(lines: readonly string[]): Kind[] {
  const out: Kind[] = new Array(lines.length);
  let inSpeech = false;
  // Inside a parenthetical wrapped onto more lines: "(in Russian;" / "English subtitles)".
  let inParen = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const prevBlank = i === 0 || lines[i - 1] === "";
    const nextNonblank = i + 1 < lines.length && lines[i + 1] !== "";
    let k: Kind;
    if (line === "") {
      inSpeech = false;
      inParen = false;
      k = "blank";
    } else if (line === "~ END SONG") {
      k = "song_end";
    } else if (line.startsWith("~ SONG")) {
      k = "song_start";
    } else if (line === "~") {
      k = "lyric_break";
    } else if (line.startsWith("~ ")) {
      k = "lyric";
    } else if (line.length >= 2 && line.startsWith(">") && line.endsWith("<")) {
      inSpeech = false;
      k = "centered";
    } else if (line.startsWith("> ")) {
      inSpeech = false;
      k = "transition";
    } else if (line.startsWith("!")) {
      inSpeech = false;
      k = "direction";
    } else if (line.startsWith("@")) {
      inSpeech = true;
      k = "cue";
    } else if (/^={3,}$/.test(line)) {
      inSpeech = false;
      k = "page_break";
    } else if (prevBlank && isHeadingText(line)) {
      inSpeech = false;
      k = "scene_heading";
    } else if (prevBlank && !nextNonblank && isTransitionText(line)) {
      inSpeech = false;
      k = "transition";
    } else if (prevBlank && nextNonblank && isCueText(line)) {
      inSpeech = true;
      k = "cue";
    } else if (inSpeech && (inParen || (line.startsWith("(") && (line.endsWith(")") || closesSoon(lines, i))))) {
      inParen = !line.endsWith(")");
      k = "parenthetical";
    } else if (inSpeech) {
      k = "dialogue";
    } else {
      k = "direction";
    }
    out[i] = k;
  }
  return out;
}

/** Split a body the way the Rust parser does (one trailing newline is not a line). */
export function bodyLines(body: string): string[] {
  const b = body.endsWith("\n") ? body.slice(0, -1) : body;
  return b === "" ? [] : b.split("\n");
}

/** Words a performer speaks: parentheticals removed, `*` emphasis ignored. Mirrors analysis.rs. */
export function spokenWords(line: string): number {
  let depth = 0;
  let kept = "";
  for (const ch of line) {
    if (ch === "(") depth++;
    else if (ch === ")" && depth > 0) depth--;
    else if (ch === "*") continue;
    else if (depth === 0) kept += ch;
  }
  return kept.split(/\s+/).filter(Boolean).length;
}

interface LiveStats {
  words: number;
  lyricLines: number;
  speakers: Map<string, number>;
}

export function liveStats(lines: readonly string[], kinds: readonly Kind[]): LiveStats {
  let words = 0;
  let lyricLines = 0;
  const speakers = new Map<string, number>();
  let current: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const k = kinds[i];
    if (k === "cue") {
      const name = stripExtension(lines[i].replace(/^@/, "").replace(/\s*\^\s*$/, ""))[0];
      current = name.split("&").map((s) => s.trim()).filter((s) => s && s !== "ALL");
      for (const s of current) if (!speakers.has(s)) speakers.set(s, 0);
    } else if (k === "dialogue") {
      words += spokenWords(lines[i]);
      for (const s of current) speakers.set(s, (speakers.get(s) ?? 0) + 1);
    } else if (k === "lyric") {
      lyricLines++;
    }
  }
  return { words, lyricLines, speakers };
}

/**
 * Lines whose cue should read (CONT'D): the same character(s) as the speech
 * before, with only action between, in the same scene. Mirrors
 * `continued_cues` in crates/stagewright-core/src/scene.rs.
 */
export function continuedCues(lines: readonly string[], kinds: readonly Kind[]): Set<number> {
  const out = new Set<number>();
  let last: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const k = kinds[i];
    if (k === "cue") {
      let t = lines[i].replace(/^@/, "").trimEnd();
      const dual = t.endsWith("^");
      if (dual) t = t.slice(0, -1).trimEnd();
      const [name, ext] = stripExtension(t);
      const speakers = name.split("&").map((s) => s.trim()).filter(Boolean);
      const group = speakers.every((s) => s === "ALL");
      // Any spelling already typed: (CONT'D), (CONT.), (CONTD), (CONTINUED)…
      const marked = ext !== null && ext.toUpperCase().includes("CONT");
      const same = speakers.length === last.length && speakers.every((s, j) => s === last[j]);
      if (!dual && !group && !marked && last.length > 0 && same) out.add(i);
      last = speakers;
    } else if (k === "scene_heading" || k === "transition" || k === "centered" || k === "page_break") {
      last = [];
    }
  }
  return out;
}
