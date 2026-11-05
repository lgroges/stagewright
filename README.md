# Stagewright

A playwriting-first, scene-based writing tool: yWriter's scene model rebuilt for theatre,
and for film and television (feature, single-camera and multi-camera TV, with series of
episodes). Personal tool first. Design: the "Playwriting Tool — Design Spec v0" doc.

## Status

| Phase | State |
| --- | --- |
| 1. Format + parser | Done: byte-exact round trip on every scene |
| 2. Google Docs conversion | Done (a one-off script, kept outside this repo) |
| 3. Writing app (Tauri + CodeMirror 6) | Done: scene tree, script editor, details panel, autosave |
| 4. Live co-writing (Yjs) | **Done**: share/join, live text + structure + details, cursors, offline merge. Relay hosting to choose |
| 5. Derived views | Production view: running time, pages, scene breakdown, cast by scene, doubling |
| 6. Export | PDF (industry layout per format) and Fountain. Sides still to do |
| 7. Film and TV | **Done**: four formats, series projects, screenplay editing, page counts |

## Using it

**Desktop app.** Download the latest version from
[Releases](https://github.com/lgroges/stagewright/releases/latest): on Windows the
`-setup.exe`, on a Mac the `.dmg` (one app for Apple silicon and Intel). The Mac build is
not signed with an Apple certificate yet: the first time, open it, then allow it under
*System Settings → Privacy & Security → Open Anyway*.
Then *Open project folder…* and pick a project folder (the one containing `project.yaml`),
or *New project…* to start one.
You can also open one directly: `Stagewright.exe C:\path\to\play`.

**Updates.** Stagewright checks for a new version when it starts (and every few hours
while open). When there is one it offers it, with what's new; *Install and restart*
saves your work, downloads the installer, checks its signature and restarts into the new
version. *Check for updates* is on the start screen, next to the version number.

**In a browser** (any OS, no install):

```
cd app && npm install && npm run build
cargo run -p stagewright -- serve path/to/play --static dist
# open http://127.0.0.1:4870/
```

### Formats

Each project has a `format` in `project.yaml`, chosen when you create it:

| Format | Page | Running time | Layout |
| --- | --- | --- | --- |
| `stage` (default) | A4 | spoken words ÷ 150 a minute | stage directions indented, acts and scene titles |
| `film` | US Letter | about a minute a page | screenplay: scene headings, action, dialogue column |
| `tv-single` | US Letter | about a minute a page | as film, plus teaser, ACT ONE… and END OF ACT on new pages |
| `tv-multi` | US Letter | about 30 seconds a page | sitcom: double-spaced dialogue, upper-case action, underlined headings, lettered scenes with the cast listed |

Every format uses Courier 12 at 10 characters an inch and 6 lines an inch, and page
counts follow the printed layout ((MORE)/(CONT'D) splits, headings never at the foot of
a page). Screen formats count pages in eighths, as a breakdown does. A scene's manual
`duration` always overrides the estimate.

A TV project can be a **series**: episodes in one project, sharing characters, locations
and co-writing. Files live in `episodes/<EE>/acts/<NN>/`; *+ Add episode* and *+ act*
are in the scene tree, and episode titles are in `project.yaml` (`episodes:`). Act 0 is
the teaser or cold open.

**Export** (header) writes a PDF of the whole script with a title page, or a
`.fountain` file for other screenwriting apps. In a series, choose one episode or the
whole series. From the command line:
`stagewright export path/to/project script.pdf [--episode 2]`.

The Export menu also chooses a **layout** for the PDF (`layout:` in project.yaml):
- **UK stage** (stage plays, as the BBC stage guide sets them): the name at the left
  margin with a colon and the speech beside it, parentheticals in capitals inside the
  speech, directions in capitals, ACT I SCENE 1 at the top of every scene's page, and
  CURTAIN at the end of each act. A character carrying on after a direction isn't
  named again.
- **BBC studio** (TV, as taped drama and sitcom): a wide left margin, scenes numbered
  (SC1. for drama, SCENE 1. for sitcom) on new pages, NAME: underlined over the speech,
  action in capitals (in square brackets for drama), CUT TO: after every scene unless
  you've written a transition, and END OF EPISODE. Headings carry the story day and
  time, `INT. HALL - DAY 2 [09.20]`: the editor offers the day as you type the
  heading, and the check notes a missing day, days or times out of order (flashbacks
  excepted), a time written oddly, and DAY or NIGHT against the clock (day begins at
  06.00, night at 20.00). Days start again each episode.

A **shooting script** (film and TV) prints scene numbers in both margins beside every
heading, counting through each episode (`#12#` on a heading sets its number), and
Fountain exports carry them too.

### Writing a scene without typing any markup

| Key | What it does |
| --- | --- |
| `Tab` on an empty line | Start a speech: type the character (names are suggested), `Enter` or `Tab` to accept |
| `Enter` | Moves to what usually comes next: character → dialogue → new paragraph |
| `Tab` on dialogue | Make it a parenthetical (and back) |
| `Shift+Tab` | Step an element back (parenthetical → dialogue, cue → direction) |
| `Shift+Enter` | Plain new line |
| `Ctrl+1` … `Ctrl+7` | Direction (Action), Character, Parenthetical, Dialogue, Lyric, Song, Transition (also on the toolbar) |
| `Ctrl+8` | Scene heading (`INT. `): places and times of day are suggested; `Enter` then moves to action |
| `Enter` on an empty lyric line | Ends the song |
| `Ctrl+Shift+L` | Stanza break |
| `Ctrl+S` | Save now (it also saves as you type) |

Scenes save themselves about a second after you stop typing. If a file is changed
outside the app (another editor, a sync tool), Stagewright stops and asks whether to
keep your version or load the other one; nothing is overwritten silently.

**Cast** lists every character and place: where each appears, and the lists in
`characters.yaml` and `locations.yaml`, edited in place (name, description, and for
characters the actor's track, used by the doubling check). Names used in the script but
missing from the list are flagged, with one click to add them. **Rename…** changes a
character everywhere (cues, song singers, mentions in the action, the lists; dialogue
is left as spoken) or a place in every scene heading, after a preview of what will
change; renaming onto a name already in the script joins the two (for typos). In action
and stage directions, typing the first letters of a name in capitals suggests it; in film
and TV, camera directions (CLOSE ON, POV, PUSH IN …) and, at the start of a line,
transitions (CUT TO:, FADE OUT. …) are suggested the same way.

**(CONT'D)** is added automatically when a character speaks again after only action in
the same scene: shown faintly in the editor (not typed into the text) and printed in
exports. It's on for film and TV and off for stage plays; switch it under **Export**
(`auto_contd` in project.yaml).

The scene list can be hidden (top-left button, or Ctrl+\) and its episodes and acts
folded (▾); both are remembered.

**Script check.** As you write, likely mistakes are underlined with a wavy line (a cue
with a blank line after it, so the speech reads as action; a cue not in capitals or not
in the cast list, with "did you mean…?"; a song never closed; a parenthetical missing its
bracket; dual dialogue with nothing beside it) and conventions with a dotted line
((CONT'D) after a different speaker; in film and TV, a heading with no or a misspelt time
of day, INT without a full stop, a character's first appearance not in capitals). Point
at one for the reason and a one-click fix where the fix is certain; the status bar counts
them (click to list, F8 for the next). **Production** lists every issue in the play.
In film and TV, characters are introduced once with their full name in capitals and
then written with their **short name** (set in Cast, e.g. `short: Joanna` in
characters.yaml): capitals after the introduction are noted with the short name as the
fix, and typing the first letters of a short name in the action suggests it. Headings
need a blank line after them and no description in them, and lead into action; action
paragraphs are kept to 5 printed lines; (V.O.) and (O.S.) go next to the name;
parentheticals are lower case; FADE OUT. only ends a film or an act; action shouldn't
describe thoughts ("remembers", "realises"); and a character's introduction gives an age,
like (30s); headings name the place ("A CHURCHYARD" doesn't); and scenes over 3 pages
(6 for multi-camera) are noted. In any format, action and stage directions get a
**grammar check** (Harper, which runs locally; plus "was sat", "him self" and "next the",
which it misses), in British English unless project.yaml says `dialect: american` (or
`canadian`, `australian`, `indian`); dialogue is left alone. The same word twice in a row ("are are") is noted, in action and
directions (dialogue is left alone, where repeats are often meant). A parenthetical can be
wrapped onto two or three lines as long as it closes. Cues should use one name, with titles
abbreviated, and one cue per character (listed under Cast). A **spec script** (the
default for film and TV; switch to **Shooting script** under Export) also notes camera
directions and CUT TO:, and leaves the draft off the title page. The title page needs
contact details: `contact:` in project.yaml, printed bottom left. The editor also checks spelling with the system dictionary (the
**Spelling** button turns it off). Checks can be switched off per project
(`checks: {off: [...]}` in project.yaml).
`stagewright check <dir>` prints the same list.

**AI review.** **Review scene** (under the script) asks a model running on your computer,
through [Ollama](https://ollama.com), for grammar mistakes, typos and wrong words the
checks miss ("turns away upon to the sight"), in dialogue as well as action. It's told
to leave period language, the way characters speak and script fragments alone. Its notes
are marked **AI** (dashed violet) alongside the check's: each is the change it suggests
and its reason ("“carries” → “carry”? The subject ‘Four men’ is plural."), with a
one-click change, and they follow the text as you edit. It takes a couple of minutes a
scene on a laptop; you can keep writing while it runs; reviewing unchanged text again is instant (reviews are kept in `.stagewright/ai/`,
never shared). It uses Qwen 3.6 if installed (`ollama pull qwen3.6`), else Qwen 3 30B, or the model
chosen under Production → Checks…, where the Ollama address can be changed too. If Ollama isn't
running, Review says so and nothing else changes. The script isn't sent anywhere else.
Switch it off for a project like any check (`ai-grammar`).

**Comments.** Select some text and press **Comment** (Ctrl+Alt+M) to comment on it, as
in Google Docs. The passage is highlighted, and the comments panel (💬 above the script)
lists the scene's threads in script order: reply, edit or delete your own messages, and
resolve a thread to hide it (Show resolved brings it back; replying reopens it). The
scene list counts each scene's open comments, and **Production** lists every comment in
the project; clicking one opens its passage. Comments live in `comments.yaml`, never in
the scene files, so exports and other apps don't see them. Each is pinned to its quoted
text with a little context either side, so it follows the text as you edit; if the text
is deleted, the comment stays, marked as detached. When co-writing, comments are shared
live, under each writer's name.

The **Details** panel edits title (renames the file), status, a manual running time
(overrides the estimate), location, synopsis, who is on stage but silent, ratings,
tags and notes. **Production** shows running time (and pages) per scene and act, the
cast-by-scene table, doubling problems for stage, and for film and TV a scene breakdown:
every heading's INT/EXT, location, time of day, pages and cast. In a series it shows
one episode or all of them. **Project files** edits `project.yaml`, `characters.yaml`
(set each role's `track` for doubling), `locations.yaml`, `songs.yaml` and `comments.yaml`; invalid YAML
is refused with the reason.

Deleted scenes go to the project's `.trash/` folder, not the bin.

## Co-writing

**Share** (header, top right) puts the open project in a private room on a relay and gives
you an invite code (`sw1.…`). Anyone with the code can join and edit, so send it
privately. A co-writer chooses **Join a shared project…** on the start screen, pastes the
code and picks an empty folder: the whole project is copied there.

While you are both in the app, text, scene order, titles and details sync live, and you
see each other's cursors and which scene each writer is in. Each app keeps writing
ordinary `.scene` files to its own folder, so git, backups and the CLI keep working.
Offline edits are kept and merged on reconnect; edits made to the files in another
program are merged in as well (three-way, against what the app last wrote).
**Stop co-writing** (in the Live menu) turns it off on that computer; the files stay.

Sharing state lives in `<project>/.stagewright/` (git-ignored automatically).

### The relay

The relay passes edits between writers and keeps each room's shared copy, so writers
don't need to be online at the same time. It is built into the CLI:

```
stagewright relay --host 0.0.0.0 --port 4444 --data /var/lib/stagewright-relay
```

It speaks the standard y-websocket protocol. For use over the internet put it behind
TLS (e.g. Caddy: `relay.example.com { reverse_proxy 127.0.0.1:4444 }`) and share with
`wss://relay.example.com`. For a trial on one machine: `npm run relay` (in `app/`)
and `ws://127.0.0.1:4444`. Where to host it for real is still to be decided.

## Layout

```
crates/stagewright-core   .scene parser/serialiser, project loader, analysis, frontmatter edits,
                          page layout per format (layout.rs), PDF writer, PDF/Fountain export
crates/stagewright-api    the JSON commands behind the app (shared by desktop and browser)
crates/stagewright-cli    `stagewright` binary: check, serve, classify, relay, export
app/src/lib/collab        shared document model, reconciler, co-writing session
app/                      Svelte + CodeMirror 6 frontend
app/src-tauri             Tauri desktop shell (one `api` command → stagewright-api)
fixtures/stage-sample     Harbour Lights, a short play with songs written for the tests
fixtures/film-sample      a short film; tv-series-sample (single-camera, 2 episodes);
fixtures/sitcom-sample    and a multi-camera series
```

The desktop app and the browser server call the same `stagewright-api` dispatcher, so
they cannot drift apart. The editor's line classifier (`app/src/lib/classify.ts`) mirrors
the Rust parser; CI regenerates `classify.json` from Rust and fails if the two disagree.

## Developing

Decisions and the reasons for them are in [docs/decisions.md](docs/decisions.md);
ideas still open or turned down are in [docs/ideas.md](docs/ideas.md). The project
files are described for readers outside the app in [docs/format.md](docs/format.md).

```
cargo test                          # Rust: parser, API operations, analysis
cargo run -p stagewright -- check fixtures/stage-sample
cd app && npm install
npm test                            # editor commands + classifier parity
npm run check                       # types
npm run tauri dev                   # desktop app with hot reload
npm run build && npm run e2e        # browser end-to-end test (needs Playwright + Chromium)
npm run e2e:cowrite                 # two writers, two folders, one relay
npm run e2e:screen                  # new TV series, screenplay drafting, export
npm run e2e:update                  # the update offer, install and restart (Tauri faked)
npm run e2e:cast                    # Cast view, renames, names suggested in action
npm run e2e:check                   # script check: underlines, fixes, settings, Production list
```

### Releasing a version

The version number lives in `Cargo.toml` (`[workspace.package]`) and is copied to
`app/package.json` and the lockfiles; CI fails if they disagree. Note changes under
**Unreleased** in `CHANGELOG.md` as you go, then:

```
node scripts/release.mjs 0.3.0      # bump everywhere, date the changelog, commit, tag v0.3.0
git push origin main v0.3.0         # starts the release workflow
```

Or push `main` and start it from GitHub: *Actions → release → Run workflow*. It
releases the version in `Cargo.toml` and makes the tag itself.

`release.yml` builds signed installers for Windows and macOS, attaches them to a GitHub
Release with the changelog notes, and publishes `latest.json`, which installed apps check.
The release stays a draft until both platforms have built. Signing needs the
`TAURI_SIGNING_PRIVATE_KEY` repository secret (the updater's private key; the public half
is in `tauri.conf.json`). Keep a copy of the key somewhere safe: without it, installed
apps can't accept further updates and would need reinstalling by hand.

Every push also builds test installers in the `ci` workflow (under *Actions*), without
update signing.

Building the desktop app on Windows needs Rust (MSVC toolchain), Node 22 and the
WebView2 runtime (already on Windows 10/11). `npm run tauri build` writes installers to
`target/release/bundle/`.

## The .scene format

YAML frontmatter, then a Fountain-style body:

```
---
id: sc-2-1
title: "Hettie's cottage"
location: hetties-cottage
duration: null        # manual minutes; overrides the estimate
onstage_silent: []    # walk-ons with no lines
---
HETTIE’s cottage. A fire, a kettle, a fiddle on the wall.

HETTIE
(singing to herself as she works)
~ SONG "Lanterns" [HETTIE]
~ Light one for the boat that’s late,
~
~ Lanterns on the water,
~ END SONG

CORNELIUS (CONT'D)
Dialogue with *emphasis*.

> BLACKOUT.
```

| Line | Means |
| --- | --- |
| upper-case line after a blank, followed by text | character cue (`A & B` for shared lines, `(CONT'D)` extension) |
| `(…)` inside a speech | parenthetical |
| other text inside a speech | dialogue |
| text outside a speech | stage direction (`!` forces one) |
| `~ SONG "Title" [SINGERS]` … `~ END SONG` | a musical number; `~ text` lyric, `~` stanza break |
| `> TEXT` | transition; `CUT TO:` on its own (blank lines around) also counts |
| `@Name` | forced cue; `^` after a cue makes dual (side-by-side) dialogue with the previous speech |
| `INT. PLACE - TIME` after a blank | scene heading (`EXT.`, `INT./EXT.`, `I/E`, `EST`; `.TEXT` forces one; `#12A#` scene number) |
| `>TEXT<` | centred text |
| `===` | page break |

Stage running time = spoken words ÷ `words_per_minute` + lyric lines ×
`seconds_per_lyric_line` (both in `project.yaml`); screen formats use pages ×
`minutes_per_page` (1, or 0.5 for multi-camera, under `running_time`). A scene's
`duration` overrides either. Files are named `<order>-<slug>.scene` inside `acts/<NN>/`
(or `episodes/<EE>/acts/<NN>/`); the app keeps names and the `act`/`scene`
numbers in step when you reorder. Scene `id`s never change.
