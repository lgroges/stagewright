# Decisions

Choices made about Stagewright and why, newest first. Each entry: what we chose,
the reason, and what we didn't do. Ideas still open or turned down are in
[ideas.md](ideas.md).

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
