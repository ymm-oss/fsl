Changed (#1412): a `mutation mutants` CI shard with zero selected mutants now
seeds its fresh Cargo target with third-party build output, including vendored
Z3, from the restored cache instead of compiling it again. Every first-party
crate still compiles in the shard: Cargo's per-artifact verdict is retained and
any reused first-party artifact fails the shard and the aggregate. Shards that
test a mutant, the same 35 detectors, and the local unsharded gate keep their
full build; an unusable seed falls back to a full fresh build, and the log names
the path taken.
