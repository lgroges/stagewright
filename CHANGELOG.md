# Changelog

What changed in each version of Stagewright. The app shows the notes for a new
version when it offers the update. Add to **Unreleased** as you go;
`node scripts/release.mjs <version>` dates it. (0.2.2 was never released: its one
change, the cursor, is listed under 0.3.0.)

## Unreleased

- **Layouts for the PDF**, chosen under Export:
  - **UK stage**, for stage plays: the name at the left margin with a colon and the
    speech beside it, directions in capitals, ACT I SCENE 1 on every scene's page,
    CURTAIN at the end of each act.
  - **BBC studio**, for TV (taped drama and sitcom): numbered scenes on new pages,
    NAME: underlined over the speech, action in capitals (bracketed in drama), CUT TO:
    after every scene, END OF EPISODE.
- **Shooting scripts print scene numbers** in both margins beside every heading, and
  in Fountain exports.
- New checks: the same word twice in a row ("are are"), in action and directions; and
  in film and TV, a character introduced without an age, like (30s).
- A parenthetical wrapped onto two or three lines ("(in Russian;" / "English
  subtitles)") is read as one, not as an unclosed bracket.

## 0.5.0 (2026-11-03)

- **Comments**, as in Google Docs. Select text and press **Comment** (Ctrl+Alt+M):
  the passage is highlighted, and the comments panel (💬 above the script) lists the
  scene's threads. Reply, edit or delete your own messages, and resolve a thread to
  hide it (Show resolved brings it back). The scene list counts open comments.
  Comments are kept in `comments.yaml`, not in the scene files, and follow their text
  as it's edited; if it's deleted the comment stays, marked as detached. Shared live
  when co-writing.
- **More script checks, from the BBC Writersroom format guides.**
  - Film and TV: a heading followed straight by dialogue; (V.O.) or (O.S.) written
    as a parenthetical (the fix moves it next to the name); parentheticals in
    capitals; FADE OUT. part-way through (it's FADE TO: between scenes).
  - Action describing thoughts ("remembers", "realises", "wonders"…) rather than what
    we see.
  - Cues using a full name, a written-out title (MISTER, DOCTOR), or two cues for one
    character, listed under Cast in Production.
  - Action paragraphs may now run to 5 printed lines (one beat each), not 4.
  - The title page note also asks for contact details.
- **Spec or shooting script** (under Export, for film and TV). A spec script, the
  default, gets notes for camera directions and CUT TO:, and leaves the draft off
  the title page. A shooting script allows them.
- **Title page contact details** (`contact:` in project.yaml, one line each for
  address, phone and email) print in the bottom-left corner, and in Fountain exports.
- **Exports open and close as the guides set out**: film starts with FADE IN: and ends
  FADE OUT. and THE END; every TV act starts with FADE IN: and ends with FADE OUT.,
  and the last act with END OF SHOW. Nothing is doubled if you've typed them.

## 0.4.0 (2026-11-02)

- **Script check.** As you write, likely mistakes are underlined (wavy) and
  conventions noted (dotted); point at one to see why, and fix it in one click where
  the fix is certain. The status bar counts them (click to list them; F8 goes to the
  next). Production lists every issue in the play, and clicking one shows its line.
  - Warnings: a cue with a blank line after it (so the speech reads as action), a cue
    not in capitals, a cue or singer that isn't in the cast list ("did you mean…?"), a
    song never closed, ~ END SONG with no song, a parenthetical missing its ")", and
    dual dialogue (^) with nothing beside it.
  - Notes: (CONT'D) after a different speaker, or typed by hand when it's automatic
    (or spelt (CONT.)); someone "on stage but silent" who speaks; lyrics outside a
    song; the title page missing a title or author.
  - Film and TV: a scene heading with no blank line after it (other apps won't read it
    as a heading) or with description in it ("- MORNING. SPRING." — the fix moves it
    into the action); no or a misspelt time of day (NITE); INT without a full stop;
    scenes not starting with a heading; action lines running together without a blank
    line; action paragraphs longer than 4 printed lines.
  - Characters in film and TV: introduced once with their full name in capitals, then
    written with their **short name** (set in Cast, e.g. Joanna): capitals after the
    introduction are noted, with the short name as the fix, and typing the first
    letters of a short name in the action suggests it.
  - Any check can be switched off for a project ("Don't check for this", or Checks…
    in Production); the setting is kept in project.yaml.
- **Spelling** is checked in the script with your computer's own dictionary (the
  Spelling button above the script turns it off).
- **(CONT'D) added automatically** when a character speaks again after only action,
  in the same scene: shown faintly after the cue (it isn't typed into your text, so it
  stays right as you edit) and printed in PDF and Fountain exports. On for film and
  TV, off for stage plays; switch it under Export.
- **The scene list can be hidden** (the button at the top left, or Ctrl+\) for a wider
  page, and episodes and acts can be folded (the ▾ by their names; Fold all /
  Unfold all at the bottom). Both are remembered; the open scene's act always shows.

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
