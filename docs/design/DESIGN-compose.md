# FSL v2.0 — spec composition (compose) implementation design

DESIGN-v1.md §10 v2.0 "composition of multiple specs". It combines multiple verified
component specs into a single system spec by **namespaced interleaving composition**.

Design core: composition is defined as a **front-end AST transformation** (prefixed
merge). After lowering it becomes an ordinary single spec, so BMC / k-induction /
scenarios / coverage diagnostics / leadsTo / runtime Monitor / replay / testgen / refine
all run against the composed spec **with no changes whatsoever**.

## 1. Syntax

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  // additional composition-side state (optional)
  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // synchronized action: execute actions of multiple components together in one step
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].amount > 0
    orders_linked = orders_linked + 1
  }

  // remove a component's action from standalone execution (only fires via synchronization)
  internal cart.checkout
  internal pay.capture

  // cross-cutting invariant / reachable / leadsTo (referenced via alias.var)
  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<relative path>"`: the path is relative to the compose
  file's directory. The spec name must match the spec name in the file. The alias is
  unique within the compose. A compose inside a compose is **not allowed** (v2.0 is a
  single level).
- A component's types, state, and actions are referenced as `alias.Name`.
- **Synchronized action** `action <name>(<params>) = <a>.<act>(<arg exprs>...) [ || <b>.<act2>(...) ]* { additional items }`:
  - requires = the conjunction of each component action's requires + additional requires
  - body = the union of each action's body + additional statements (simultaneous
    assignment; since the write targets are disjoint across components and additional
    statements can only write composition-side state, there is no conflict —
    synchronizing two actions of the same component is not allowed)
  - Breaking removal (#1334): additional statements also reject component-state
    assignments written with physical names such as `alias__x`, at the assignment's
    location; those spellings previously passed `check`. This removal changes
    acceptance only for physical-name component-state assignments in sync additions.
    Before/after binaries, each in its own materialized tree, ran `fslc check`
    on all 579 `.fsl` files under `specs/`, `examples/`, and
    `rust/fslc/tests/fixtures/`. Recorded exit codes, results, and the first
    160 characters of each message agree for every file. There were
    **zero ok-to-error transitions** (this write form does not occur in that
    corpus). No pre-existing required tests were
    rewritten, renamed, deleted, or stopped; only new regression tests were added.
  - Breaking removal (#1442): reference arguments read only header parameters,
    consts, and state, never bindings from the additional items' `let`s.

    **Migration contract (#702, limited to sync reference argument scope):**

    1. **Versions/editions:** reproduced with native 4.12.0 and the default/current
       edition at base `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9` (accepted) and
       the scoped compose implementation (rejected, also reporting 4.12.0).
       This is the v5.0.0 contract; no edition bypass is provided. The earliest
       affected release has not been established.
    2. **New rejection:** `action s() = b.go(t) { let t = true }` no longer
       checks when the additional `let` is the only declaration of `t`.
       For the measured five-line component with
       `action go(v: Bool) { flag = v }`, the exact diagnostic is exit 2,
       `result: "error"`, `kind: "semantics"`, message
       `invalid model expression: public Kernel cannot type identifier 't' at 4:24`,
       and `loc: {"line": 4, "column": 24}`. The location depends on the input;
       it is not a fixed location for every rejected reference.
    3. **Rewrite:** declare a header parameter, for example
       `action s(t: Bool) = b.go(t) { }`, and supply the intended value at its
       call sites, or represent the intended value in composition-side state
       and pass that state expression. Header parameters, consts, and state
       remain valid argument sources. Review the action interface and state
       semantics: these rewrites are not automatically equivalent to an
       additional local binding. There is no automatic migration.
    4. **Commands:** run unscoped `fslc check composed.fsl`, correct each
       out-of-scope reference, repeat `check`, then run
       `fslc verify composed.fsl --depth 8 --no-cache` on the corrected source.
       There is no dedicated lint/migrate auto-fix for this scope change;
       a selected-property verification run is not a migration detector.
    5. **Verdicts/evidence:** a previous `ok` now becomes the type error above;
       do not reuse verification evidence for the removed source form after
       rewriting it. Existing JSON envelopes and assurance classes are unchanged.
       The recorded `fslc check` sweep of all 579 `.fsl` files under `specs/`,
       `examples/`, and `rust/fslc/tests/fixtures/`, from the base above to
       `acd81719248dd7740b942e4ee3fff0c56892e6be`, has zero ok-to-error
       transitions; this inline form is not in that corpus. Its base acceptance
       and new rejection were each reproduced twice, and the regression also
       checks accepting header-parameter and composition-state rewrites.
    6. **Consumers/artifacts:** native CLI, Worker/shared compose loading,
       hooks, CI, and downstream callers must handle the rejection rather than
       treating an old `ok` as current acceptance. Do not use editor silence
       instead of `check`. Reverify corrected sources, regenerate affected
       scenario/testgen/replay/ledger/document/HTML artifacts, and recheck
       approval records and implementation conformance against the new inputs.
    7. **Rollback:** v5.0.0 has no compatibility bypass or grace period for this
       form. Returning to an older accepting binary is a version rollback,
       not evidence that the input satisfies the v5 contract; revalidate its
       artifacts with that binary. No support duration is promised. This note
       does not implement #702's general release gate.
  - Breaking removal (#1442, original component names): a composition cannot
    make an originally undeclared component value read or update root valid.
    Resolution uses the component's own declarations and lexical bindings
    before prefixing and merging, for actions, properties, and initialization.
    A bare enum member retains its owning enum identity when an unrenamed
    public value could capture it; otherwise its existing representation is
    retained. Its printer uses the source member spelling. Public header names
    remain legal; private capturers use the shared hygiene rules instead.
    Display-only provenance on the reified node distinguishes these bare reads
    from conversion, abstraction, and user-qualified enum literals, whose owner
    qualification is preserved; semantic Kernel AST/JSON output is unchanged.
    Pattern collisions follow the ordinary type checker's aggregate phase:
    a pattern sharing a normalized non-typed `count` or `sum` binder is accepted
    exactly as in the standalone component, while non-expanding `forall` and
    `unique` retain their refusal. Source rewriting and hygienic renaming use
    that same phase decision, including independent header-argument scopes.
    The ordered free-reference walk is shared with model checking; set-valued
    capture inventories do not change preorder diagnostic selection. Late
    component errors use the checked model's use-site wrapper and retain the
    inner cause and the component's file, line and column.
    Helpful-action arguments also use the existing expression checker with
    the action parameter's expected type and the property's binder scope.
    This is a breaking closure of a previously unchecked position, not a new
    normalization rule. Original-source aggregate pattern decisions survive
    normalization as check-only data: a surviving pattern retains its original
    refusal, while an erased empty-domain pattern does not invent one.
    The helpful argument type gate (#1527, addressing part of #1483) reports the
    `helpful` clause location, not a separate span for every argument.
    State independence remains induction-only (#1483); `check` and BMC accept
    well-typed state reads. Operand validation for `abs` was supplied by #1533.

    **Migration contract (#702, limited to original component name resolution):**

    1. **Versions/editions:** reproduced with native 4.12.0, default/current
       edition, at base `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9`: a component
       with an undeclared read can fail alone yet check inside a composition
       whose header or state supplies that spelling. This is removed for
       v5.0.0. The earliest affected release has not been established.
    2. **New rejection:** an undeclared `u` in a component is no longer supplied
       implicitly by a sync header parameter, composition state, foreign enum
       member, or an own/foreign generated state or const spelling. The paired
       composition retains the standalone component's full error, not a new
       capture diagnostic. The aligned regression's read reports exit 2,
       `kind: "semantics"`, message
       `invalid model expression: public Kernel cannot type identifier 'u' at 12:3`,
       and `loc: {"line": 12, "column": 3}`. Update roots retain the original
       `invalid model expression: unknown update target ...` diagnostic with
       their own source position. Locations depend on the input layout.
    3. **Rewrite:** correct or declare the intended name in the component's
       original scope. A local must precede a guard that reads it; bodies and
       ensures still use the complete normalized guard environment. If data
       comes from the composition, declare a component action parameter and
       pass the header/state value explicitly. Properties and initialization
       must reference their own declared inputs. Review the intended state and
       interface semantics; there is no automatic migration.
    4. **Commands:** run `fslc check component.fsl` and
       `fslc check composed.fsl`, fix the source-owned declaration/reference,
       repeat both checks, then run
       `fslc verify composed.fsl --depth 8 --no-cache` on the corrected input.
       No lint/migrate auto-fix or selected-property verification bypass is
       provided for missing component declarations.
    5. **Verdicts/evidence:** a formerly accepted composition can become the
       same exit-2 error as its standalone component; old verification evidence
       does not justify the removed source form. JSON envelopes and assurance
       classes are unchanged. The base-to-fixed `fslc check` sweep of all 579
       `.fsl` forms under `specs/`, `examples/`, and
       `rust/fslc/tests/fixtures/` has zero ok-to-error transitions; the invalid
       forms are covered separately by 182 paired standalone-refusal cells.
       Corrected sources require fresh verification evidence.
    6. **Consumers/artifacts:** native CLI, Worker/shared loading, hooks, CI,
       and downstream consumers must handle the preserved component error;
       editor silence is not an acceptance gate. Reverify corrected inputs,
       regenerate affected scenario/testgen/replay/ledger/document/HTML
       artifacts, and recheck approval bindings and implementation conformance.
    7. **Rollback:** there is no compatibility bypass or grace period. An
       older accepting binary is a version rollback, not v5 conformance; its
       artifacts must be revalidated against that binary. No support duration
       is promised. This is not #702's general release gate.
  - ensures are also inherited from each action
  - argument expressions may use composition-side parameters, consts, and state
    expressions
  - fairness is **not** inherited: if a fair component action is synchronized
    into a non-fair synchronized action, `check` / `verify` emits a
    `fair_not_inherited` warning in the JSON `warnings` array; declare
    `fair action <name>(...) = ...` on the synchronized action to make the
    composite action fair
- `internal <alias>.<action>`: excludes that component action from standalone
  interleaving (it is executed only via a synchronized action).
- An ordinary `action` (without `=`) may also be written (a glue action; reads and writes
  composition-side state and component state directly via `alias.var`).
- Compose `init` may assign initial values to composition-side state and component
  state, using `alias.var` or its physical `alias__var` spelling. If it overlaps a
  component's own `init` assignment to the same location, the existing duplicate
  initialization error rejects it; neither assignment takes precedence.

## 2. Semantics (= definition of the transformation)

In the stage before `build_spec`, the compose is **expanded into the AST of a single spec**:

1. For each use, parse the target file, and prefix all declaration names (the type
   namespace including type names and enum members, state variables, actions, and
   invariant/reachable/leadsTo names) with `<alias>__`, rewriting references inside the
   body. Enum **member names** are not prefixed (since the type carries the namespace,
   there is no collision — but even if two components export enums with same-named
   members, member resolution is done in type context, so it is a check error only when
   ambiguous).
2. Rewrite `alias.x` references in the compose body to `alias__x`.
3. Expand synchronized actions into a flat action by the rules of §1 (the
   component action's formal parameters are replaced by the synchronized
   argument expressions).
4. Remove `internal`-designated actions from the spec's action list, and use them by
   **copying** the body when expanding synchronized actions.
5. The components' invariants / automatic _bounds_ / reachable / leadsTo / fair survive
   as-is under prefixed names (all are checked). A synchronized action keeps
   only its own `fair` marker; constituent fairness is not propagated, and a
   non-fair synchronized action that references fair constituents records a
   `fair_not_inherited` warning.
6. Display metadata: a physical variable `alias__x` is displayed in JSON as `alias.x`
   (a display-name map in logical_state_values. Traces, witnesses, CTIs, scenarios, and
   the Monitor state all follow).
7. Component action names (#1234): lowering records, for every surviving
   component action, state variable, and property, its `(alias, name)` pair
   keyed by the physical name (#1284; before it, state variables and properties
   kept only the physical name, enough for `cost.properties` to tell a
   component declaration from a glue one). The table is the model's
   `fsl_core::NameMap` (`rust/fsl-core/src/naming.rs`), which is also the only
   place that spells the `alias__name` physical name and the names the engines
   build (`fsl_core::SyntheticName`). No declared name contains `__` (#1256:
   check rejects one, `fsl_core::reject_declared_separator`, before lowering),
   so the physical name `alias__name` has one reading. Before #1256 it did not
   (`a__b` + `c` and `a` + `b__c` both gave `a__b__c`); the public name
   `alias.name` is still built from the pair, for an action, a state variable,
   and a property alike, and never parsed back out of the physical name. A name derived from
   an action (`_requires_failed_<action>`, `_partial_<action>`, an `ensures`
   violation) is rebuilt from the action that
   produced it, so the alias boundary comes from the same pair. Every output of
   `verify` (all engines, including `action_coverage`, `action_profile`,
   `cost.properties`, vacuity warnings, and induction CTIs), `sweep`,
   `scenarios`, `explain`, `testgen`, `conformance`, `replay` (including
   `--from-log`), `refine`, `diff`, `mutate`, `html`, `ledger`, `analyze`, and
   the `untagged` warnings of `check --strict-tags` publishes this name. `analyze` graph IDs carry it too
   (`action:alias.name`, `guard:alias.name:<i>`, `effect:alias.name:<i>`,
   `ensures:alias.name:<i>`), as do the nodes' `name` fields, `--export
   tag-review` names, `undecided:` records (`declaration`/`node`), and the
   project `traceability_graph` (`action_map:`/`stutter_map:` IDs and
   `maps_action` endpoints such as `design:action:alias.name`). Every graph and
   finding builds an action's node ID through one function (`action_node_id`),
   so an edge never ends at an ID no node declares. The frozen Python reference
   keeps `action:alias__name` in these IDs; this is an intended difference
   (decision 4 of #1234 makes the key `.` as well). Keyed lists are ordered by
   the name they print, sorted with the same function that renders the key, so
   `acct.Inv` sorts before `acct2.Inv`. A v1 replay trace resolves an action by
   its public spelling or its physical name (`NameMap::resolve_action`), so a
   component action is `alias.name` or the pre-#1234 `alias__name`. Public Kernel export (`kernel`, `typestate`) and the
   RCIR document projection reject compose, so they print no component names.
   State-key, state-graph-ID, and property display still uses the string rule
   above, so with a `__` alias a state variable is shown as `a.b__n` while Python
   shows `a__b.n`; this is a known, unresolved difference from the Python
   reference.

Static checks (check stage, `kind: "type"`):
- missing use file / spec-name mismatch / duplicate alias
- failure to resolve `alias.x` (unknown alias / variable / action)
- referencing multiple actions of the same component in a synchronized action
- nonexistent internal target
- a compose inside a compose

### 2.1 Cross-spec parameter compatibility

Synchronized action arguments are matched **structurally**, not nominally. During
compose expansion, component declarations are prefixed (`core__TaskId`,
`note__NoteId`), then each synchronized action body is copied with its formal
parameter substituted by the argument expression. There is no check that the
argument's declared type name equals the callee's parameter type name; both are
encoded as bounded integer values, and the callee state's implicit `_bounds_*`
invariant enforces the target range during verification.

For example, this composition is accepted and verifies because both domains are
`0..2`:

```fsl
spec Core {
  type TaskId = 0..2
  state { selected: TaskId }
  init { selected = 0 }
  action choose(t: TaskId) { selected = t }
}

spec Notes {
  type NoteId = 0..2
  state { last: NoteId }
  init { last = 0 }
  action attach(n: NoteId) { last = n }
}

compose CrossSpecSameRange {
  use Core as core from "core.fsl"
  use Notes as note from "notes.fsl"
  action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }
}
```

Observed result: `fslc check compose_same_range.fsl` returned
`result:"ok"`, and `fslc verify compose_same_range.fsl --depth 1` returned
`result:"verified"` with action coverage for `sync`.

If the target domain is narrower, `check` still succeeds but verification can
fail on the target component's implicit bounds:

```fsl
spec NotesNarrow {
  type NoteId = 0..1
  state { last: NoteId }
  init { last = 0 }
  action attach(n: NoteId) { last = n }
}

compose CrossSpecNarrow {
  use Core as core from "core.fsl"
  use NotesNarrow as note from "notes_narrow.fsl"
  action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }
}
```

Observed result: `fslc check compose_narrow_verify.fsl` returned
`result:"ok"`, while `fslc verify compose_narrow_verify.fsl --depth 1`
returned `result:"violated"`, `violation_kind:"type_bound"`, invariant
`"_bounds_note.last"`, with `sync(t: 2)` as the counterexample.

Recommended idiom: if two components intentionally share an identifier domain,
declare same-range component-local domain types and name the synchronized
parameter after one side (`t: core.TaskId` is fine). If the ranges differ, add a
`requires` guard on the synchronized action, or model an explicit conversion /
translation in one of the component specs before passing the value to the
narrower component.

## 3. CLI

No new command. `fslc check / verify / scenarios / replay / testgen` accept a compose
file as-is (if the parse result is a compose, it is expanded before normal processing).
A compose can also be passed to the impl side of `fslc refine` (since after expansion it
is a single spec).

Public Kernel v1/v2 still reject compose export until component file provenance is
truthful. Native `testgen` therefore selects the documented explicit compose metadata
constructor before export; it passes checked names/order plus the versioned
single-path testgen trace into the same normalized target adapter and preserves
compose support without weakening the public contract.

## 4. Implementation notes

- grammar.py: `compose_def` (use / internal / sync action). `alias.x` collides with the
  existing `field` syntax (same form as `o.st`) — during compose expansion, "field
  access to a name in the alias set" is rewritten as a namespace reference, and otherwise
  treated as a struct field as before (expansion runs only for compose, so there is no
  impact on existing specs).
- New module `src/fslc/compose.py`: `expand_compose(ast, base_dir) -> ast`. A pure
  transformation rewriting the tuple of parsed AST. Wire it from the CLI so `base_dir` can
  be passed to `parser.parse` (add a compatible `parse(src, base_dir=...)` library API).
- Display-name map: give the spec dict `display_names: {phys/logical: "cart.stock"}`, and
  look it up at the display sites such as `logical_state_values`. runtime.py uses the same
  map.
- File reading rides on the io error handling of check/verify (`kind: "io"`, pointing at
  which use via loc).

## 5. Test plan (tests/test_compose.py) + sample

Sample: `specs/order_system.fsl` (almost the same as §1: cart_v1 + payment,
synchronization of checkout and capture, cross-cutting reachable).

1. **Positive case**: order_system is verified (coverage all true, PaidOrder witness).
   With `--engine induction`, proved (the components' auxiliary invariants take effect
   as-is).
2. **Synchronization semantics**: after checkout_and_pay, the cart-side stock decrement
   and the pay-side Captured happen in the **same step** (confirmed via the witness
   changes).
3. **internal**: cart.checkout alone does not appear in coverage (it is not in the action
   list); removing internal makes it fire standalone too.
4. **Cross-cutting invariant violation**: a deliberately broken composition (a glue action
   making orders_linked negative) → violated, with trace state keys in the `cart.stock`
   form.
5. **Static checks**: duplicate alias / unknown alias / spec-name mismatch /
   nonexistent internal target / two-action synchronization of the same
   component → kind: type. Missing file → kind: io. Synchronized argument
   range mismatches are observed through normal verification, not as compose
   static checks (§2.1).
6. **Display**: across all JSON output (witness / scenarios / Monitor.state) the
   `cart.stock` display and no `__` leakage.
7. **runtime**: the Monitor runs as-is on a compose file (including replay).
8. No regression of existing specs (files not containing compose take the previous path).

## 6. Documentation reflection

- LANGUAGE.md "composition" section (syntax / synchronized-action semantics / internal).
- README / DESIGN-v1.md §10 note.
