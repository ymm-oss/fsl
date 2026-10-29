Preserved progress now refuses any property pattern binder whose name matches a
free impl name introduced by a map actually used under lexical scope, even outside
readable continuations. Formerly accepted `or` forms change from exit 0 `refines` to
located exit 2 type errors. Rename the abstract pattern binder to repair the mapping.
Ordinary source binding scope is unchanged. Refs #1595.
