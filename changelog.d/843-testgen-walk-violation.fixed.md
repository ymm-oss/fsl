Fixed (#843): `fslc testgen` no longer bakes a Monitor rollback as a conformance expectation. The
fixed-seed walk is a concrete Monitor run capped at 100 steps and independent of
`--depth`, so it could reach a violation the bounded verification `testgen` runs
first proved absent within `depth`; the violating `StepResult` was discarded and
the rolled-back (unchanged) state was recorded as that step's `expected`. A
conforming implementation failed the generated test, an implementation that
silently did nothing passed it, and `--target pytest` -- which drives the walk live
-- reached the opposite conclusion from the five baked targets on the same input.
The walk now fails closed, reporting the violation with the same
`result:"violated"` envelope, exit code, property, step, and replayable trace
`verify` reports, and writing no harness.
