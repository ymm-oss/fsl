Decided (#1612): Adopt the main merge queue with squash merging and zero required
approvals so a single maintainer can enqueue. Record the live queue rule in the
CI contract and fixture, retain full pull-request evidence with
`FSL_MERGE_QUEUE_CI` unset, reserve any remaining administrator queue bypass for
emergencies, and record merge-group cache saves as a known open risk (#1615).
