# Plan: project context and the AI editor

Agreed with Liam on 2026-11-18. A plan, not a record of what's built: as each part is
built, its decisions move to [decisions.md](../decisions.md) and this file is updated.

## The idea

Stagewright holds the script. This plan adds what a writer keeps around it — character
notes, a beat sheet, research — as linked records in the project, a *context web*.
It's useful on its own (the notes for the open scene beside the script, beats measured
against where they land) and it's what lets the AI act as a well-informed editor
rather than a proofreader.

Three releases:

| Release | What | Format |
| --- | --- | --- |
| 0.9.0 | The AI copy-edit, with Claude as well as Ollama | 6: a project opts in to sending text to Anthropic |
| 0.10.0 | Notes on characters and places, beats, research, links between them | 7: new files and folders |
| 0.11.0 | The editorial read: notes on a scene from the AI, informed by the context web | none |

## 1. Claude for the copy-edit (0.9.0)

What's built (Review scene with Ollama) gets a second provider.

- **Tested 2026-11-05** (`docs/decisions.md`): Sonnet 5.5 found 9 of 9 planted mistakes
  in two scenes, changed nothing in the clean versions, left period language and
  dialect alone, gave correct reasons, in about 5 seconds and about 1p a scene. High
  effort was no better than the default. Qwen 3.6 locally: 8 of 9, about 1½ minutes.
- **One request a scene**: the whole scene as numbered paragraphs; the answer lists
  only the paragraphs corrected, each with its changes (`from`, `to`, `why`). The
  word diff still places each change; the reason comes with it, so no second request.
  The prompt is the one tested.
- **Settings** (Production → Checks…, this computer's): provider (Ollama or Claude),
  model (Sonnet 5.5 by default), the API key. The key is kept in the system's
  credential store (Windows Credential Manager, macOS Keychain) via the `keyring`
  crate, never in the project, prefs or logs.
- **A project opts in to sending text to Anthropic**: `ai: {cloud: true}` in
  project.yaml, asked once per project ("Send this project's scenes to Anthropic for AI
  review?"), with a note for co-written projects that it's the co-writers' work too.
  Off by default; Ollama needs no opt-in.
- Calls go from Rust (`ureq` with TLS: `rustls`), outside the workspace lock, as now.
  Failures (no key, no credit, offline, rate-limited) are reasons, never errors.
- Cost shown after a review from the response's usage ("about 1p").

## 2. The context web (0.10.0)

### Records

Every record has an id, structured fields, an optional Markdown page, and labelled
links. Ids are unique across the project; links use ids.

| Record | Where | Fields | Page |
| --- | --- | --- | --- |
| Character | `characters.yaml` (as now) | id, cue, name, short, description, track, + `age`, `role` | `characters/<id>.md` |
| Location | `locations.yaml` (as now) | id, name, + `period`, `description` | `locations/<id>.md` |
| Beat | `plan/beats.yaml` | id, title, summary, purpose, target, scenes, episode | — (summary and purpose are enough to start) |
| Research note | `research/<id>.md` | title, tags, sources (text), links | the note itself |
| Scene | `.scene` files (as now) | synopsis, notes … | the script |

A page is Markdown with optional YAML frontmatter. Pages are optional: a character
with no page is just its line in characters.yaml.

### Links

- **Labelled links** live in a page's frontmatter:

  ```yaml
  links:
    - {to: dee, label: scryer to, back: employs}
    - {to: mortlake}
  ```

  `label` reads from this record to the other ("Kelley — scryer to — Dee"); `back` is
  the other way ("Dee — employs — Kelley"). Both optional. A link is stored once; the
  other record shows it as a backlink, with `back` as its label.
- **Links in the text**: `[[dee]]` or `[[dee|the Doctor]]` in any page (Obsidian's
  syntax). Unlabelled; shown as backlinks too.
- **Implied links** the app works out and never stores: a scene links to the
  characters who speak or are named in it, its location, and the beat(s) that list it.
- A link to an id that doesn't exist is shown as broken (and the script check notes it).

### Beats

- `plan/beats.yaml`:

  ```yaml
  template: save-the-cat
  beats:
    - {id: catalyst, title: Catalyst, summary: "Weston dies; Dee meets Kelley", purpose: "…",
       target: 0.11, scenes: [sc-mvy4wgtl]}
  ```

- **Templates** fill in a starting list, all editable, none enforced: Save the Cat
  (15 beats), three acts, Dan Harmon's Story Circle (8), Freytag's pyramid, blank.
- **Target** is a share of the running length (0–1): pages for screen, minutes for
  stage, the episode for TV beats (`episode:` set) or the series for series beats.
- **Where it lands**: the start of its first scene, as a share of the length, shown
  against the target ("Catalyst: page 19, aimed at 12"). No scenes yet: not shown.
- **Make scenes from beats**: a beat with no scenes can make an outline scene in the
  right act with its summary as the synopsis.

### In the app

- **A Notes view** (beside Script, Cast, Production): characters, locations, beats and
  research in a list; the page editor (the script's CodeMirror, Markdown mode);
  `[[` offers every record by name; labelled links edited as a list; backlinks below.
- **Beat board**: beats in order with target and actual position, linked scenes, and
  drag to reorder.
- **Context beside the script**: for the open scene, its beat, its characters and
  location (with their pages), and research linked to any of them. One hop, the same
  set the AI gets.
- Co-writing shares the new files like the others (to check: the shared document's
  file list).

### Files and the skill

A format change (7): docs/format.md and the `stagewright-projects` skill summary are
updated in the same commit, and the skill re-proposed. The skill will read pages and
research with the script, which helps research sessions with Claude too.

## 3. The editorial read (0.11.0)

- **What it looks for**, in a scene, with its context: continuity (names, ages, dates,
  who knows what by now); period detail against the project's research; whether the
  scene does what its beat says; a character's voice against their page; set-ups with
  no pay-off and pay-offs with no set-up; thoughts the camera can't see.
- **Notes, not fixes**: findings become comment threads on their passages, written by
  "AI review", to reply to or resolve. No one-click changes (those are the copy-edit's).
  Each note has a kind, so kinds can be switched off.
- **Context**: the scene; its beat; its characters' and location's pages; research
  linked to any of those; the scenes either side (or their synopses); the project's
  title, format and dialect. Stable parts first, so they're cached between scenes.
- **Cost** (Sonnet 5.5, $2 / $10 per million tokens): about 30,000 tokens a scene, so
  about 6–7p, about 2p when the context is cached (reviewing several scenes in a
  sitting). A whole-script continuity pass (about 150,000 tokens) about 25–30p a run,
  run per draft, not per scene. Opus 5.5 available for the editorial read if Sonnet's
  notes are thin.
- Claude only: tested local models weren't up to this.
- **Tests**: a project with planted continuity slips (an age that changes, a character
  who knows something before they're told), checked like the copy-edit was.

## Open questions

- Series beat sheets: one sheet per episode plus a series sheet, or one sheet with
  `episode:` on each beat (as above)?
- Should the editorial read run on a selection of scenes in one go (an act), with one
  cached context?
- Research notes from Claude sessions: a way to import a chat's findings as notes.
