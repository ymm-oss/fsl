Changed (#1464): the unknown authority tool regression walk also runs
`approval create --kind html`, which panicked at the AI lowering lookup
before the #1464 fix. Command behavior is unchanged.
