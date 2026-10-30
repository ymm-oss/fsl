Changed (#1472, breaking removal): whole Set, Seq, Map and relation values
require matching carriers, including Seq capacity and both Map keys and values.
This resolves #1627: narrow-to-wide as well as wide-to-narrow whole Set copies
now fail check with the existing located assignability error (exit 2), rather
than disagreeing across engines. Equal-range aliases remain compatible through
symbolic retagging; scalar/Option numeric compatibility and scalar arguments
are unchanged. Refinement resolves each model's numeric types independently and
retains its existing type error. Upstream #1642 recovers authored map spans, with
unknown 1:1 retained only without an authored span. Migrated unchanged-source
anchors are runtime1:40/1:45 map tokens, standalone4:3 map and inline2:3 implements,
formerly1:1. Same-name struct-field and enum/range collisions remain open pending
#1599's shared merge rule and Type-kind handoff. Int is outside the finite-carrier gate;
existing literal compatibility and inferred Seq capacity are preserved. Static
implements-map type failures in mutate now use the shared check-invalid path:
whole-container range mutants move from errored to invalid, outside kill_rate
and without oracle_errors in the gate. Solver/evaluation errors and budget
cutoffs retain their existing classifications. Independent archive builds of
79687655 and 3c99c93b twice checked 606 corpus and 16 original inline
forms: zero corpus removals, zero error-to-error diagnostic changes; three inline
ok-to-error removals are FALSE_VACUITY/CondMap (Map keys 0..0 vs 0..2, 20:41),
the #1522 MAP conditional (keys 0..2 vs 0..1, 7:35), and SET_WIDENING
(Set 0..2 to 0..4, 6:48). Also removed are admitted whole Seq maps in runtime
SeqWide to SeqAbs, SeqWider to SeqGuarded, CLI SEQ_WIDE_IMPL to SEQ_ABS and
inline seq_req(-1): capacity 2, model-local elements -1..2 or 0..3 vs 0..2;
former refinement failures now reject statically. Another 18 literal-boundary
forms retain identical check outputs. Set/Seq conditional inference keeps an
already known finite carrier/capacity across let, without literal materialization.
Six additional breaking removals, measured with 18 bridge forms, are both arm
orders of Set0..2→0..4, Seq0..2→0..4 at capacity2 and Seq capacity2→3 with equal
elements (all8:3). Their standalone/same-carrier controls still check; nine
total ok-to-error forms were measured. Literal materialization remains #1467.
