Fixed (#1285): `fslc analyze` graphs no longer present the acceptance /
forbidden cases of a spec whose traces the requirement-trace gate refuses
as if they had passed it. The graph still projects them (analyze reports
structure, so it does not fail), and each such `acceptance:<id>` /
`forbidden:<id>` node now carries `"trace_gate": "unchecked"`. Nodes of a
spec whose traces pass the gate are unchanged. Migration: a consumer of the
graph that treats acceptance / forbidden nodes as checked claims skips nodes
with `trace_gate: "unchecked"`.
