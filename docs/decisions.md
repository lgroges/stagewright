# Decisions

Choices made about Stagewright and why, newest first. Each entry: what we chose,
the reason, and what we didn't do. Ideas still open or turned down are in
[ideas.md](ideas.md).

## 2026-11-03 · A clean-up sweep

A pass over the whole codebase for needless work and duplication.

- **The script check is a `Scan` with one method per rule group** (cues, directions,
  headings, parentheticals, songs, transitions) instead of one 800-line function.
  Behaviour is unchanged; the tests and every message are the same.
- **As you type, earlier scenes are only read for introductions**
  (`note_introductions`), not fully checked: about four times faster on a 144-scene
  series. Cast names for the check come straight from the cues (`speakers_in`), with
  no page layout.
- **Grammar results are cached by paragraph**, since most paragraphs don't change
  between checks. Camera terms are sorted once, not on every line.
- **One copy of shared helpers**: act names (`ProjectMeta::act_name`), `title_case`,
  the group cues (ALL), double-spaced dialogue, and the end-to-end suites' helpers
  (`app/e2e/lib.cjs`, which also waits for each test server to be listening rather
  than sleeping).
- Saving a scene no longer loads the project twice; the grammar checker is warmed up
  once per project open, not twice.
- App: names exported but only used in their own file are no longer exported, and an
  unused dependency (@lezer/highlight) is gone.
- Left for later (see ideas.md): every API call still loads the whole project, and
  `ops.rs` and the app's `state.svelte.ts` could be split into smaller modules.

## 2026-11-03 · A grammar checker: Harper, locally

The script check gets grammar from Harper (harper-core, Apache-2.0), a Rust grammar
checker that runs on the writer's computer.

- **Why Harper**: it's local and private (unpublished scripts never leave the
  machine), fast (a scene in tens of milliseconds), and it's Rust, so it lives in the
  core with the other checks. We didn't use LanguageTool, which is broader but needs
  Java locally or sends text to its server.
- **Action and directions only.** Dialogue is how characters speak, and period dialogue
  (the 1559 burial service, say) sets off false alarms such as who/whom.
- **What's left out of Harper**: spelling (the system spellchecker does it and knows
  the writer's words), readability and style advice, repetition (our own check, which
  allows "had had"), its who/whom rule (wrong on "the front right of whom is…"), and
  anything on words in capitals (names, places, camera terms).
- **House rules** for British usage Harper misses: "was sat"/"been stood", split
  reflexives ("him self"), "next the".
- **British by default**, `dialect:` in project.yaml for others. No setting in the app
  yet; it rarely changes.
- **Cost**: about 13 MB on the app, and most of a second to start, so it's built in
  the background when a project opens. Dependencies are optimised in dev builds too.
  As you type, only the open scene is grammar-checked.

## 2026-11-03 · A format reference, and a skill that reads it

So Claude can read Stagewright projects in any session (for research on a play, say),
the file format is written up in docs/format.md, with a format version. Liam's
`stagewright-projects` skill (source in docs/skill/SKILL.md) fetches that page live
and carries a summary as a fallback, so most format changes need no skill update;
when the summary would mislead, the skill is updated and re-proposed. We didn't put
the whole reference in the skill, because a saved skill can only change when Liam
saves a new version.

## 2026-11-03 · Layouts, scene numbers, and two more checks

- **Layouts are a project setting (`layout:`), chosen under Export**, and apply only to
  the formats they belong to: UK stage to stage plays, BBC studio to TV. Running time
  and page counts still use the standard layout, which is what timings are based on;
  Fountain exports are always standard.
- **UK stage** follows the BBC stage guide. A character carrying on after a direction
  isn't named again (the guide's rule), worked out the same way as automatic (CONT'D).
- **BBC studio** follows the taped drama and sitcom guides. Speeches move whole to the
  next page rather than splitting with (MORE). CUT TO: is added after a scene unless
  the writer wrote a transition. The studio layout's capitals parentheticals and
  CUT TO: aren't flagged by the check.
- **Shooting scripts number every heading** in both margins, counting through each
  episode; a typed `#12#` sets the number and the count goes on from it.
- **Repeated words are only checked in action and directions.** In dialogue a repeat
  is often meant ("no no no"). "had had" and "that that" are allowed.
- **Ages at introduction** accept "(40s)", "(2)", "(late twenties)" or ", 50,"; a
  possessive introduction ("JOHN WESTON's funeral") is let off. Like every check it
  can be switched off.
- **A parenthetical may wrap onto up to three lines** if it closes; the editor and the
  Rust parser read it the same way.

## 2026-11-03 · Comments: threads in a separate file

Comments work like Google Docs: a thread on a selected passage, with replies, edit
and delete for your own messages, and resolve (hidden, kept, reopened by a reply).

- **Kept in `comments.yaml`, not in the scene files.** The script stays clean, exports
  and other Fountain apps never see comments, and threads can hold replies, names,
  times and a resolved flag. We didn't use Fountain's `[[notes]]` in the text, which
  other apps keep but which clutter the script and can't hold threads.
- **Pinned by quote and context**: the quoted passage, 40 characters either side, and a
  line hint. When a scene opens each comment is located; while it's open the highlight
  follows every edit, and the new quote is stored when it moves. If the passage is
  edited outside the app, a close match is found; if it's deleted, the comment stays,
  marked detached. We didn't store character offsets, which break with any edit made
  elsewhere.
- **Co-writing shares `comments.yaml` like the other project files**, so it needed no
  change to the shared document's format. Ids and times are made by the writer's app,
  so every machine writes the same file.
- **The author is the co-writing name**, or a name asked for once and remembered.
- Suggested edits (Docs' "suggesting" mode) were left out for now (see ideas.md).

## 2026-11-03 · Format rules from the BBC Writersroom guides

The script check follows the BBC format guides (screenplay, TV screenplay, taped
drama, taped sitcom, stage; by Matt Carless, 2004). We treat them as conventions,
not as BBC requirements, because they're old.

- **Action is limited per paragraph, not in total.** Each paragraph is one beat of
  four or five printed lines, so the limit is 5. An earlier version used 4.
- **Cues use one name.** The first or last name, with titles abbreviated (MR, DR,
  British style without a full stop), and one cue per character throughout. The full
  name appears in capitals where the character is introduced. Suggested short cues
  avoid first names already used by another cue.
- **Spec or shooting script is a project setting** (`shooting_script`; spec is the
  default). Spec scripts get notes for camera directions and CUT TO:, and leave the
  draft off the title page. We kept camera-term suggestions in the editor either way,
  so the setting changes the checks, not the suggestions.
- **Novelistic action is a word-list heuristic** ("remembers", "realises" …). It's
  easy to switch off. We left out risky words such as "used to" and "feels", which
  are often visible action.
- **Exports add the standard openings and closings** (FADE IN:, FADE OUT., THE END,
  END OF ACT …, END OF SHOW), but never double ones the writer has typed.
- **Stage plays keep "(off)" under the name.** The (V.O.)/(O.S.) rule is for screen
  formats only.

## 2026-11-02 · Characters: introductions and short names

In film and single-camera TV, a character's first mention in the action is their full
name in capitals. After that they're written in ordinary case, using their **short
name** (`short:` in characters.yaml). The check recognises the cue, the full name and
the short name as the same person. Multi-camera scripts are left out, because their
action is printed in capitals anyway.

## 2026-11-02 · (CONT'D) is added, not typed

When a character speaks again after only action, the editor shows (CONT'D) as a faint
label and exports print it. It isn't written into the text, so it stays correct as you
edit. It's on by default for film and TV and off for stage (`auto_contd`). A typed
(CONT'D) or (CONT.) is noted as redundant when the automatic one is on.

## 2026-11-02 · A rule-based script check first, AI later

We started with rules that run as you type and work offline, with a one-click fix
only where the fix is certain. AI features wait until we've seen how useful the rules
are. Every check is a suggestion and can be switched off per project
(`checks: {off: […]}`); nothing is enforced.

## 2026-11-02 · Cast and places are lists you edit in place

characters.yaml and locations.yaml stay the source, edited from the Cast view.
Renaming a character changes cues, songs and mentions in the action, but leaves
dialogue alone, as it was spoken. In film and TV, places are taken from scene
headings.

## 2026-11-01 · Public repo, Conversion kept out

The repo is public (lgroges/stagewright). Conversion (co-written with Marco
Biasioli) was removed and the history rewritten. The full history is in the private
lgroges/stagewright-archive. Fixtures are original samples credited to
"Stagewright sample".

## 2026-11-01 · Versions and the updater

- One version number, in `[workspace.package]` in Cargo.toml.
  `node scripts/release.mjs <version>` updates every copy and dates the changelog;
  CI checks they agree.
- The app updates itself from GitHub Releases (`latest.json`, signed). The release
  notes come from CHANGELOG.md.
- Releases start from **Actions → release → Run workflow**, because pushing tags
  from Claude's environment is blocked. The signing key is a repository secret and is
  never committed.

## Earlier · Foundations

- A playwriting-first, contemporary yWriter: scenes as files, extended Fountain,
  CodeMirror 6, Tauri and Rust with Svelte. A personal tool first.
- Running time defaults to 150 spoken words a minute, with a per-scene override. Film
  and TV use page count instead (a page is about a minute; about 30 seconds for
  multi-camera).
- No Google Docs import: existing scripts are converted by hand with Claude's help.
- Live co-writing uses CRDTs over a relay. Where to host the relay is undecided (see
  [ideas.md](ideas.md)).
