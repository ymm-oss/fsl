# skills-next/

The layer skills for the next edition of the language (files headed `fsl 5`),
kept outside `skills/` until that edition is released. `skills/` is the
installable bundle: the release packages every directory directly under it, so
anything placed there reaches the users of the current release. This
directory is not packaged, installed, or declared as a release resource.

## What is here

| Skill | Replaces | What changed |
|---|---|---|
| [`fsl-business/`](fsl-business/) | `skills/fsl-business/` | business and governance under the `fsl 5` header: the stricter edition rules, the stage order the requirements layer must repeat, and the history flag a no-bypass policy hands to the layer below |
| [`fsl-requirements/`](fsl-requirements/) | `skills/fsl-requirements/` | the requirements forms of this edition (`entity` / `lifecycle` / `event` / `rule` / `decide` / `scenario` / `forbidden` / relations / measures), English as the canonical spelling, the English / Japanese keyword table, and how to write a `glossary` |
| [`fsl-design/`](fsl-design/) | `skills/fsl-design/` | a kernel design under `fsl 5` that refines a requirements spec of this edition through its lowered names |
| [`fsl-verify/`](fsl-verify/) | `skills/fsl-verify/` | the diagnostics of this edition (entity / event, `rule`, `decide`, glossary aliases, particles) and the origin self-check with `fslc explain --readable` / `--lowering` |

The other skills (`fsl`, `fsl-delivery`, `fsl-design-review`,
`fsl-from-code`, `fsl-requirements-document`) have no next-edition version
yet. A skill here is a whole replacement of its namesake, not a patch: when
the edition is released, each directory here replaces the directory of the
same name under `skills/`, and the others stay as they are. Until then, to use
these skills, install `skills/` and then these directories over it, so that
the `../fsl/` references resolve.

## Conventions the checks hold

- **Every `fsl` fence is checked.** In this directory a fence whose info
  string starts with `fsl` must be marked `fsl check`, and every marked fence
  must pass `fslc check` with the next edition enabled
  (`tools/check-doc-fences.py check --every-fence skills-next`, run by
  `rust/fslc/tests/literate_docs_contract.rs`). An unmarked `fsl` fence is a
  failure here, not a skipped fragment. A partial snippet that cannot check on
  its own (a mapping file, one declaration taken out of a spec) uses the info
  string `fsl-fragment`, which no checker reads.
- **One spec per document, headed `fsl 5`.** All fences of a document are one
  compilation unit, as `fslc check file.md` reads them, so a document holds at
  most one spec, spread over as many fences as the prose needs. Its first
  `fsl` fence starts with the line `fsl 5`.
- **The keyword table is generated.**
  [`fsl-requirements/references/keywords.md`](fsl-requirements/references/keywords.md)
  holds, between its markers, the table rendered from the parser's own table
  (`rust/fsl-syntax/src/keywords.rs`, `KEYWORDS` and `PARTICLES`).
  `rust/fslc/tests/fsl5_keywords.rs` renders it again and fails on any
  difference; locally, run that test with `FSLC_SKILLS_KEYWORDS_UPDATE=1` to
  rewrite the block after the table changes, and review the diff. When `CI`
  is set, any set `FSLC_SKILLS_KEYWORDS_UPDATE` variable fails the test instead
  of rewriting or comparing; CI without an update request compares normally.
- **The English and Japanese examples are one spec.** The example of
  `fsl-requirements/SKILL.md` and its Japanese spelling in
  `fsl-requirements/references/glossary.md` lower to the same kernel (spans,
  origins and annotations aside); the same test file checks it.
- **No gate name.** The material here is about the `fsl 5` header, which is
  what users write once the edition is released, so the header is allowed
  here (the published-material leak check does not scan this directory). The
  environment variable that enables the edition today is not: these files
  move into `skills/` unchanged, and the variable would then be a stale
  instruction. `rust/fslc/tests/unstable_gate_leak.rs` checks both that and
  that the release workflow and the installer never package this directory.
