Changed (#1459, breaking): shared static checking now rejects non-Option
`is none`/`is some` operands; non-integer `abs` arguments, including enum
domain/range elements; non-relation operands of acyclic, functional, injective,
domain, range and reachable; and non-self path/cycle relations. Reachable and
acyclic require exactly the same declared endpoint type, T -> T: mixed
enum/numeric relations and N -> M are rejected even if numeric N and M have
identical bounds. Numeric compatibility does not establish self-relation identity.
Non-relations no longer infer a fallback Set<Int> projection.

Collection method receivers, names and arities must match the runtime:
Set contains/add/remove take one item and size takes none; Seq contains/push/at
take one argument and pop/head/size take none; relation contains/add/remove
take exactly two endpoints. Map has no methods. Invalid examples now rejected
include n.contains(0), Set.push, Seq.add, relation.size, s.contains(), q.pop(1)
and action assignment n = q.at().

Count/sum check original binders, Bool filters and integer values before
expansion, even over empty ranges or zero-capacity sequences: count with
`where 1`/`where not 1` and sum with `of true`/`of -Red` no longer disappear
into a zero result. Well-typed empty aggregates still return zero.

These checks cover dead branches, action assignments/requires/ensures and
trace expressions, reporting authored locations as semantics errors, exit 2,
before execution or replay. Valid Option bindings, integer abs, supported
methods, heterogeneous non-path relation operations, endpoint hints and
non-reflexive reachability remain unchanged. The former abs exception in the
#1458 projection records is removed. Native LSP and Worker parity tests cover
the new Option operand error.

Boundary audit: BASE e8cf78f0, HEAD 33d7faa7; 591 corpus files plus 384 inline
occurrences (975 forms), stable/fsl5 twice in three independently built trees
(baseline, immediate parent, head), 11,700 checks. The corpus ok-to-error set is
empty, with no other corpus output drift and no excluded forms or fields.
New #1459 removals total 248 mode-forms (124 occurrences/123 distinct sources)
across all six categories above, including numeric N -> M. Another 60 mode-forms
remain error/semantics/2 but change from unlocated runtime failures to authored
static diagnostics. Inherited #1439 (86 removals/11 reclassifications) and #1458
(56 removals/6 additions) are accounted separately in the expanded inventory,
not counted as new #1459 behavior. The design amendment records full evidence.

Review round 1 also rejects non-self selections hidden behind conditional
then-branch inference, through nesting, old, relation add/remove receivers and
action let aliases. Projection equality preserves substantive relation errors
over context-needed empty literals and retains authored locations; range bounds
and terminal no longer report generated 1:1 origins for authored input. Existing
numeric/Set compatibility and valid dynamic verdicts are unchanged. The checked
positions do not include parameter-type validation of helpful arguments, and
this does not add relation-valued temporary collection support; those gaps are
separate upstream work.

Round-1 delta BASE 33d7faa7 to HEAD 8caf5c2e: 42 additional mode-form removals
(21 source occurrences) and 24 diagnostic reclassifications (12 occurrences:
projection inference 16, range-bound origins six, terminal origins two). The
diagnostic changes retain error/semantics/2. The expanded 591-corpus/447-inline
inventory (1,038 forms) runs both modes twice in four own-built trees, 16,608
checks including the separate previous-fix baseline. All 8,304 repeat pairs
agree in full, with no exclusions or corpus output drift. Inherited/R0 changes
are accounted separately, not attributed to round 1.
