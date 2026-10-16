Changed (#1263): a `proved` result from `fslc verify --engine induction` now
says what the proof does not cover. The step reasons over unbounded integers
and only the base case (BMC to `--depth`) checks i64 overflow, so a counter
that overflows beyond `--depth` stays `proved`; the envelope now carries
`not_covered: ["int_overflow"]` whenever an invariant, `trans`, `leadsTo`
trigger/goal/measure, `terminal`, guard, body or `ensures` uses an operator
that can overflow (`+ - * / %`, unary `-`, `abs`, `sum`). The test is
syntactic: arithmetic on small bounded values carries it too, and a proof
with no such operator omits the key (never an empty array). `--engine
explicit` checks overflow concretely and does not carry it. Additive output
only: no verdict, exit code or other field changes, and the envelope schema
documents the key. Migration: none required; a consumer that needs overflow
coverage should treat `int_overflow` in `not_covered` as "check with
`--engine bmc`/`explicit` at a depth that reaches it, or bound the value".
