Changed (#1417): native verification baselines now delegate to the same ordered
pipeline as verify, sweep and depth-bearing chain spec layers, with caching disabled
and the default vacuity probe preserved. Direct ledger and HTML reports check verification errors
before presentation-model loading: malformed causal, Agent and DB-column inputs
retain the shared diagnostic envelope; an unknown AI authority tool returns a JSON
semantics error with exit 2 instead of panicking with exit 101. Passing reports,
warnings, metadata, public Kernel projections and approval artifact digests are
unchanged. The error-precedence inventory is in docs/design/DESIGN-ledger.md and
docs/design/DESIGN-html-report.md; both language references and the command skill
reference describe the shared boundary.
Approval create for ledger/HTML, approval check, approval diff and counterexample
export still lower the spec before verification or rerendering; their invalid-input
precedence and unknown-AI-tool panic/exit 101 are unchanged. The panics are tracked
in #1464; approval create's diagnostic precedence is a separate issue.
