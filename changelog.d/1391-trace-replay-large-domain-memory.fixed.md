Fixed (#1391): replaying a requirements document's `acceptance` and
`forbidden` traces no longer holds memory in proportion to the
`verify { values }` scope of an action parameter. Each trace step used to list
every enabled instance of every action, so a published requirements spec with,
for example, `values Amount = 0..2000000` and a single `forbidden` made
`fslc check` use several GB until it was killed. Each step now keeps only the
call it names, and `check` on that spec stays at about 20 MB. Every instance's
guards are still evaluated, so trace verdicts and errors are unchanged, and
the replay time still grows with the scope.
