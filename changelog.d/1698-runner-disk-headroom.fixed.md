Fixed (#1698): `rust tests` shards ran out of disk while linking test binaries on some
hosted runners, which failed the merge queue. Each shard now removes the
preinstalled Android, .NET and GHCup toolchains before it builds, and prints free
space before and after the removal and at the end of the job. Tests, Cargo profiles
and cache keys are unchanged.
