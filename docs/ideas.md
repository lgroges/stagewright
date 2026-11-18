# Ideas

Things we've thought about doing. Move an entry to [decisions.md](decisions.md) when
it's settled, either built or turned down for good.

## Open

- **Sides**: a script for each actor, with only the scenes they're in and their lines
  marked, printed from Export. On the original plan (README, phase 6) but never
  recorded here.
- **More from the AI review**, once the grammar review has been tried (from the plan of
  2026-11-05): thoughts the camera can't see beyond the word list ("her heart sinks");
  weeding out the rule checks' false alarms (marking, not deleting, them); running
  automatically when you pause instead of on a button. Drama and history notes need a
  stronger model than runs locally; left for later.
- **A period setting for the AI review** (when and where the script is set), if the
  model flags deliberate period wording. For now it's told the title and to judge from
  the scene. The context web (plans/context-and-ai.md) may cover it: locations get a
  `period`, and research notes can say when the story is set.
- **An AI trained on editorial material**, for better editorial advice: craft books,
  script reports, good scripts, Liam's own notes on what he likes. Raised by Liam on
  2026-11-18. Claude can't be fine-tuned by us, so in practice this means a reference
  library the editorial read draws on (the relevant passages put in its context,
  cached), not training. Copyright: summaries and principles in our own words, not
  copied text, unless the material is Liam's or licensed.
- **From script to shooting script**: a way to take a script as written to a shooting
  script, which storyboards and drafted scenes would need first (Liam, 2026-11-18).
  Today `shooting_script: true` only changes the checks and prints scene numbers. A
  real transition would mean locking scene numbers (new scenes become 12A, omitted ones
  stay as OMITTED), a shot list per scene (shot, size, angle, movement, who and what's
  in it), revisions marked by colour and date, and an export of the shot lists.
- **AI storyboards and scenes**: generated storyboard frames for a scene, and drafted
  scenes (from a beat or a synopsis, say). Raised by Liam on 2026-11-18. Needs the
  shooting script above first: storyboards are drawn from a shot list. Storyboards
  need an image model, which Claude isn't (a local one through ComfyUI, or a hosted
  image API). Drafting scenes is a change of stance: so far the AI comments on the
  writing and doesn't write it (since the first plan, 2026-11-01).
- **Hosting the co-writing relay.** It runs locally for now; where it should live
  for real is undecided.
- **Suggested edits**, like Google Docs' suggesting mode: proposed insertions and
  deletions shown in the text, to accept or reject. Put off on 2026-11-03 in favour of
  comment threads.
- **Printing comments** in a PDF export (in the margin or as endnotes), for a read-through.
- **Cache the loaded project in the API.** Every command reads and parses every scene
  (about 5 ms for a 144-scene series in a release build, more for the script check's
  context). A cache keyed on file times would make big projects snappier. Found in
  the clean-up sweep, 2026-11-03.
- **Split `crates/stagewright-api/src/ops.rs` (about 1,100 lines) and the app's
  `state.svelte.ts` (about 800)** into modules by area: scenes, acts and episodes,
  project files, co-writing, export; and comments, cast, co-writing in the app. The
  AI review was split out on 2026-11-18 (`ai/`, `lib/ai.svelte.ts`); the rest is open.
- **List Playwright as a dev dependency of the app** so the end-to-end suites declare
  what they need (CI installs it separately today).

## Turned down

- **Importing from Google Docs.** Scripts are converted by hand with Claude instead.
- **Typing (CONT'D) into the text automatically.** It's shown as a label and printed
  in exports instead (see decisions.md).
