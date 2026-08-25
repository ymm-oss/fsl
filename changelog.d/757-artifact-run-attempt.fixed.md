Fixed (#757): sharded Rust and semantic-mutation artifacts now use stable logical names so a failed
shard can be rerun without discarding successful peers. Provenance, checksums, shard identity, and
exact-union controls reject incomplete or incompatible mixed-attempt cohorts.
