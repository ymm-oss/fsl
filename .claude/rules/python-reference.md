---
paths:
  - "src/fslc/**/*.py"
  - "pyproject.toml"
---

# Frozen Python reference rules

- `src/fslc` is a frozen compatibility reference and retained LSP surface, not the product path for new
  native CLI behavior.
- Before editing it, identify the explicit compatibility, public Kernel, packaging, or LSP contract that
  requires the change. If none exists, make the change in Rust only.
- Do not use Python structure as the template for Rust internals. Use observable behavior and accepted
  contracts as evidence.
- When Python must move, add focused compatibility evidence and verify the Rust contract remains
  authoritative.
- New Python files require the Apache-2.0 SPDX and copyright header used by neighboring files.
