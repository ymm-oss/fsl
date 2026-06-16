# skills/

A place for distributable Agent Skills. They are placed at the repository root so
they are easy to find on GitHub and can be distributed via the `gh` skill
extension or by manual copy.

## Skill set

FSL work is split by layer so an agent does not turn PM requirements into design
artifacts by accident:

| Skill | Use for | Main deliverable |
|---|---|---|
| [`fsl/`](fsl/) | shared language syntax, verifier commands, repair protocol, JSON result interpretation | core FSL reference |
| [`fsl-business/`](fsl-business/) | business flows, As-Is/To-Be controls, policies, KPIs, goals | `business` spec |
| [`fsl-requirements/`](fsl-requirements/) | PM/PdM requirements, acceptance criteria, forbidden flows, NFR/SLA | `requirements` spec |
| [`fsl-design/`](fsl-design/) | engineering design, internal state/actions, refinement mapping, testgen/replay handoff | kernel `spec` + mapping |
| [`fsl-design-review/`](fsl-design-review/) | design review, variants, SOLID/LSP/OCP/substitutability judgment | contract-conformance report |
| [`fsl-delivery/`](fsl-delivery/) | end-to-end FSL delivery orchestration from planning through implementation conformance | lifecycle status and gated handoff |

The role-specific skills delegate syntax and verifier details to `fsl/`. Use the
narrowest role skill for authoring, then load `fsl/` when writing syntax or
repairing verifier output. Use `fsl-delivery/` when the task spans multiple
layers and needs lifecycle coordination.

## Files

- [`fsl/SKILL.md`](fsl/SKILL.md) — shared workflow, result→next-move repair
  protocol, minimal syntax, and structural rules
- [`fsl/reference.md`](fsl/reference.md) — condensed full language reference card
  for kernel FSL, compose, refinement, business/requirements dialects, and NFR
- [`fsl-business/SKILL.md`](fsl-business/SKILL.md) — business-layer procedure and
  guardrails
- [`fsl-requirements/SKILL.md`](fsl-requirements/SKILL.md) — requirements-layer
  procedure and PM guardrails
- [`fsl-design/SKILL.md`](fsl-design/SKILL.md) — design-layer procedure and
  refinement guardrails
- [`fsl-design-review/SKILL.md`](fsl-design-review/SKILL.md) — review procedure and
  design-principle interpretation
- [`fsl-delivery/SKILL.md`](fsl-delivery/SKILL.md) — lifecycle orchestration
  across business, requirements, design, verification, and implementation
  conformance

## Installation

**Claude Code inside this repository**: no extra work is needed, because there are
symbolic links to each skill under `.claude/skills/`.

**Using it in another project**: copy the relevant `skills/fsl*` directories into
the target project's `.claude/skills/`, or into the user-wide
`~/.claude/skills/`. If you use the `gh` skill extension, specify this directory
(`skills/`) as the distribution source.

The verifier `fslc` itself is required separately (`pip install -e .` at the
repository root; the only dependencies are lark and z3-solver).
