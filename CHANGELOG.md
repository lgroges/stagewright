# Changelog

What changed in each version of Stagewright. The app shows the notes for a new
version when it offers the update. Add to **Unreleased** as you go;
`node scripts/release.mjs <version>` dates it. (0.2.2 was never released: its one
change, the cursor, is listed under 0.3.0.)

## Unreleased

## 0.4.0 (2026-11-02)

- **Script check.** As you write, likely mistakes are underlined (wavy) and
  conventions noted (dotted); point at one to see why, and fix it in one click where
  the fix is certain. The status bar counts them (click to list them; F8 goes to the
  next). Production lists every issue in the play, and clicking one shows its line.
  - Warnings: a cue with a blank line after it (so the speech reads as action), a cue
    not in capitals, a cue or singer that isn't in the cast list ("did you mean…?"), a
    song never closed, ~ END SONG with no song, a parenthetical missing its ")", and
    dual dialogue (^) with nothing beside it.
  - Notes: (CONT'D) after a different speaker, someone "on stage but silent" who
    speaks, lyrics outside a song; in film and TV, headings with no time of day or a
    misspelt one (NITE), INT without a full stop, scenes not starting with a heading,
    and a character's first appearance in the action not in capitals.
  - Any check can be switched off for a project ("Don't check for this", or Checks…
    in Production); the setting is kept in project.yaml.

## 0.3.0 (2026-11-02)

- **Cast** view (next to Production): every character and place, where each appears,
  and the lists in characters.yaml and locations.yaml, edited in place: name,
  description, the actor (track) for doubling.
  - Names in the script but not in the list are flagged; add them in one click, or
    rename a typo onto the right name.
  - Rename a character everywhere: cues, songs and mentions in the action, the lists
    too (dialogue is left as spoken). A preview says what will change first.
  - Film and TV: places are found in scene headings; renaming one changes every
    heading that uses it.
- Character names are suggested in action and stage directions: type the first
  letters of a name in capitals.
- Film and TV: camera directions (CLOSE ON, ANGLE ON, POV, PUSH IN, SLOW MOTION …)
  are suggested in action the same way, and at the start of a line transitions too
  (CUT TO:, DISSOLVE TO:, FADE OUT. …).
- The text cursor is easier to see: white and thicker in dark mode (dark on the light
  theme), in the script and in project files.

## 0.2.1 (2026-11-02)

- Delete an act or a whole episode from the scene tree (the bin button that appears
  when you point at its heading). Its scenes go to the project's `.trash` folder, and
  later acts or episodes move up to close the gap. Works while co-writing too.
- Fixed: after switching to Production (or a project file) and back to Script, the
  editor could show the scene as it was when you opened it, often a blank page.
  Typing there could then overwrite the scene. The editor now stays as you left it.
- The update prompt now shows what's new in each version.

## 0.2.0 (2026-11-01)

- Film and television: feature film, single-camera and multi-camera TV formats, each
  with its own page layout, page counts (in eighths) and running time.
- TV series: one project holds episodes, each with its own acts and scenes, sharing
  characters, locations and co-writing.
- Scene headings (Ctrl+8) with suggested places and times of day.
- Production view: pages, an episode filter and a scene breakdown.
- Export the script as a PDF in the format's standard layout, or as a Fountain file.
- Stagewright now updates itself: it checks for a new version when it starts.

## 0.1.0 (2026-10-31)

- First version: scene tree, script editor, details panel, autosave, Production view,
  and live co-writing.
