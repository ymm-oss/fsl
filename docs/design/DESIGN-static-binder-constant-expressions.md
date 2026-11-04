<!-- SPDX-License-Identifier: Apache-2.0 -->

# Static binder constant expressions (#1480)

Status: Accepted. What `check` then refuses is decided by
[checked range admission](DESIGN-check-executable-forms.md#1480-checked-range-admission).
The original init-statement/Typed Struct refusal policies are recorded in
[checked executable forms](DESIGN-check-executable-forms.md#1480-shared-refusal-for-unsupported-binder-domains).

## Decision

Use the existing typed checked constant evaluator for successful integer folds
in three consumers: public Kernel aggregate normalization, symbolic finite
range expansion and static `leadsTo` wrapper expansion (also used by Monitor).
This does not introduce another evaluator or change concrete/runtime execution.

The additive native Rust API is
`KernelModel::constant_integer(&KernelExpr) -> Result<i64, ModelError>`.
It delegates to the existing private integer constant checker with the model's
**current** public `consts` map on every call. There is no cache, public typed
constant representation, general evaluator export or new root re-export.
It evaluates constants, not lexical locals or state initializers.
Before folding, the existing free-state-reference walker checks the current
model state. A free state read is nonconstant even when `consts` contains that
name. Ordinary const/state names can coexist; this is not a new collision rule.
For example, with const `n=0` but state `n=1`, `0..abs(n)` must not newly expand
as `0..0`: it retains the old symbolic unsupported-bound refusal while explicit
execution can exhibit the violation at step 0. The private declaration constant
checker is unchanged. The plain `0..n` sibling used to take the const's value
under a same-named state, so BMC reported verified and induction proved while
explicit violated at step 0. The planner now reads the state: with const `n=0` and
state `n=1`, `forall i in 0..n { i == 0 }` is `violated` on BMC, induction and
explicit (measured against base `5564b25cc`, which reports verified, proved and
violated).

## Successful grammar and arithmetic

The shared constant grammar includes integer/Boolean literals and constants,
integer negation, `not`, binary `+ - * / %`, integer comparisons, same-type
`== !=`, Boolean `and or =>`, `if condition then value else value`, integer
`min`/`max` and `abs`. The final result must be integer; Boolean conditions do
not imply Boolean-to-integer coercion. Unsupported expression forms do not fold.

Arithmetic is checked signed i64 arithmetic. Division and remainder are
Euclidean, including negative operands: `-5 / 2 == -3`, `-5 % 2 == 1`.
Overflow and zero divisors return errors. Both conditional arms are name- and
type-checked, but arithmetic executes only in the selected arm. Thus
`if false then 1/0 else 1` folds to 1; a dead Boolean arm opposite an integer
arm remains ill-typed. Binary Boolean operands remain eager, not short-circuiting.
Inclusive candidate order, empty reversed ranges, membership and binding values
do not change. Body/filter validation still precedes aggregate expansion.

## Failed folds are not a new admission policy

Each consumer tries the common fold and uses its value **only on success**.
Otherwise it runs its exact previous helper, including recursion through that
legacy helper rather than through fold-enabled children. For example,
`-(if true then MIN else 0)` retains the old unsupported-conditional refusal
instead of exposing a new negation-overflow diagnostic from a folded child.
Errors never become empty candidate lists.

Folding does not install a uniform invalid-static check gate or propagate the new
API's errors indiscriminately. The temporal-only extension is specified below. Existing typed
binding validation and constant-shadowing rejection own name provenance. No outer
lexical name is replaced with an initial state value or a same-spelled model constant.

## Fixed-outer temporal amendment (#1480)

For unfiltered outer `leadsTo` wrappers only, an inner integer range may use
already bound enclosing wrapper locals with the checked grammar above. Each
range is inclusive and evaluated for its enclosing assignment, rather than as
a rectangular product: `forall i in 0..1 { forall j in 0..i { ... } }` has the
ordered roster `(0,0), (1,0), (1,1)`. A Bool wrapper followed by
`0..(if b then 1 else 0)` yields `(false,0), (true,0), (true,1)`. Bool remains
Boolean; named finite integers and enums retain their declared type identity.
Existing carrier order, duplicates and binding restoration between siblings and
properties are preserved. A same-named inner binder's own bounds see the prior
binding; its body sees the latest binding. Checked-source const-shadow rules
remain unchanged.

A real enclosing local takes priority over model state or a same-spelled const.
A free state read is refused before constant fallback, including plain `0..n`
when state and const share `n`; no state initializer supplies a wrapper bound.
This corrects the temporal expander's old namesake-const roster, while retaining
the verifier's existing free-state eligibility refusal. It does not repair or
extend ordinary symbolic, action, init, aggregate or P/Q quantifier domains.
`within` remains a nonnegative declaration constant, never an outer-dependent
expression. Collection/filtered wrappers, structs and explicit `leadsTo` remain
unsupported. This adds no syntax, LSP node or public Kernel/JSON schema.

Dependent bounds execute in written binder order, lower before upper, for each
reachable prefix. An earlier reachable arithmetic error precedes a later
static or qualified-type error. Unreachable dependent arithmetic is not executed;
empty prefixes do not mask independent invalid-static, carrier, collection,
filter or type failures. Every authored endpoint is name/type-checked using prior
declared wrapper types, including its condition and both arms, even after an empty
prefix. This checks types without arithmetic, aggregate normalization or enumeration.
Independent static failures keep each consumer's exact
legacy diagnostic and priority. Checked-source validation and existing raw-Kernel
guards keep their separate scopes; this adds no general raw-model validator.
One additive Rust-only Core traversal seam permits read-only, reject-only checks
at each binder/domain and lower/upper evaluation point, retaining one ordered
Core roster. Typed errors (`E: From<ModelError>`) preserve consumer-owned
diagnostics unchanged. The existing `static_leadsto_bindings` wrapper stays;
callbacks cannot rewrite bindings or bypass Core guards. No solver dependencies,
new schemas or root re-exports are introduced.
A newly reached dependent division by zero is `ModelError("division by zero")`
with `origin`, `span`, `diagnostic_code`, `hint` and `quick_fix` absent and
`name_resolution: false`; conversion retains Runtime `partial_operation: None`
and verifier `is_solver_failure: false`.
Domain planning is not an action partial operation. Existing legacy errors are
unchanged.

## Preserved boundaries and evidence

The #1463 wrapper/filter/collection and init-source guards remain unchanged.
Quantifiers in P/Q are not wrapper binders. Explicit `leadsTo` stays unsupported;
bounded liveness metadata remains property-keyed with unchanged assurance.
Public Kernel's dynamic statement partial-operation inventory, read-order and
current-key rules, empty domains and literal/runtime paths retain their contracts.
Runtime's limited explicit constant helper is not the whole init arithmetic
route: its existing concrete fallback already executes those bounds.

The frozen independently authored #1480 matrix supplies literal candidate,
poststate and temporal expectations. The new direct API control checks mutation
of the public constant map, noninteger/unbound errors and nested failed folding.
Shared folding is not an independent semantic lineage or an assurance promotion.
The single CURRENT `X1/static-range` expectation becomes
`[Ok, Safe, Safe, Error]`, while its source table and dynamic sibling
remain unchanged. Required own-tree corpus/inline transition evidence is recorded
with the measured implementation identity; no new removal policy is authorized.

## Own-tree transition inventory

Measured B: `d11f32631f75afa64fa486cdf4f9b46c6e77d28e`; measured code H:
`c86f6bf4422b507f57862f830ad13726f8ce7734`. Each binary was built in its own
named materialized Git tree with Rust 1.98.0, offline/locked Cargo. The command
was `FSLC_CACHE=off <own-binary> check <tree-relative-form>` from that tree.
All **610** tracked `.fsl` files under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/`, plus **299** inline IDs emitted by the actual
`issue_1463_forms`, aggregate/component and nested/sync helpers (with their
resolver companions), were checked: **909 IDs, zero ok→error transitions**,
zero error→ok transitions and zero other full check-output differences.
No JSON field, diagnostic, location, exit code or stderr was excluded.
B binary SHA256: `b9aadd8379c9f93e8ad17bc53d7b8d14d32e504e577ba0dda9099b1f745cb526`;
H: `7eef478bc4c514d97922b1ee3ac9e570b4f896ace993969407fc0a6639e6f6c6`.
The independent 452-source/five-route matrix was also rerun twice on H;
all expectations matched, with unchanged stable full outputs on the 1269
route cells already matching on B. Six isolated adapter/arithmetic faults were
measured RED twice and exactly restored GREEN twice on this code H, not
transferred from the superseded pre-state-shadow candidate. Broad integration
gates remain separate from this finite source handoff.

## Pure integer-range planning API

The additive Rust-only surface is the public `fsl_core::binder_range`
module and inherent `KernelModel::plan_integer_range(lo, hi, lexical)` method;
there are no root re-exports of the new types or changes to existing public
`TypeRef`, symbolic values or binding meanings. The old failed-fold helpers and
recursive error precedence above are unchanged.

`InclusiveI64` has private endpoints and a validating constructor. Its checked
u128 cardinality describes the envelope, including 2^64 for MIN..MAX, without
enumerating values or imposing an execution budget. `IntegerEnvelopeFact` is
separate from a value's semantic carrier. Lexical presence uses the last input
entry and its original index, ahead of current state and then constants; an
absent lexical fact cannot fall back to a same-spelled global value.

Plans retain first-occurrence ordered, deduplicated state/input prerequisites.
Those are eligibility, never membership or an assertion of validity. Conditional
empty/singleton plans retain their premises and actual endpoint-status obligations.
Checked interval negation/addition/subtraction/all multiplication corners only
describe containment; abstract overflow is not an empty domain or runtime failure.
Eligible constant subtrees delegate to the existing integer constant owner.
Unknown classifications do not adopt new admission or execution diagnostics.

The pure planner uses a same-model, non-enumerating full authored-validation entry.
It shares type inference, compatibility and recursive authored rules with the
existing validator; only its local phase disables generated aggregate expansion.
All old entries remain normalized delegates, including their generated-copy
checks. No normalizer/candidate algorithm or caller policy changes.
Fresh frozen coverage includes 62 pure plans and five constructor cases, with
raw aggregate AST assertions before calls: static count folding fails through
the constant owner, dynamic count is unsupported at its constructor, and an
ill-typed inactive aggregate filter is rejected by full authored validation.
Safe validation-omission calibration distinguishes that refusal from constant
fold failure without enabling normalization. No wide aggregate was executed
through the old normalizing entry, and no OOM experiment is a detector.

## Measured empty boundary-transition set for the pure API

Own-tree B `ffcb51b05ce63669e496ffb9df66693b64c7af85` and H
`5771539b02649aee78b848fcabd2c696e7037278` rebuilt native CLIs with SHA256
`e35e13acef264f6d09479a7651c2590cac8b4d86de4979700474512b5dddb8a6` and
`f01567d7c5961ba08491c9d9d11e0dfceaef2106a1a1c6727cbe72de4174f78a`.
Each ran `FSLC_CACHE=off <own CLI> check <tree-relative source>` with
`FSLC_UNSTABLE` unset and with `FSLC_UNSTABLE=fsl5`, twice per side/mode.
The fresh roster was **610 corpus +2 actual BASE +299 freshly emitted helper
IDs =911 IDs**, yielding **7288 observations**, not 911 unique programs.
Measured sets are **EMPTY**: ok→error, all accepted→error, error→accepted,
old-error drift, other full-output drift and same-tree repeat drift. Complete
stdout/stderr/exit/packet comparisons excluded no fields.

The separately inventoried **112 endpoint-literal/AST occurrences and five
constructor cases are not complete model roots**; huge aggregate endpoints were
never wrapped, projected or sent through the old normalizer. This measured the
pure API alone, before any caller used it.

## Authored aggregate admission: Expanded versus Deferred (#1480)

Authored validation and value-producing normalization are separate. After the
existing bound-name/type, Boolean filter and integer sum-value checks succeed,
validation uses either `Expanded` (the existing unconditional expansion and its
generated-copy checks) or validation-only `Deferred`. A state carrier or lexical
type alone is not an established execution premise. A range requiring such a
premise, or otherwise lacking an unconditional finite plan, retains its original
aggregate AST rather than becoming a clipped sum or a guarded zero. Deferral does
not itself reject source. The later
[checked range admission](DESIGN-check-executable-forms.md#1480-checked-range-admission)
refuses a deferred bound the engines cannot execute, outside bare Core construction.
No public Deferred node or Kernel/JSON schema is added.

Deferral is not catch-all error suppression: both written endpoints are checked
in lower/upper order. A genuinely closed endpoint whose checked fold fails, such
as `0..(1/0)`, retains `public Kernel requires static aggregate range bounds`.
Authored scope/type errors keep their existing priority, including in empty
domains and inactive filters. Bounds use the enclosing scope; the filter and
sum value use the new binder scope. After a deferred aggregate's full authored
typing pass succeeds, a non-expanding bounds-only pass still checks nested
aggregates in its filter/value and later siblings for invalid closed endpoints;
an earlier deferred bound does not skip a later closed failure.

Ordinary sum-value arithmetic is not endpoint folding. The well-typed empty
`sum(i in 1..0 of (1/0)) == 0` remains accepted: its value is unreached. In contrast,
`sum(i in 1..0 of true)` remains a type error. Deferred nested dynamic aggregates
may validate without acquiring a runtime premise or a value-producing expansion.
The stored authored AST remains unchanged even for unconditional aggregates;
`Expanded` describes validation work, not replacement of the model's source AST.

Strict public Kernel/static projection refusal remains separate. Symbolic
consumers must establish eligibility for the exact active/read/capture frame or
explicitly refuse; eligibility is never candidate membership. Direct concrete
evaluation keeps actual endpoints, including out-of-carrier supplied states:
declared `n:0..2`, supplied `n=3`, `count(i in 0..n)` is 4, not 0 or 3; declared
`n:1..2`, supplied `n=0`, `count(i in n..0)` is 1, not 0. These literal contracts
do not infer symbolic eligibility from a Monitor result or from source admission.

Executable anchors are `core_authored_aggregate_validation.rs`,
`issue_1480_nested_deferred_bounds.rs`, `concrete_aggregate_values.rs`, and the
admission and public-API controls under `rust/fsl-verifier/tests/`. The numbers
above are bound to the commits they name.
