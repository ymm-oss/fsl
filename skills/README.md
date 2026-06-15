# skills/

A place for distributable Agent Skills. They are placed at the repository root so
they are easy to find on GitHub and can be distributed via the `gh` skill extension
or by manual copy.

## fsl

[`fsl/`](fsl/) — a skill that teaches FSL (this repository's formal specification
language) to AI agents. FSL is a language that does not exist in training data, so
for an agent to write specs, this skill must supply the language specification and
the repair protocol into context.

- [`fsl/SKILL.md`](fsl/SKILL.md) — workflow, the result→next-move repair protocol
  table, minimal syntax, and the rules to follow structurally (the main body read
  when the skill is invoked)
- [`fsl/reference.md`](fsl/reference.md) — a condensed full language reference card
  (compose / refinement / a catalog of all expressions / a collection of idioms)

## fsl-design-review

[`fsl-design-review/`](fsl-design-review/) — a procedure skill for design study and
design review using FSL. It describes a design proposal, variant, extension, or
change as a "refinement to a frozen contract (abstract spec)" and reports the result
of `fslc refine` in the vocabulary of design principles (SOLID's LSP/OCP, design by
contract, etc.). The procedure is the spine, and the principles appear as judgment
lenses at each step. FSL syntax is delegated to the fsl skill (intended to be used
together).

- [`fsl-design-review/SKILL.md`](fsl-design-review/SKILL.md) — the 5-step procedure,
  the check-result→design-judgment translation table, the principle↔mechanism
  correspondence table, and the discipline of the abstract layer

### Installation

**Claude Code (inside this repository)**: no extra work is needed, because there are
symbolic links to each skill under `.claude/skills/`.

**Using it in another project**: copy `skills/fsl/` (and, if needed,
`skills/fsl-design-review/`) into the target project's `.claude/skills/`, or into
the user-wide `~/.claude/skills/`. If you use the `gh` skill extension, specify this
directory (`skills/`) as the distribution source.

The verifier `fslc` itself is required separately (`pip install -e .` at the
repository root; the only dependencies are lark and z3-solver).
