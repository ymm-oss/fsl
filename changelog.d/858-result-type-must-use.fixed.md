Fixed (#858): the public types that carry a verification outcome are now `#[must_use]` on the
type rather than only on the functions returning them. `Result` is already
`#[must_use]`, so `monitor.step(x).map_err(..)?;` satisfied the compiler while the
inner `StepResult` -- which owns `violation` -- was dropped as a statement value;
annotating the function is redundant against `Result` and closed nothing. With the
attribute on the type, `cargo clippy -- -D warnings` rejects the discard **where
the outcome is returned directly**, which is the shape behind the `testgen` walk
bug. It does not reach an outcome wrapped in `Option`: `#[must_use]` does not
propagate through `Option`, and `Option` is not itself `#[must_use]`, so a
`-> Result<Option<Outcome>, E>` signature stays undetectable by this mechanism
(issue #868).
