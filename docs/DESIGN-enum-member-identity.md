# Checked-Kernel enum-member identity and lookup

Status: accepted design decision for issue #454.

## Decision

FSL keeps bare enum-member source syntax, but an unshadowed bare member in a checked context
must resolve to exactly one nominal enum. When implementation and abstraction models are combined
for refinement and two enum types publish the same spelling, that spelling is ambiguous and the
frontend rejects its use with a located type error. Existing mapping identifier precedence is
unchanged: implementation state and constant names plus mapping parameters and binders shadow enum
members. Abstraction constants are not mapping inputs and are not imported into the checked
expression environment. A refinement can cross an enum boundary only through a construct that has
already established the source and target nominal types. `enum conversion` covers bijections, while
`enum abstraction` covers source-total many-to-one mappings; both elaborate to checked
`Expr::EnumMember` expressions.

This is a staged **go** for nominal lookup at the checked refinement boundary and a **no-go**
for replacing `KernelModel.enum_members` with a tuple-keyed public field in the current Kernel
contract. The flat map remains a compatibility index for unambiguous bare syntax inside one
checked model. `KernelModel.types` is the canonical inventory for nominal membership, and
`Expr::EnumMember { type_name, member }` is the canonical checked expression when identity
must survive a merge.

## Context and local rationale

The flat `BTreeMap<String, Value>` is locally useful. It makes legacy bare expressions cheap to
type and evaluate, preserves the accepted source grammar, feeds duplicate-write detection, and
supports document/CLI catalogs without adding a second surface spelling. Model construction also
rejects duplicate member spellings inside one Kernel model, so the index is unambiguous there.

The boundary changes when refinement combines two independently valid models. Before this
decision, `typecheck::base_env` derived a first-wins owner from the merged type inventory, while
runtime and symbolic refinement helpers separately merged flat value maps. Their insertion order
could choose a different owner. The local convenience therefore externalized nominal identity to
each consumer and could turn a checked bare member into a different enum value at evaluation time.

## Consumer and compatibility inventory

| Consumer | Existing purpose | Decision |
|---|---|---|
| `fsl-core::model::ModelBuilder` | Reject duplicate bare members in one model and build the legacy lookup index | Retain; one-model bare syntax stays compatible. |
| `fsl-core::typecheck::base_env` | Assign types to bare members | Publish only members with one owner in the checked type inventory; diagnose multiple owners. |
| `fsl-core::refinement::refinement_type_context` | Combine impl/abs types for mapping checks | Keep both nominal type definitions, but only impl constants; ambiguity is detected from the complete type inventory without admitting unevaluable abs constants. |
| `fsl-runtime::eval` and `fsl-verifier::eval` | Evaluate ordinary bare variables | Retain for unambiguous checked expressions; typed `Expr::EnumMember` bypasses the flat map. |
| runtime/verifier refinement model assembly | Combine impl/abs models | May retain the flat compatibility index because an ambiguous bare expression can no longer pass the shared frontend. Typed literals resolve through `types`. |
| duplicate-write detection | Evaluate static indices while building one model | Retain the flat one-model index. |
| Public Kernel v1/v2 projection | Publish checked expressions | No schema change: a typed member remains `kind:"var"` plus its named type. Importers must use the `(type, name)` pair. |
| document renderer and glossary | Recognize/display members | Continue using the one-model index; glossary targets already carry `Type.Member` and validate the nominal owner. |
| CLI catalog and raw mapping evaluator | Completion/catalog or untyped external mappings | Continue exposing unambiguous model members. Raw production/causal replay rejects typed enum conversions and abstractions because no typed implementation model exists. |
| frozen Python reference | Compatibility reference | No change; this decision concerns the authoritative native Rust checked boundary. |

Bare-member source behavior is unchanged for every model that was valid by itself. The only newly
rejected case is a refinement expression whose combined impl/abs inventory gives an unshadowed
referenced bare spelling more than one nominal owner. Implementation state and constant entries are
installed before enum members, and expression-local parameter/binder bindings are installed
afterward; those ordinary identifiers therefore retain their established precedence. Abstraction
constants are excluded because §1 mapping expressions range over implementation state and constants,
and neither concrete nor symbolic evaluation supplies abstraction constants. The rejected enum
expression had no stable interpretation across the checked and evaluation boundaries. Public Kernel
v1/v2, JSON envelopes, exit codes, and replay schemas are unchanged.

## Negative control and positive path

`rust/fsl-core/tests/enum_conversion.rs` constructs independently valid `AImplStage.Shared` and
`ZAbsStage.Shared` enums. Their names deliberately make the implementation owner win the former
first-wins type environment while the abstraction flat value would overwrite it at runtime. The
test asserts that a bare `Shared` in a refinement conditional is rejected and names both owners.
This is the required wrong-nominal negative control: an expression that the old typecheck order
accepted cannot reach the disagreeing runtime merge. A second negative control prevents an
abstraction constant from masking the collision during checking when that constant is unavailable
to evaluation. Adjacent positive controls preserve implementation state/constant and local-binder
shadowing.

The adjacent enum-mapping tests are the positive paths. They check that same-spelled members
are elaborated as both `ImplStage.Shared`-style and `AbsStage.Shared`-style typed literals and that
the existing Public Kernel expression shape retains both named types. Runtime and verifier
agreement tests for `enum conversion` and `enum abstraction` are the execution-level anchors.

## Alternatives and migration

### A. Keep first-wins/overwrite flat lookup

This has no migration cost but leaves ownership dependent on insertion order. It is rejected at the
refinement boundary because typechecking and evaluation have separate merge implementations.

### B. Reject ambiguous bare members and use typed checked literals

This is the selected state. It is a small, reversible compatibility tightening, requires no schema
version, and centralizes the safety decision in the shared frontend. Authors migrate an ambiguous
conditional to checked `enum conversion` plus `convert` for a bijection, or to source-total
`enum abstraction` plus `abstract` for a many-to-one mapping, rather than renaming enums or relying
on declaration order.

### C. Replace the Kernel field with `(type_name, member)` keys

This may become useful if FSL accepts general qualified member syntax or publishes a versioned
nominal-literal Kernel node. Doing it now would require coordinated changes to ordinary evaluation,
duplicate-write analysis, documents, CLI catalogs, downstream Rust consumers, and compatibility
policy while still needing a separate rule for bare syntax. The migration cost is not justified by
additional safety after option B, so this issue does not authorize it.

## Future gate for canonical storage migration

Reconsider option C only with all of the following evidence:

- an accepted source or Kernel contract for qualified enum-member lookup outside refinement;
- an inventory of downstream users of the public `KernelModel` field;
- byte/schema compatibility or an explicit Public Kernel version transition;
- positive and rejecting concrete/symbolic/replay agreement controls; and
- a measured reduction in consumer compensation beyond the ambiguity rejection established here.
