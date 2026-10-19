Changed (#1284): the conversion between internal names (compose
`alias__name`, engine-made violation names such as `_partial_<action>` and
`_bounds_<state>`) and the names written to public output now has one owner,
and a test rejects new spellings outside it. Command output is unchanged.
