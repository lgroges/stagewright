---
name: stagewright-projects
description: Read Liam's Stagewright writing projects (plays, musicals, film and TV scripts) when he shares one, as a folder, zip, files or GitHub repo, for research, notes, analysis or edits.
---

# Reading Stagewright projects

Stagewright is Liam's scene-based writing app (github.com/lgroges/stagewright). A
project is a folder: `project.yaml`, `characters.yaml`, `locations.yaml`,
`songs.yaml`, `comments.yaml`, and one `.scene` file per scene under `acts/<NN>/` or,
for a TV series, `episodes/<EE>/acts/<NN>/`.

## 1. Get the current format first

The format changes as the app develops. Before reading a project, fetch the live
reference and follow it wherever it differs from the summary below:

- https://raw.githubusercontent.com/lgroges/stagewright/main/docs/format.md
  (or https://github.com/lgroges/stagewright/blob/main/docs/format.md)

It starts with a "Format version" line. This skill was written for **format version
3**. If the live version is higher, use the live page, and tell Liam in one line that
this skill can be refreshed (he can ask Claude in the Stagewright repo to propose the
update). If the page can't be reached, use the summary below and say it may be out
of date.

## 2. Get the files

- **Zip or files attached**: unpack the zip into its own empty folder and read from
  there. If only some files were attached, say which are missing if they matter (a
  scene without `characters.yaml` still reads fine).
- **A folder on Liam's computer** (when the session is linked to it): list it, then
  stage the project files. Skip `.trash/` and `.stagewright/`.
- **A GitHub repo**: clone it.

Never edit Liam's originals unless he asks; work on a copy and send back what changed.

## 3. Read the script in order

Order is episode folder, then act folder (00 is the prologue, teaser or cold open),
then scene file name (`010-…`, `020-…`). For each scene read the frontmatter (title,
status, synopsis, notes, location, on-stage-silent, ratings, tags) and then the text.
For a whole-script read, put it together in that order with scene labels such as
"E1 1.2 The van", and keep track of which scene each finding comes from.

Read `comments.yaml` too: each comment is pinned to a passage of one scene (by
`scene` id and `quote`). Unresolved comments are open questions Liam or a co-writer
raised; mention ones that bear on what he asked.

## 4. Summary of the format (version 3)

**project.yaml**: `title`, `subtitle`, `authors`, `contact`, `format` (`stage`,
`film`, `tv-single`, `tv-multi`), `series` and `episodes: [{number, title}]`, `acts:
[{number, title}]`, `draft`, `rating_fields`, `running_time` (`words_per_minute` 150,
`seconds_per_lyric_line` 6, `minutes_per_page`), `auto_contd`, `shooting_script`,
`layout` (`uk-stage`, `bbc-studio`), `checks: {off: [...]}`.

**characters.yaml**: `- {id, cue, name, short, description, track}`. `cue` is the name
above speeches; `name` the full name used at the character's introduction; `short`
what the action calls them afterwards; `track` groups doubled roles.

**locations.yaml**: `- {id, name}`. **songs.yaml**: `- {id, title, sung_in, heard_in,
singers, note}` (scene ids).

**Scene frontmatter**: `id` (stable), `title`, `act`, `scene`, `episode`, `status`
(`outline`, `draft`, `first-edit`, `second-edit`, `done`), `location`, `duration`
(manual minutes), `onstage_silent`, `synopsis`, `notes`, `tags`, `ratings`.

**Scene text** (Fountain-style; a blank line ends a speech):
- Capitals line after a blank with text below it: a character cue (`A & B` together;
  `(V.O.)`, `(O.S.)`, `(CONT'D)` extensions; `@Name` forced; `NAME ^` dual dialogue).
- `(…)` in a speech: parenthetical (may wrap over two or three lines). Other speech
  lines: dialogue.
- Anything else: stage direction or action (`!` forces it).
- `INT. PLACE - TIME` after a blank: scene heading (`#12#` at the end is its number).
- `~ SONG "Title" [SINGERS]` … `~ END SONG`; `~ lyric`; `~` stanza break.
- `> TEXT` transition; `>TEXT<` centred; `===` page break; `*word*` emphasis.

**comments.yaml**: `- {id, scene, quote, before, after, line, resolved, messages:
[{author, at, text, edited}]}`. Find the passage by searching the scene for `quote`,
using `before`/`after` to pick between repeats; if it's missing, the comment is
detached.

**Running time**: stage, spoken words ÷ `words_per_minute` + lyric lines ×
`seconds_per_lyric_line`; film and TV, about a minute a page (half for multi-camera).
A scene's `duration` overrides either. Give these as estimates.

## 5. Writing back

If Liam asks for changes to a project (a revised scene, a new character), keep to the
format: LF line endings, the frontmatter keys as they are (never change `id`, `act`
or `scene` numbers or rename files; the app does that when scenes move), cues in
capitals, one blank line between paragraphs, and comments only ever in
`comments.yaml`. Hand back the changed files with a short note of what changed, and
put them in his folder only when he asks.

Conversion (his St Augustine play, co-written with Marco Biasioli) is private: never
publish or share it.
