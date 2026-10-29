Changed (#1573): breaking removal of pattern-name reuse after matches inside
`not`, expression-if conditions/arms, where-to-body/of, and statement-if
conditions or arms flowing into later statements. Rename the later binding
and its reads; visibility is not broadened, opposite statement arms remain
independent, and candidate-local names do not escape. Expression then-to-else
reuse is also rejected while the symbolic expression evaluator shares bindings
(#1577). Compose now gives independent sources distinct hygienic pattern names:
the guard-output examples change `v` to `__v_substitution_0/1`, preserving their
explain result and verification verdicts. Two-source statement payloads no
longer produce the false violated verdict caused by one source's value being
used by another; check stays accepted and the correct verdict is holds, while
a genuinely false control remains violated. Evaluator lifetime work remains
tracked in #1622.
