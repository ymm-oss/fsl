Documented (#747): Recorded recovery observations after #794: on run `31565897267` attempt 1, a cancelled
Windows native-Z3 job's post step saved 619,429,238 B, although the same changeset also raised its
timeout from 40 to 60 minutes; runs `31581715093` attempt 1 and `31583381471` attempt 1 logged four
merge-readiness full matches; the Windows entry's 2026-08-12T06:16:19Z recreation resolved
`main-cache-absent`, and removing two human-authorized orphaned #793 caches resolved the remaining audit
findings before run `31654305398` attempt 1 succeeded at 7.337 GiB. The direct scheduled recovery
record, run `31632094255` attempt 1 (`event: schedule`), has both `native Z3 4.16 (windows-latest)`
and `product gate` concluding `success`. The restore-only FSL Logic Test logged shared-key full
matches on run `31565897267` attempt 1 (2m54s) and on run `31570480618` attempt 1 (3m02s). Also recorded that the current
`save-if: false` guard, rather than historical timing, makes operators the only configured
`semantic-mutation` saver: run `31086907528` attempt 1's mutants job `92568586155` uploaded
2,922,378,363 B after `No cache found.` while all operator posts were skipped.
