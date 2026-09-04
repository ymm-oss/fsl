Fixed (#946): the Codex Cargo lock wrapper no longer waits for the lock its own
parent holds. The lock-owning wrapper marks its child with
`FSL_CARGO_LOCK_HELD=<absolute path of the lock file>` -- in the environment and
again inside the shell, after the login profile runs, specifically so that a
login profile which unsets the marker cannot reintroduce the self-deadlock: the
re-export inside the shell restores it before the command runs, so a nested call
still bypasses acquisition instead of waiting on its own parent. Acquisition is
bypassed only when the marker names *this* lock and the lock is still held. A
marker from another repository falls back to ordinary serialization, because it
never matches this lock's path. A marker left behind after the owner released
the lock is stale: the next call finds the lock free, takes it, and becomes the
new holder like any ordinary invocation. A `flock` failure that is not
contention is still reported at once rather than waited out. The outer
invocation keeps the Git common-directory lock for the whole command, so
serialization across worktrees is unchanged. Known limitation: a descendant
that outlives its own wrapper can still bypass while an *unrelated* invocation
holds the same lock; closing that needs owner identity, which is not attempted
here.
