# self specs — dogfooding fslc itself in FSL

This directory holds self-specs (meta-circular dogfooding). That is, the design
contracts of `fslc` itself are written as FSL state machines, making the verifier
a subject of the verifier itself.

## Cast

| File | Contract being modeled |
|---|---|
| `fslc_session.fsl` | CLI result classification and the ordering of exit-code severity. Only advance to a success result after a successful check; `proved` includes `verified`; an internal error is unrecoverable. |
| `fslc_monitor.fsl` | The reject-stickiness of the replay runtime. Once a log becomes nonconformant it does not revert, and it is conformant only when every step is ok. |
| `refinement_algebra.fsl` | Through refinement layers, safety propagates and liveness does not. A change that breaks safety is not a valid refinement link. |

## Run

```bash
E=examples/self

# fslc_session: declare intended terminal states such as ToolFault and each success result with terminal { }
./.venv/bin/python -m fslc check  $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl
./.venv/bin/python -m fslc verify $E/fslc_session.fsl --engine induction

# fslc_monitor: declare Conformant / Nonconformant with terminal { }
./.venv/bin/python -m fslc check  $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl
./.venv/bin/python -m fslc verify $E/fslc_monitor.fsl --engine induction

# refinement_algebra: there is no terminal state because reflexive_refine is always enabled
./.venv/bin/python -m fslc check  $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl
./.venv/bin/python -m fslc verify $E/refinement_algebra.fsl --engine induction
```

In every case, `check` is `ok`, ordinary `verify` is `verified`, and induction is
`proved`. Because `fslc_session` / `fslc_monitor` **declare their intended terminal
states with `terminal { }` blocks**, no deadlock warning is raised even with
`--deadlock warn` (the default). If there is a stop state **not included** in
terminal (an unexpected deadlock), it is warned as before. This addresses F23 of
DOGFOOD-11 (the lack of a means to declare intended stops).

The kill-rate of `fslc mutate` was used as a non-triviality (anti-ghost) indicator
to check whether an invariant leans on a dead ghost.

## Implementation-conformance anchors

`fslc_session.fsl` is a self-spec that models fslc's CLI result classification and
exit-code severity in FSL, but proving the model's internal consistency alone via
`verify` / induction does not guarantee **whether the implementation
(`src/fslc/cli.py`) actually honors that contract**.

`tests/test_self_conformance.py` fills that gap. For a set of specs that produce
diverse outcomes, it runs `check` → (if ok) `verify` → (if verified) `verify
--engine induction` against the real CLI and checks:

1. that each subcommand's `result` and the process exit code match the severity
   table of `exit_code()`,
2. that contracts such as `ProvedImpliesVerified` / `SuccessRequiresCheck` hold on
   the actual results,
3. that mapping the recorded `(subcommand, result)` sequence onto `fslc_session`'s
   action sequence makes `fslc replay` return `conformant` (the real CLI's
   transitions conform to the model state machine),
4. that a hand-written trace violating a contract becomes `nonconformant` (a
   negative control).

With this, meta-circular dogfooding is lifted from "model verification" to
"implementation-conformance verification."

### fslc_monitor anchor (replay runtime)

`fslc_monitor.fsl` models the contract that `Monitor` / `run_replay` in
`src/fslc/runtime.py` must honor (stop at the first reject, conformant only when
all are ok, no processing proceeds after a reject). The monitor section of
`tests/test_self_conformance.py` maps the observed result of a real `fslc replay`
against a guarded spec (`specs/cart_v1.fsl`) onto a `step_ok` / `step_reject` /
`finish` sequence, and checks that the replay against `fslc_monitor` becomes
`conformant`. It also includes a negative control where a contract-violating trace
(e.g., `step_ok` after a reject) becomes `nonconformant`.

### Expanding subcommand coverage

The previous anchor covered only the `check` → `verify` → `induction` pipeline. The
following were added.

| Real subcommand | Mapped action | Note |
|---|---|---|
| `verify` (semantics error) | `verify_user_error` | `no_actions.fsl` etc.: check ok but verify is a semantics error |
| `scenarios` | `scenarios_ok` | |
| `explain` | `explained_ok` | |
| `mutate` | `mutated_ok` | |
| `typestate` | `typestate_ok` | |
| `refine` (success) | `refines_ok` | |
| `refine` (failure) | `refine_failed` | |
| `replay` (conformant) | `replay_conformant` | |
| `replay` (nonconformant) | `replay_nonconformant` | |

`tool_fault` (exit 3, internal error) exists in the model, but because an internal
error cannot be deliberately induced, **the implementation anchor is not yet in
place**.
