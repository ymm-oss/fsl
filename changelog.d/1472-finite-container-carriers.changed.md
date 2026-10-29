Changed (#1472, breaking removal): whole Set, Seq, Map and relation values
require matching carriers, including Seq capacity and both Map keys and values.
This resolves #1627: narrow-to-wide as well as wide-to-narrow whole Set copies
now fail check with the existing located assignability error (exit 2), rather
than disagreeing across engines. Equal-range aliases remain compatible through
symbolic retagging; scalar/Option numeric compatibility and scalar arguments
are unchanged. Refinement resolves each model's types independently and retains
its existing fixed 1:1 error location. Int is outside the finite-carrier gate;
existing literal compatibility and inferred Seq capacity are preserved. Static
implements-map type failures in mutate now use the shared check-invalid path:
whole-container range mutants move from errored to invalid, outside kill_rate
and without oracle_errors in the gate. Solver/evaluation errors and budget
cutoffs retain their existing classifications. Independent archive builds of 5536ebc1 and
implementation ancestor fbaab589 twice checked 603 corpus and 16 original inline
forms: zero corpus removals, zero error-to-error diagnostic changes; three inline
ok-to-error removals are FALSE_VACUITY/CondMap (Map keys 0..0 vs 0..2, 20:41),
the #1522 MAP conditional (keys 0..2 vs 0..1, 7:35), and SET_WIDENING
(Set 0..2 to 0..4, 6:48). Also removed are admitted whole Seq maps in runtime
SeqWide to SeqAbs, SeqWider to SeqGuarded, CLI SEQ_WIDE_IMPL to SEQ_ABS and
inline seq_req(-1): capacity 2, model-local elements -1..2 or 0..3 vs 0..2;
former refinement failures now reject statically. Another 18 literal-boundary
forms retain identical check outputs. Literal typing remains #1467.
