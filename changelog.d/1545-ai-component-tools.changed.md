Changed (#1545, breaking removal): shared component validation rejects tool-less
components, including empty authority blocks, in `ai compat`, `ai replay`,
project `ai check` and native/Worker project checking. Standalone no-tools
envelopes from #1546 are preserved on check and specialized-preflight paths;
the lowering-only duplicate-fallback priority exception is measured below.
Generic project checking no longer accepts
the fixtures `error_envelope_ai_project_unknown_tool.fsl`,
`error_envelope_ai_project_invalid_rule.fsl` and the tool-less Markdown project
`error_envelope_literate_ai_project.md`. The first two report their existing
unknown tool/rule semantics errors; the Markdown project reports type,
`ai_component 'EnvelopeParityLiterateProject' declares no tools`, block loc1:1,
exit2. Project semantic coordinates remain
block-relative (#1574); LSP project-component diagnostics remain separate (#1575).
Measured using independently built B
`c32e7f5a88b412056210ee66f0cff9765001fed9` and H
`7be60c922c3d4f719746fda0d9763b075d1fa2a0` in their own git-archive trees:
`<B-binary> check <relative-path>` in B and `<H-binary> check <relative-path>`
in H, all 597 FSL and 30 Markdown corpus files plus twelve new inline occurrences
per tree, 1,278 checks, cache/unstable overrides unset and no excluded output
fields. Four Markdown files have FSL fences; bash-only examples/ai/README.md is
also covered as a negative control. Corpus ok/exit0 to error/exit2 removals are
the three named fixtures. Four new test-inline
occurrences are also breaking removals: tool-less-component projects in CLI and
Worker-shaped tests (two), an empty-authority-only component in a project (one),
and an invalid later component after a valid first one (one). The three tool-less
occurrences report type at block loc 1:1; the later unknown tool reports
unlocated semantics. No existing inline FSL was rewritten; other check-only
output/status observations are unchanged. Lowering-only analyze on 18 AI corpus
inputs plus four explicit shapes, followed by scenarios (depth2/deadlock ignore)
and explain on changed shapes and a compose control, produced
30 command-form pairs × 2 binaries = 60 observations.
Analyze22/scenarios4/explain4 each have one ok-to-error, zero error-to-ok and two
changed error envelopes. An undeclared forbidden X hidden by selecting only
`tool_authority` now reports semantics/`unknown tool 'X' in authority block`/no loc/exit2 instead of
success. Duplicate fallback x without tools changes
type/`ai_component 'DupNoTools' declares no tools`/loc1:1 to
semantics/`duplicate fallback reason 'x'`/no loc; with a tool, unlocated
semantics/`duplicate action 'fallback_x'` becomes
semantics/`duplicate fallback reason 'x'`/no loc (both exit2).
AI-component compose use stays rejected with
`compose use must reference a spec at 2:3`, loc2:3, exit2.
These are explicit lowering diagnostic/priority changes, not broad
envelope preservation. Declare a tool or remove an unneeded component and
correct unknown authority/rule references.
