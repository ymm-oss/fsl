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
