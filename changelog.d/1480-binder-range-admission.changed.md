Breaking check-admission removal: `check`, every `verify` engine, LSP and Worker now
refuse every integer range bound the symbolic engines cannot execute, with one
rule (`RangePlan::is_executable`) and a located `semantics` error, exit 2. Refused:
unplanned dependent bounds (index, field, `size()`, `if`, `min`, `/`, `%`, `old`,
aggregates); state-dependent bounds in `ensures`, `reachable`, `terminal`, `until`,
leadsTo P/Q, `decreases` and `helpful` arguments; bounds reading a `let` (even
literal), pattern binding, Int state, non-literal Seq element, several-item Set
literal element, or element of a state Set that can start non-empty outside
`trans`, `unless`, `ensures` and init statements; and intervals that do not fit `i64`. Forms that reported a verdict before reaching the
bound lose it: an earlier violated property, `--property`/`--exclude-property`,
`--depth 0`, and explicit-engine verdicts on these forms. No size cap is added.
Bare `build_model`, raw Runtime constructors and `replay` keep their boundaries.
[#1480 final sweep: the measured base/head form count and `ok`→`error` list are recorded here after the final sweep.]
See `DESIGN-check-executable-forms.md`.

Reject two previously check-admitted forms before engine dispatch: init statement
`forall` integer-range bounds with free state reads, and expression/action typed
Named Struct binder domains that engines already refuse as non-scalar. Diagnostics
retain
owning source locations. Supported init RHS reads, Bool/Enum/Range binders and
ordinary/collection Struct values are unchanged.

Checked-source range admission runs after existing init ownership/read checks,
preserving their earlier diagnostics and component provenance. Bare Core model
construction and raw Runtime constructors keep their separate contracts.
Already-refused init-statement Typed-Struct domains retain their complete
unlocated finite-scalar error; they are not new check-admission removals.

This is a breaking check-admission removal. The required named-base/head sweep,
actual form count and complete `ok`→`error` list remain pending; no empty removal
set is claimed. See `DESIGN-check-executable-forms.md` and `DESIGN-spec-domains.md`.

Static binder ranges now share successful checked constant folding across
aggregate normalization, symbolic verification and static leadsTo/Monitor
bindings, including Euclidean division/remainder, min/max/abs and typed
conditionals with selected-arm arithmetic. The additive Rust API
`KernelModel::constant_integer` reads the current constant map without caching.
Free reads of current model state are nonconstant even for same-spelled consts;
the new folds do not substitute a const for a state read or add a collision rule.
Failed folds preserve each consumer's old recursive diagnostic/refusal path;
this adds no invalid-static admission policy, dynamic/outer/struct support,
runtime rewrite or liveness assurance promotion. The historical static-range
source/table stay intact; only its CURRENT expected symbolic outcomes become safe.
Own-tree `FSLC_CACHE=off <own-binary> check <tree-relative-form>` comparison of
B `d11f32631f75afa64fa486cdf4f9b46c6e77d28e` and code H
`c86f6bf4422b507f57862f830ad13726f8ce7734`: 610 corpus +299 actual inline IDs,
zero ok→error transitions or other full check-output changes.

Changed (#1480): builtin `Bool` binders retain their finite `false`/`true`
identity in symbolic/concrete quantifiers, statement-forall coverage and static
`leadsTo` wrappers. Breaking admission removal: negative constant `within`
deadlines now fail checked-model validation at the owning property before engine
dispatch. The four observed removals are shared `X1/negative-within` (`-1`, 8:3)
and native deadline fixtures `-1`, named `K = -1`, and `(0 - 1)` (each 5:3).
Zero/positive deadlines and all 610 corpus check dispositions are preserved;
the full comparison is recorded in `DESIGN-spec-domains.md`. Dynamic/struct and
per-position arithmetic policy remain open; property-keyed JSON and bounded
unranked liveness assurance are unchanged.

Introduce a candidate Rust-only compact integer-range planning API under
`fsl_core::binder_range` and `KernelModel::plan_integer_range`, without caller
wiring, runtime changes or new admission policy. Plans preserve contextual name
provenance and prerequisite ownership; full i64 envelope cardinality uses u128
without candidate enumeration or an execution cap. A planner-only non-enumerating
full authored-validation entry preserves normalized behavior for old callers.
Independent pure/aggregate boundary controls and safe semantic fault calibration
cover the new API; broader caller execution and full #1480 remain separate work.

G5 own-tree B `ffcb51b05ce63669e496ffb9df66693b64c7af85` /H
`5771539b02649aee78b848fcabd2c696e7037278`, rebuilt CLI hashes `e35e13ac…`
/`f01567d7…`: `FSLC_CACHE=off <own CLI> check <tree-relative source>`,
both internal configurations, twice each side/configuration. Fresh610 corpus +2 BASE
+299 helper IDs =911 IDs /7288 observations; measured ok→error, all accepted→error,
error→accepted, old-error/other/repeat drift sets EMPTY, full outputs/no exclusions.
Separate112 endpoint AST/literal occurrences and five constructors are not model
roots or huge-normalization projections. Source A finite only; full B/C/D/plain
`0..n`/public-verifier/combined-policy/Bool/product obligations remain OPEN.

Authored aggregate validation (#1480 U5) now distinguishes unconditional
Expanded validation from validation-only Deferred after the existing written
bound/filter/sum typing checks. Deferred preserves the original AST instead of
carrier-clipped expansion, guarded zero or a new check rejection. Both endpoints
and nested deferred filter/value bounds still reject genuinely closed failed
folds in order, preserving authored scope/type-error priority. Empty-domain
unreached sum-value arithmetic remains accepted; ill-typed values do not.
Strict Kernel/static projection and symbolic same-frame eligibility/refusal
remain separate, and concrete evaluation keeps actual endpoints. No public
Deferred node/schema or general symbolic execution support is added. The
historical measurements above remain bound to their named source/producer;
fresh integration boundary comparisons, qualification/calibration, independent
review, full B/C/D/original500 tenRED and product obligations remain open.

Unfiltered outer `leadsTo` range binders can use already bound wrapper locals
with checked integer arithmetic and typed Boolean conditions. Preserve declared
binding types, expansion order and legacy diagnostics; refuse free state bounds
even when a const shares the name. `within` remains a nonnegative declaration
constant; collection/filtered wrappers and explicit liveness remain unsupported.
