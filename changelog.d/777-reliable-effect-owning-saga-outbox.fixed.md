Fixed (#723): `fslc domain check`/`analyze`'s `reliable_effect_without_outbox_boundary`
finding now honors `DESIGN-effect.md`'s accepted "outbox on the effect *or
owning saga*" contract instead of only the effect's own `outbox`. A saga owns
an effect when one of its steps or `compensation` blocks emits the effect's
request event; an unrelated saga's outbox boundary no longer silences the
finding, and a partially-covered set of owning sagas still fires with the
uncovered saga names recorded in the finding's `witness.uncovered_sagas`.
