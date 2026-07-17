# Workflow lessons

- Before scoping an issue, fetch `origin` and inspect recently merged pull
  requests that touch the same contract surface; a locally cached
  `origin/main` is not evidence that the issue's prerequisites are current.
- When the user makes the native Rust implementation the completion authority,
  do not block delivery on a stalled full Python suite. Stop that suite when
  directed, retain only focused compatibility evidence, and finish the Rust
  workspace gates.
- Do not add Intel Mac to the regular CI matrix unless it is explicitly required;
  Apple Silicon macOS coverage is sufficient for this repository.
- Treat `skills/` as the distribution surface only. Put repository-internal
  workflow Skills under `.claude/skills` and `.codex/skills`, without adding
  them to `skills/`.
- Do not preserve a chronological field-trial report merely because earlier work used
  that format. Distill reusable findings into authoritative design rules, tests,
  skills, or repository instructions; retain a separate report only when its raw
  experiment method or evidence has independent long-term value.
