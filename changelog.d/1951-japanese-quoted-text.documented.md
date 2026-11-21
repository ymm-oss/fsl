Documented (#1951): the `fsl-requirements` and `fsl-business` skills tell the
writer to write a quoted text in Japanese directly as a Japanese sentence, not
as a translation of the English title, and not to set a name into it with
spaces around it (`"休暇は 1 日以上、最大日数以下で申請する"`, not
`"休暇は 1 日以上 最大日数 以下で申請する"`): nothing checks a quoted text and a
name inside it is not resolved. Four quoted texts of
`examples/fsl5/leave_request.ja.fsl` follow the rule. No behavior changes.
