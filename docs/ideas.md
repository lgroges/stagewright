# Ideas

Things we've thought about doing. Move an entry to [decisions.md](decisions.md) when
it's settled, either built or turned down for good.

## Open

- **UK stage layout for export.** The character name at the left margin with a colon
  and dialogue on the same line, stage directions in capitals, "ACT I SCENE 1"
  headings, CURTAIN at the end (BBC stage format guide). At present the stage export
  centres the name above the dialogue. Put off on 2026-11-03.
- **BBC studio layout** for taped drama and sitcom. Scenes numbered (SC1.); the story
  day and time in the heading (DAY 1 [09.20]); names underlined with a colon; action
  in capitals (in square brackets for drama); a new page and a transition for every
  scene; END OF EPISODE. Only needed for studio shows. Put off on 2026-11-03.
- **Scene numbers on shooting scripts.** The guides say spec scripts don't have them
  and shooting scripts do. This would follow the `shooting_script` setting.
- **AI features** beyond the rule-based check, once we've seen how useful the rules
  are.
- **Hosting the co-writing relay.** It runs locally for now; where it should live
  for real is undecided.
- **A parenthetical wrapped onto two lines** is flagged as unclosed. Typing it on one
  line avoids this; a fix would be to treat the following line as its continuation.
- **A repeated-word check** ("are are"), which the system spellchecker misses. Raised
  on 2026-11-03 after a read of the Dee-Kelley scene.
- **Ages at introduction**: a note when a character's first appearance gives no age,
  such as (30s). Optional convention, so it would be easy to switch off.
- **Suggested edits**, like Google Docs' suggesting mode: proposed insertions and
  deletions shown in the text, to accept or reject. Put off on 2026-11-03 in favour of
  comment threads.
- **Comments in Production**: a project-wide list of open comments, like the script
  check's. At present the scene list counts them per scene.
- **Printing comments** in a PDF export (in the margin or as endnotes), for a read-through.

## Turned down

- **Importing from Google Docs.** Scripts are converted by hand with Claude instead.
- **Typing (CONT'D) into the text automatically.** It's shown as a label and printed
  in exports instead (see decisions.md).
