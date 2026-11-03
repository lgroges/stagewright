# Notes for Claude

- Record decisions in docs/decisions.md (newest first: what, why, what we didn't
  do) and keep docs/ideas.md current: add ideas we put off or turn down, and move
  them to decisions.md when they're settled. Do this in the same commit as the work.
- Add user-facing changes to CHANGELOG.md under the version being prepared.
- Never publish Conversion (it lives only in lgroges/stagewright-archive) or commit
  the updater signing key.
- Tag pushes are blocked from Claude's environment; releases are started by Liam
  from Actions → release → Run workflow.
- docs/format.md is the reference for reading projects outside the app (Liam's
  `stagewright-projects` skill fetches it live). When a change touches the files the
  app reads or writes, update docs/format.md and bump its "Format version" in the
  same commit. If the change also makes the summary in docs/skill/SKILL.md wrong,
  update that file too and propose the updated skill to Liam (propose_skills, full
  SKILL.md).
