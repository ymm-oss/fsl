Fixed (#1842): replaying a requirements document's `acceptance` and
`forbidden` cases, which `fslc check`, `verify`, every `analyze` projection and
the other trace-reading commands do first, no longer slows down with
`verify { instances }` or a `values` scope. Building the concrete initial state
copied the whole state for each `init` assignment and compared each `init`
write with every earlier one, so it was quadratic in the instance count, and the
replay built it again for every case. Each step also evaluated the guards of
every instance of every action. `init` now writes in place, the replay builds
the initial state once, and a step evaluates only the call it names for an
action whose guards cannot fail in the current state. Any other action still has
every instance evaluated, so trace verdicts and errors are unchanged. On
`examples/fsl5/leave_request.en.fsl` with `instances Leave = 2048`,
`analyze --projection action_state_graph --format mermaid` takes 0.07 s instead
of 13 s; with `values Days = 0..262144`, 0.04 s instead of 4 s.
