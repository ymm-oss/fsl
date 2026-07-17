<!-- SPDX-License-Identifier: Apache-2.0 -->
# Toggle Spec

A simple toggle modeled in FSL.

## Model

```fsl
spec Toggle {
  state { active: Bool }
  init  { active = false }
```

## Actions

```fsl
  action toggle() {
    active = not active
  }
```

## Properties

```fsl
  invariant AlwaysBool {
    active or not active
  }
}
```

That's the whole spec.
