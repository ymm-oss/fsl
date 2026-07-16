# Workflow lessons

- When the user makes the native Rust implementation the completion authority,
  do not block delivery on a stalled full Python suite. Stop that suite when
  directed, retain only focused compatibility evidence, and finish the Rust
  workspace gates.
- Do not add Intel Mac to the regular CI matrix unless it is explicitly required;
  Apple Silicon macOS coverage is sufficient for this repository.
- Treat `skills/` as the distribution surface only. Put repository-internal
  workflow Skills under `.claude/skills` and `.codex/skills`, without adding
  them to `skills/`.
