Fixed (#821): the explicit-engine init duplicate-write check now rejects a `forall i { m[i]
= ... }` write that overlaps a later flat `m[K] = ...` write to the same concrete key `K`,
whether or not the two values agree. It previously tracked forall-indexed writes at
whole-variable granularity while flat writes were tracked per key, so an overlapping flat
write with an agreeing value was silently accepted and one with a conflicting value fell
through to the unrelated "init constraints are unsatisfiable" diagnostic instead of the
duplicate-write rule's own message.
