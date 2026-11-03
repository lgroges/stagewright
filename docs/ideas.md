# Ideas

Things we've thought about doing. Move an entry to [decisions.md](decisions.md) when
it's settled, either built or turned down for good.

## Open

- **Story day and time in studio headings** (DAY 1 [09.20]): the BBC studio layout
  prints whatever the heading says; it could offer them, or check they're in order.
- **AI features** beyond the rule-based check, once we've seen how useful the rules
  are.
- **Hosting the co-writing relay.** It runs locally for now; where it should live
  for real is undecided.
- **Suggested edits**, like Google Docs' suggesting mode: proposed insertions and
  deletions shown in the text, to accept or reject. Put off on 2026-11-03 in favour of
  comment threads.
- **Comments in Production**: a project-wide list of open comments, like the script
  check's. At present the scene list counts them per scene.
- **Printing comments** in a PDF export (in the margin or as endnotes), for a read-through.

- **Cache the loaded project in the API.** Every command reads and parses every scene
  (about 5 ms for a 144-scene series in a release build, more for the script check's
  context). A cache keyed on file times would make big projects snappier. Found in
  the clean-up sweep, 2026-11-03.
- **Split `crates/stagewright-api/src/ops.rs` (about 1,100 lines) and the app's
  `state.svelte.ts` (about 800)** into modules by area: scenes, acts and episodes,
  project files, co-writing, export; and comments, cast, co-writing in the app.
- **List Playwright as a dev dependency of the app** so the end-to-end suites declare
  what they need (CI installs it separately today).

## Turned down

- **Importing from Google Docs.** Scripts are converted by hand with Claude instead.
- **Typing (CONT'D) into the text automatically.** It's shown as a label and printed
  in exports instead (see decisions.md).
