Changed (#1545, breaking removal): shared component validation rejects tool-less
components, including empty authority blocks, in `ai compat`, `ai replay`,
project `ai check` and native/Worker project checking. Standalone no-tools
envelopes from #1546 are preserved. Generic project checking no longer accepts
the fixtures `error_envelope_ai_project_unknown_tool.fsl` and
`error_envelope_ai_project_invalid_rule.fsl`, reporting their existing unknown
tool/rule semantics errors instead. Project semantic coordinates remain
block-relative (#1574); LSP project-component diagnostics remain separate (#1575).
