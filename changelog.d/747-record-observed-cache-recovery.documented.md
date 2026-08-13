Documented (#747): Recorded recovery observations after #794: on run `31565897267`, a cancelled
Windows native-Z3 job's post step saved 619,429,238 B, although the same changeset also raised its
timeout from 40 to 60 minutes; runs `31581715093` and `31583381471` logged four merge-readiness full
matches; the Windows entry's 2026-08-12T06:16:19Z recreation resolved `main-cache-absent`, and
removing two human-authorized orphaned #793 caches resolved the remaining audit findings before run
`31654305398` succeeded at 7.337 GiB. The restore-only FSL Logic Test logged shared-key full matches
on run `31565897267` (2m54s) and on run `31570480618` (3m02s).
