Changed (#1600, breaking): in next-edition requirements, a relation role, a
resource name, and a measure name may no longer contain `__`, completing
#1256's rule for declared names. `fslc check` rejects
`Claim belongs to User as __owner via approve`, `resource __r: Amount = 0`,
and `measure __m = count Claim in Approved` with `kind: "name"`, exit 2,
located at the declaration, and the message
`relation role name '__owner' contains '__', which is reserved for the names FSL generates ...`
(or `resource name`, `measure name`). Before, the declared-name walk missed
these three, so they passed check. No spec or example in the repository
declares such a name. Migration: rename the role, resource, or measure so that
it does not contain `__` (for example `__owner` to `owner`), and update the
references to it.
