Documented (#1908): a spike under `rust/spikes/dafny-core/` runs steps 1–4 of
#1908 end to end: `fslc kernel` to Dafny, proof obligations whose lemmas a model
writes and Dafny decides for every size of the widened types, the proved code
compiled to a typed TypeScript core, and that core checked against fslc
(`conformance`, `testgen`, `replay` and planted-state `verify`). It is a
prototype outside the product: no Rust code, CLI, Kernel contract or `specs/`
change, and it is not wired into CI.
