Root synchronized actuals, including unused formals, are checked before substitution
in the root header/state/const/enum context. Preserve numeric carrier compatibility
and verification-time bounds; root diagnostics retain whole/legitimate-inner
argument spans and bare ownership without changing action/Kernel metadata.
Public SyncRef requires arg_spans; validated conversion supplements raw lossy APIs
and the three CLI migrations each add full core validation, not free work.
A conditional's arms now check against the supplied expectation, not the inferred carrier: `m[if c then 0 else true]` with `m: Map<A, Bool>` now names `Named("A")` instead of `Range(0, 1)`. This arm change altered no measured admission result.

Measured `check` removal union: **11 complete contexts /9 root-component pairs
/22 mode-context pairs**. Base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d`;
head `7653cc784354bc562bd85bd888cf78d0091272f2`. Each mode is
stable or FSL5-opt-in on unchanged source, twice per side. Every transition below
is check-ok/exit0 → semantics-error/exit2. The nine new exact three-file `check c.fsl`
contexts are preservation only; physical direct.fsl presence grants no direct/API/
verify coverage. This finite measured union is not runtimefull472 or issue acceptance.

| ID | Exact complete-context key | Pair | Changed admission / candidate loc |
|---|---|---|---|
| R01 | `1cae9cf75cb49d332ab850296a1902db6e08971cbae68ab7229309b979632f05` | P01 | unused true + relation index, argument order; 5:16 |
| R02 | `456840abf00faf2844ad3223e80abab2efb03e62ec5c2fc4c6c5cef9daa45af4` | P02 | body-only x after let; 5:16 |
| R03 | `5aa3baefa078f9ddc0a1d5ed5157e9bd6295df700bf86536d7042d774e69e0d2` | P03 | true then later action index; 5:16 |
| R04 | `8d7b0467683a00853cc958fcf91c33e6e23d32c7cd5506d1401029e98338b5d9` | P04 | relation index; no good sibling; 5:16 |
| R05 | `a1760ef370b7c9164f0eda14b36c84dfa208b847c75fb6ecb8bfa8e5faa8df22` | P05 | unused true; no good sibling; 5:16 |
| R06 | `ab059cd5d0c142246d68ccb9ce2ea193e43ce9b4365e6a865e0a7b46c63942ac` | P06 | true then second reference index; 5:16 |
| R07 | `ab291946a038aa0e77692160e7516fa40a75b8781797c928405dddfd8949bbbe` | P04 | relation index + offered good sibling; 5:16 |
| R08 | `acc62ea03d44de4dd0ddbcd65d4f73e4737e914153f50bed7832a4bb4905da3b` | P07 | body-only x after let/requires; 5:16 |
| R09 | `cd076b93cba275c70735ba2a73731c1935fabcdb0fe16f3246b5ba615e720ded` | P08 | parenthesized unused true; 5:16 |
| R10 | `dd449ab57104f7c5a53e33d3f756937344e76161615f960cb821e2e62372adff` | P05 | unused true + offered good sibling; 5:16 |
| R11 | `91412ae303f1f1877fcdcea2e7e35912726e9832662e0c49d971d8ca09a9bc02` | P09 | second ref `(0), (true),`; 6:37 |

Exact source identities and per-context scope are recorded in
DESIGN-sync-argument-root-validation.md. Mandatory610 disk forms had no removals;
the finite inline union is not an empty set or runtimefull472. Direct API controls
compiled and passed on the named state. The caller cohort, on an earlier candidate and not rerun on this head, measured seven states/
four builds/16CLI observations, twice on fixed valid inputs: R/I/M honestly survived,
and exact restores preserved output with fresh H0 bytes/no restore builds. Full
stdout/JSON/exits/non-time stderr matched; only the observed, framing-validated
mutate elapsed scalar was not compared literally. No caller-specific semantic kill
or isolated incremental cost is claimed; broader execution/issue/product gates
are not waived.
