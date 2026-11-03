# Notes for Claude

- Record decisions in docs/decisions.md (newest first: what, why, what we didn't
  do) and keep docs/ideas.md current: add ideas we put off or turn down, and move
  them to decisions.md when they're settled. Do this in the same commit as the work.
- Add user-facing changes to CHANGELOG.md under the version being prepared.
- Never publish Conversion (it lives only in lgroges/stagewright-archive) or commit
  the updater signing key.
- Tag pushes are blocked from Claude's environment; releases are started by Liam
  from Actions → release → Run workflow.
