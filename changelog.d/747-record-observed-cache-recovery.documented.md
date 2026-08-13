Documented (#747): Recorded recovery observations after #794: on run `31565897267`, a cancelled
Windows native-Z3 job's post step saved 619,429,238 B, although the same changeset also raised its
timeout from 40 to 60 minutes; runs `31581715093` and `31583381471` logged four merge-readiness full
matches; and removing two human-authorized orphaned #793 caches left 7.337 GiB and made audit run
`31654305398` succeed. The restore-only FSL Logic Test logged a full cache match before completing in
3m02s on run `31570480618`.
