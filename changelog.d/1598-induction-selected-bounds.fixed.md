Induction now checks every implicit type bound assumed by its step in the base
case, even with `--property P`. A selected run can therefore report a violated
`_bounds_*` (exit 1) instead of a false unbounded proof (exit 0). Selected user
properties and standalone BMC/explicit selection remain unchanged.
