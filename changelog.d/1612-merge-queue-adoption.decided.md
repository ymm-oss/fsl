Decided (#1612): Adopt the main merge queue with squash merging and zero required
approvals so a single maintainer can enqueue. Record the live queue rule in the
CI contract and fixture, retain full pull-request evidence with
`FSL_MERGE_QUEUE_CI` unset, and reserve administrator queue bypass for emergencies.
