Changed (#1439): requirements acceptance `expect <expr>` and FSL 5 scenario
`then <expr>` now use the invariant's recursive Bool expression checker before
replay. Enum-vs-integer comparisons such as `c != 1` and `c == 1` fail with the
existing assignability message, `kind: "semantics"`, exit 2, and the authored
expect/then keyword location. Valid comparisons remain post-state predicates,
not invariants; well-typed false expectations retain `kind: "acceptance"`.
Trace-step arguments now use the same checker for internal well-typedness,
including unselected branches, without forcing the outer parameter type.
Errors point to the first authored step token: the action for calls, actor for
actor-first phrases, or first token (for example the instance) for reordered
particle phrases. Literal and internally
consistent conditional wrong-kind values retain `wrong_kind_argument`.
This fixes false-green traces, including an enum/integer condition selecting a
guard-rejected value. Breaking removal: these previously accepted, internally
ill-typed inline forms now fail with `kind: "semantics"`, exit 2 (IDs identify
the exact tests in `docs/design/DESIGN-requirements-stage.md`):

| Removal | Ill-typed trace form now rejected |
|---|---|
| T01 | acceptance Color argument with condition `c != 1` |
| T02 | forbidden setup Color argument with condition `c != 1` |
| T03 | forbidden final: `paint(if c != 1 then Red else Blue)` |
| T04 | FSL 5 scenario call with condition `claim_stage[0] != 1` |
| T05 | FSL 5 scenario with input, same malformed stage condition |
| T06 | FSL 5 scenario particle input, same condition |
| T07 | FSL 5 forbidden call setup, same condition |
| T08 | FSL 5 forbidden call final, same condition |
| T09 | FSL 5 forbidden with setup input, same condition |
| T10 | FSL 5 forbidden with final input, same condition |
| T11 | FSL 5 forbidden particle setup input, same condition |
| T12 | FSL 5 forbidden particle final input, same condition |
| T13 | acceptance numeric argument: `setn(if c != 1 then 2 else 0)` |
| T14 | FSL 5 marked/with hybrid input, malformed ClaimStage comparison |
| T15 | FSL 5 relation-role value with condition `ticket_stage[0] != 1` |
| T16 | acceptance unselected branch: `paint(if true then Blue else (if c != 1 then Red else Blue))` |
| T17 | native/shared diagnostic test's `expect c != 1` row |
| T18 | rewritten Worker diagnostic test's identical `expect c != 1` row |
| T19 | step: `paint(if o is some(z) then z else Red)` with `o = none` |
| T20 | expectation: `expect (if o is some(z) then z else Red) == Red` with `o = none` |
| T21 | reordered particle input: `claim1 を Employee が amount (if claim_stage[0] != 1 then 1 else 2) で submits` |

Historically, against base `5a539bbe`, all 579 forms per tree under `specs/`,
`examples/` and `rust/fslc/tests/fixtures/` in both modes still have a measured empty
ok-to-error transition set. Including the lint-rewritten test's 21 inline
fixtures, both modes twice give 4,800 checks: the original T01–T16 removals produce
21 mode/form transitions (T01–T03, T13, T16 in both modes; others only in
FSL 5), no other full-output/exit differences, no repeat differences or
excluded fields. Repair comparisons using members of the same enum, including
in unselected branches. See the design note for commands and measured identity.
The round 1 supplement checks all sources of the rewritten diagnostic tests and
changed-oracle consumers plus scope/reordered-particle controls: 20 forms per
tree, both modes twice, 160 checks against the same base and unchanged
production code. T17–T20 are additional breaking removals in both modes; T21
only in FSL 5 (five fixture occurrences / nine mode-form transitions; no other
differences or exclusions). T17/T18 are identical FSL in two tests. T19/T20
report `public Kernel cannot type identifier 'z'`: `is some(z)` binds only in
its true logical continuation (`=>` / `and`), not a conditional's `then` arm,
even if unselected. Invariants already reject that scope escape; use a supported
continuation or avoid the escaped name. T21's location is the first token
`claim1` (8:5), not the actor later in the phrase.

The shared checker now requires a Bool operand for `not` and an Int-compatible
operand (including named numeric ranges) for unary minus in every model
expression and trace. Unselected branches and short-circuited `and` / `or` do
not bypass typing. `not (c == Red)`, `-(-1)` and numeric `- n` remain valid.
The fresh sweep against base `5a539bbe` checks 579 corpus forms plus 51 inline
fixture occurrences per tree, both modes twice (5,040 checks), with an empty
corpus ok-to-error set. These seven additional inline **breaking removals**
all change from `ok` / 0 to `error` / `semantics` / 2 in both modes:

| Removal | Ill-typed unary form now rejected |
|---|---|
| U01 | forbidden step `paint(if true then Red else (if not 1 then Red else Blue))` |
| U02 | forbidden step `paint(if true then Red else (if -Red > 0 then Red else Blue))` |
| U03 | forbidden step `paint(if c == Red or not 1 then Red else Blue)` |
| U04 | acceptance `expect c == Blue or not 1` after `paint(Blue)` |
| U05 | invariant `c == Red or c == Blue or not 1` |
| U06 | invariant `c == Red or c == Blue or -Red > 0` |
| U07 | acceptance `expect not (false and not 1)` after `paint(Blue)` |

These add 14 mode/form transitions to T01–T21 (44 total); there are no other
full-output/exit differences, repeat differences or excluded fields. Use Bool
for `not` and numeric operands for unary minus, even in skipped expressions.
See `DESIGN-requirements-stage.md`, Unary operand typing, for commands and
the exact measured base/head identities.

Current rebase sweep: BASE `23eb5e41326ef4cf81a100ad0e011e81ed9fd507`,
HEAD `1e27d896b69b395ad8e48c570b4ddbca50e442a0`, built in their own trees.
`BINARY check FORM` covers 594 corpus forms and 65 inline occurrences per tree
in both modes twice (5,272 checks). The corpus ok-to-error set remains empty;
the historical 51-inline subset retains all 44 recorded mode/form removals.
Including every inline form from all five fixed test files additionally records
acceptance `expect` / FSL 5 `then` predicates `c != 1`, `true and c != 1`,
`not (c == 1)`, `if c != 1 then true else false` and
`forall color: Color { color != 1 }` as **breaking removals** for enum `c: Color`:
acceptance in both modes, scenario only in FSL 5 (15 additional mode/form
transitions, 59 total). Repair comparisons with same-enum members. The two
corresponding `c == 1` fixtures remain errors but change their diagnostic from
`acceptance` to `semantics` at the expectation keyword, with exit 2 unchanged (three mode/form
changes). These account for all 62 full-output differences, with no others,
repeat differences or exclusions. S1 calibration passed BASE and rejected HEAD
twice. See the design note for inventory commands and full measurement scope.
