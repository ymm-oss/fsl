Preserved progress refuses property pattern names matching free impl names from lexically
used maps, even outside readable continuations; prior `or` exit 0 `refines` becomes exit 2/type.
Used-map RHS patterns may not reuse any property binder or sibling used-map pattern name.
Rename the abstract or mapping pattern binder to repair these located errors at `respond`.
Ordinary source binding scope is unchanged. Refs #1595.
