Fixed (#1023)!: `fslc verify`'s cache key is now built from the dependencies
actually read while resolving `use`/`from`/inline `implements` (including
across a symlinked alias), not from walking the checked spec's parent
directory. A `from "../x.fsl"` dependency outside that directory used to be
invisible to the key, so a stale `verified`/exit 0 cache entry kept being
served after the dependency started violating the refinement contract;
editing an unrelated sibling `.fsl` file that nothing depended on used to
invalidate the entry too. Both are fixed: the key's dependency domain now
matches what the verdict actually depends on. Because a cache hit now has to
read those dependencies to know its key is still valid, a spec whose
dependency has since become unreadable now fails closed (`error`, exit 2) on
what used to be a cache hit, instead of replaying the stale pre-removal
verdict. The on-disk cache generation moved from `verify/v2` to `verify/v3`
(orphaning any existing `verify/v2` entries on disk; nothing removes them).
