<!-- SPDX-License-Identifier: Apache-2.0 -->
# Toggle — a literate FSL example

This document demonstrates **literate Markdown FSL**: a Markdown file whose
` ```fsl ` fenced code blocks are extracted and verified as a single
compilation unit by `fslc check` and `fslc verify`.

## Model

The toggle has a single Boolean state variable.

```fsl
spec Toggle {
  state { active: Bool }
  init  { active = false }
```

## Behavior

A single action flips the state.

```fsl
  action toggle() {
    active = not active
  }
```

## Invariant

The invariant is trivially true — it exists to demonstrate that verification
results (line numbers, counterexample locations) point to the Markdown
document's own lines.

```fsl
  invariant AlwaysBool {
    active or not active
  }
}
```

## Verification

```sh
fslc check  examples/literate/toggle.md
fslc verify examples/literate/toggle.md --depth 4
```
