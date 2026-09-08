Fixed (#1000): `fslc kernel` now projects relation state fields initialized with
`Set {}` as `set_lit` with relation-typed `type` and empty `items`, matching
`fslc check`; non-empty relation literals remain rejected.
