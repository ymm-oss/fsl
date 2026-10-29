Changed (#1573): breaking removal of pattern-name reuse after matches inside
`not`, expression-if conditions/arms, where-to-body/of, and statement-if
conditions or arms flowing into later statements. Rename the later binding
and its reads; the four standalone/compose #1442 nested-if reuse routes are
included, with original refusal inputs retained and only the inner `u` renamed
to `w` for accepted preservation controls. Visibility is not broadened, opposite
statement arms remain independent, and candidate-local names do not escape.
The statement rules also apply to ordinary init checking, preserving its
initializer/const/map-key diagnostics. Expression then-to-else
reuse is also rejected while the symbolic expression evaluator shares bindings
(#1577). For sync actions and non-init paths, compose gives independent sources
distinct hygienic pattern names:
the guard-output examples change `v` to `__v_substitution_0/1`, preserving their
explain result and verification verdicts. Two-source statement payloads no
longer produce the false violated verdict caused by one source's value being
used by another; check stays accepted and the correct verdict is holds, while
a genuinely false control remains violated. Evaluator lifetime work remains
tracked in #1622, including the still-unrepaired separation of component init
patterns; the general init-source separation guarantee is not made here.
