<!-- SPDX-License-Identifier: Apache-2.0 -->

# Canonical ID policy

## Status

Accepted.

## Problem

FSL has several semantic ID owners (`requirement`, `acceptance`, `forbidden`,
business `policy`/`goal`/`control`) and several relationship syntaxes
(`@requirement` and process `covers`). The parser intentionally accepts broad
identifiers, so syntax validity alone cannot establish a stable project naming
convention. Older declaration metadata also combined an ID and prose in one
string (`"REQ-1: text"`), which obscured whether the string owned an ID or
linked a declaration to one.

## Decision

### Canonical syntax

- A semantic declaration owns the ID written after its declaration keyword:
  `requirement REQ-CHECKOUT-001`, `acceptance AC-CHECKOUT-001`, and
  `forbidden FB-CHECKOUT-001`.
- A kernel or dialect declaration links to an owned requirement with the typed
  `@requirement("REQ-CHECKOUT-001", "text")` annotation.
- Process `covers REQ-CHECKOUT-001 "text"` remains canonical dialect sugar for
  the same typed relationship.
- `"REQ-1: text"` declaration metadata remains readable for migration but is
  non-canonical. `fslc migrate --edition next` converts it to the typed
  annotation form when the existing migration safety checks permit that edit.

The language grammar remains permissive. Naming policy is a lint concern and
does not change parsing, lowering, verification, or Public Kernel identity.

### Built-in default

`fslc lint` applies this policy even when no project file is supplied:

| Semantic kind | Accepted template(s) |
|---|---|
| requirement | `REQ-{scope}-{number:3}`, `NFR-{scope}-{number:3}`, `INV-{scope}-{number:3}` |
| acceptance | `AC-{scope}-{number:3}` |
| forbidden | `FB-{scope}-{number:3}` |
| policy | `POL-{scope}-{number:3}` |
| goal | `GOAL-{scope}-{number:3}` |
| control | `CTRL-{scope}-{number:3}` |
| model | `MODEL-{scope}-{number:3}` |
| assumption | `ASSUME-{scope}-{number:3}` |

`scope` is one or more uppercase ASCII alphanumeric segments separated by
hyphens. `number` is decimal; `number:3` requires exactly three digits. The
model and assumption classes apply to typed requirement links whose configured
literal prefix identifies those verification-only roles.

Numeric components retain their source spelling in the surface AST and typed
model. In particular, `001` is not normalized to `1`; the canonical spelling is
also the stable requirement identity carried into evidence.

### Project override

Projects may pass an explicit manifest to lint:

```console
fslc lint specs/ --project fsl-project.toml
```

```toml
[id_policy.patterns]
requirement = ["PAY-{number}", "NFR-{scope}-{number:3}"]
acceptance = "TEST-{number}"
```

Each configured key replaces only that semantic kind; omitted kinds retain the
built-in default. A value is either one string or a non-empty string array.
Supported placeholders are `{scope}`, `{number}`, and `{number:N}` where `N`
is positive. Unknown kinds, unknown placeholders, invalid widths, and empty
lists are configuration errors (exit 2).

The repository manifest reader intentionally supports a closed TOML subset:
policy values use double-quoted JSON-compatible strings and arrays without
trailing commas or inline comments. Single-quoted TOML strings are rejected as
configuration errors rather than being interpreted as pattern characters.
Model and assumption templates must begin with distinct literal prefixes that
overlap neither each other nor requirement templates, so a typed
`@requirement` link has one deterministic class.

Manifest discovery is deliberately explicit: lint does not search parent
directories. The JSON result records `id_policy.source` and the fully resolved
`id_policy.patterns`, so CI evidence identifies the exact active convention.

### Diagnostics and migration boundary

A mismatch produces `non_canonical_id` with taxonomy `non_canonical`, the
exact ID span, and the accepted templates. It is a warning in the current
edition and an error in the next edition under the existing edition severity
contract. Lint exits 1 when any finding exists.

The diagnostic is never machine-applicable. Renaming an ID requires coordinated
updates across specifications, tests, code annotations, telemetry, and external
evidence; guessing that graph from one source file would be unsafe. Therefore
`fslc migrate` does not rename IDs and the policy is not applied by `check` or
`verify`.

## Consequences

- New projects receive a deterministic convention without configuration.
- Existing projects can adopt their established taxonomy one semantic kind at
  a time without weakening unspecified kinds.
- Surface roles remain distinguishable even where lowering shares a typed
  requirement relation.
- ID naming stays an auditable policy layer rather than becoming a restrictive
  grammar feature.
