Fixed (#1258): `verify --engine bmc` reported `verified` and `--engine
induction` reported `proved` (exit 0) for a spec whose `init` reaches a
failing operation, e.g. `init { d = 0  x = 6 / d }` or a finite `Map` read
outside its key domain, while `--engine explicit` stops on the same spec with
`division by zero` (exit 2). The symbolic init encoding totalized those
operations, and a failing deterministic init also disabled the concrete
pre-pass; a nondeterministic init never had it. BMC (and so the induction
base case, every `verify_bounded` caller, and the browser Worker) now asks
after the search whether init is defined: a `/`/`%` by zero, a partial `Seq`
operation, a `Map` key outside its domain (read or assignment target), or
checked i64 overflow that init evaluation reaches is a `kind: "semantics"`
error, exit 2, whose message names the failure with the explicit engine's
wording and the init statement, e.g. `division by zero in init at 9:5`. It
supersedes the search's verdict. An operation on the unreached side of an
init `if`, conditional, or `and`/`or`/`=>` stays defined, and the step-0
type bounds the search asserts are assumed, as before. An init overflow that
used to surface as `integer model value is unavailable` now reads e.g.
`integer overflow in multiplication in init at 9:5`.

Migration: a spec that was `verified`/`proved` only because of this defect
now exits 2; guard the init operation with an init `if` (or fix the initial
value). No spec under `specs/`, `examples/`, or the test fixtures changes
verdict, exit code, or message. When init contains an operation that can fail,
`cost.properties` gains one `{"kind": "init", "name": "definedness"}` entry
(and `cost.solver.checks` counts it); an init with nothing that can fail builds
no solver term and asks nothing, so its output is byte-identical.
`fslc kernel`'s `partial_operations` and `fslc explain`'s `auto_checks` still
list action sites only.
