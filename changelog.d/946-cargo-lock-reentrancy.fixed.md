Fixed (#946): the Codex Cargo lock wrapper no longer waits for the lock its own
parent holds. The lock-owning wrapper marks its child with
`FSL_CARGO_LOCK_HELD=<absolute path of the lock file>` -- in the environment and
again inside the shell, after the login profile runs -- and a nested call
bypasses acquisition only when that marker names *this* lock and the lock is
still held. A marker from another repository, a marker left behind after the
owner released the lock, and a profile that unsets the marker all fall back to
ordinary serialization instead of the 3600-second self-deadlock, and a `flock`
failure that is not contention is still reported at once rather than waited out.
The outer invocation keeps the Git common-directory lock for the whole command,
so serialization across worktrees is unchanged. Known limitation: a descendant
that outlives its own wrapper can still bypass while an *unrelated* invocation
holds the same lock; closing that needs owner identity, which is not attempted
here.
