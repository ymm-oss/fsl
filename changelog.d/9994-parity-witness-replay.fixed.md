Fixed (#1789): the native/WASM parity gate no longer fails when the two builds
pick different actions for a reachable or deadlock witness, such as
`auto_approve` and `approve` for `Submitted -> Approved`. It accepts that
difference only if both witnesses have the same states, each action's location
is its native source location, and both witnesses replay through native
`fslc replay`. All other envelope fields are still compared exactly. A new
self-test checks that the gate still rejects a witness action that is not
enabled, a witness that reaches a different state, and a moved action location.
