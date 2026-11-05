# Stagewright project format

The reference for reading a Stagewright project outside the app: what each file
holds and how to put the script together in order. Keep it current: any change to
the files the app reads or writes updates this page (and its version) in the same
commit.

**Format version 5** · last changed in Stagewright 0.9.0 (unreleased)

## A project is a folder

```
my-play/
  project.yaml          title, authors, format, settings
  characters.yaml       the cast list
  locations.yaml        places
  songs.yaml            musical numbers (musicals)
  comments.yaml         comment threads on the script (may be absent)
  acts/00/000-prologue.scene      a stage play or film: acts/<NN>/
  acts/01/010-the-quay.scene
  episodes/01/acts/01/010-cold-open.scene   a TV series: episodes/<EE>/acts/<NN>/
  .trash/               deleted scenes and acts (ignore when reading the script)
  .stagewright/         this writer's own state: co-writing, AI reviews (ignore)
```

All text is UTF-8 with LF line endings.

**Running order.** Episodes by folder number, then acts by folder number (act 0 is
the prologue, teaser or cold open), then scenes by file name. The file names start
with an order number (`010-`, `020-` …) and the slug of the scene's title. Each
scene's frontmatter also carries its `act`, `scene` and `episode` numbers, but the
folders and file names are what decide the order.

## project.yaml

| Key | Meaning |
| --- | --- |
| `title`, `subtitle` | The title page. A title like `dee-kelley-tv` is the folder name, not yet a real title. |
| `authors` | A list of names. |
| `contact` | Title page contact details, one line each (address, phone, email). |
| `format` | `stage` (default), `film`, `tv-single` (single-camera drama), `tv-multi` (multi-camera sitcom). |
| `series`, `episodes` | `series: true` for TV series; `episodes: [{number, title}]`. |
| `acts` | Optional act titles: `[{number, title}]`. Untitled acts print as ACT ONE, ACT TWO …; act 0 as PROLOGUE (stage), OPENING (film) or TEASER (TV). |
| `draft` | e.g. "Draft 3 (May 2026)". |
| `rating_fields` | Names of the four 1–9 scene ratings (default tension, humour, relevance, quality). |
| `running_time` | `words_per_minute` (150), `seconds_per_lyric_line` (6), `minutes_per_page` (screen formats; 1, or 0.5 for multi-camera). |
| `auto_contd` | (CONT'D) added in exports when a character speaks again after only action. Default: on for film and TV, off for stage. |
| `shooting_script` | `true`: a shooting script (camera directions allowed, scene numbers printed). Default: a spec script. |
| `layout` | PDF layout: `uk-stage` (stage plays) or `bbc-studio` (TV); absent means standard. |
| `dialect` | The English the grammar check expects: `british` (default), `american`, `canadian`, `australian`, `indian`. |
| `checks` | `{off: [codes]}`: script-check rules switched off (`ai-grammar` switches off the AI review). |

## characters.yaml

A list, one character per entry:

```yaml
- {id: joanna-weston, cue: JOANNA, name: Joanna Weston, short: Joanna, description: "…", track: actor-2}
```

- `cue`: the name as it appears above speeches (capitals).
- `name`: the full name, written in capitals where the character is first introduced in
  the action (screen formats).
- `short`: what the action calls them after that ("Joanna").
- `track`: doubling (stage): roles one actor plays share a track; `any-free` means
  anyone not on stage.

## locations.yaml and songs.yaml

```yaml
- {id: quay, name: "The quay at Saltmarsh"}
- {id: lanterns, title: "Lanterns", sung_in: [sc-2-1], heard_in: [sc-2-4], singers: [HETTIE], note: "…"}
```

A scene's `location` refers to a location `id`. In film and TV, places also come
from the scene headings (`INT. KITCHEN - NIGHT`).

## Scene files (`.scene`)

YAML frontmatter between `---` lines, then the script text:

```
---
id: sc-2-1
title: "Hettie's cottage"
act: 2
scene: 1
status: draft
location: hetties-cottage
duration: null        # manual minutes; overrides the estimate
onstage_silent: []    # on stage but with no lines
synopsis: "…"
notes: "…"
tags: [comic]
ratings: {tension: 6, humour: 8}
---
```

`id` never changes (comments, songs and co-writing refer to it). `status` is one of
`outline`, `draft`, `first-edit`, `second-edit`, `done`. Frontmatter may contain YAML
comments and other keys; ignore keys you don't know.

### The script text

Fountain-style, line by line. A blank line ends a speech.

| Line | Means |
| --- | --- |
| a capitals line after a blank, with text on the next line | character cue. `A & B` speak together; an extension in brackets: `(V.O.)`, `(O.S.)`, `(CONT'D)`. `@Name` forces a cue. `NAME ^` is dual dialogue beside the speech before. |
| `(…)` inside a speech | parenthetical. It may wrap onto two or three lines if it closes, e.g. `(in Russian;` then `English subtitles)`. |
| other lines inside a speech | dialogue |
| text outside a speech | stage direction (stage) or action (film/TV). `!` at the start forces it. |
| `INT. PLACE - TIME` after a blank | scene heading (`EXT.`, `INT./EXT.`, `I/E`, `EST`; `.TEXT` forces one; `#12A#` at the end is the scene number). |
| `~ SONG "Title" [SINGERS]` … `~ END SONG` | a musical number; `~ text` is a lyric, `~` alone a stanza break. |
| `> TEXT` | transition (`CUT TO:` on its own line between blanks also counts). |
| `>TEXT<` | centred text |
| `===` | page break |
| `*word*` | emphasis (not printed) |

Screen conventions the app expects: a character's first mention in the action is
their full name in capitals, often with an age, e.g. `JOANNA WESTON (30s)`, and after
that their short name in ordinary case. (CONT'D) is usually not typed (the app adds it).

## comments.yaml

Comment threads, Google Docs style, never stored in the scene files:

```yaml
- id: cm-wu7blq6i
  scene: sc-0-0                # the scene's id
  quote: Charts on every wall. # the passage commented on
  before: '…40 characters before it'
  after: '…40 characters after it'
  line: 0                      # body line (from 0) it started on, a hint
  resolved: false
  messages:
  - {author: Liam, at: 2026-11-03T09:05:23Z, text: "Which charts? Name one."}
  - {author: Marco, at: …, text: "…", edited: …}
```

To find a comment's place, search the scene's text for `quote`, using `before` and
`after` to choose between repeats. If the quote isn't there, the text was deleted or
rewritten and the comment is detached.

## Derived figures

- **Stage running time** of a scene: spoken words ÷ `words_per_minute` + lyric lines ×
  `seconds_per_lyric_line`. Spoken words exclude cues, parentheticals and directions.
- **Screen running time**: printed pages × `minutes_per_page` (about a page a minute;
  half for multi-camera). A film or TV page holds about 54 lines in the standard layout.
- A scene's `duration`, when set, overrides either estimate.
- The CLI gives exact figures and the script check: `stagewright check <folder>`
  (running time, cast by scene, doubling, script check), and
  `stagewright export <folder> out.pdf|out.fountain`.

## Changes to this format

- **4** (0.7.0): `dialect`.
- **3** (0.6.0): `layout`; parentheticals may wrap onto several lines.
- **2** (0.5.0): `comments.yaml`; `contact`, `shooting_script`; characters' `short`.
- **1** (0.4.0 and earlier): the folder layout, project.yaml, the registries and `.scene` files.
