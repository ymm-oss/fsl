Fixed (#1603): invalid written next-edition rule-property names, including `__`
and literal names such as `none:`, now report `kind: "name"` instead of
`kind: "semantics"` and include structured `loc` at the statement's start, not
the label. Exit 2, `diagnostic_code: "invalid_rule_property_name"`, and the
diagnostic wording are unchanged. When checking a file, the message suffix
changes from `at L:C` to `at <source-file>:L:C`, like other located name errors.
