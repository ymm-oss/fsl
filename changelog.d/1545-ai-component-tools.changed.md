Changed (#1545, breaking removal): shared component validation rejects tool-less
components, including empty authority blocks, in `ai compat`, `ai replay`,
project `ai check` and native/Worker project checking. Standalone no-tools
envelopes from #1546 are preserved. Generic project checking no longer accepts
the fixtures `error_envelope_ai_project_unknown_tool.fsl` and
`error_envelope_ai_project_invalid_rule.fsl`, reporting their existing unknown
tool/rule semantics errors instead. Project semantic coordinates remain
block-relative (#1574); LSP project-component diagnostics remain separate (#1575).
Measured using independently built B
`e856bbe023f8fb64557658afbd365c355ac56715` and H
`651e58b344d0b4c95712ea216b9b177cdd7a7ca9` in their own git-archive trees:
`<B-binary> check <relative-path>` in B and `<H-binary> check <relative-path>`
in H, all 597 corpus files plus ten new inline occurrences per tree, 1,214
checks, cache/unstable overrides unset and no excluded output fields. Corpus
ok/exit0 to error/exit2 removals are the two named fixtures. Four new test-inline
occurrences are also breaking removals: tool-less-component projects in CLI and
Worker-shaped tests (two), an empty-authority-only component in a project (one),
and an invalid later component after a valid first one (one). The three tool-less
occurrences report type at block loc 1:1; the later unknown tool reports
unlocated semantics. No existing inline FSL was rewritten; other output/status
observations are unchanged. Declare a tool or remove an unneeded component and
correct unknown authority/rule references.
