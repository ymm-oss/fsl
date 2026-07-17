# FSL — AI-Native Formal Specification Language

FSL is a formal specification language for application development, designed with
the primary goal of being **written, verified, and repaired by generative AI**.
The verifier `fslc` uses Lark + Z3 to perform **bounded model checking (BMC)**
and **infinite-depth proofs via k-induction**, and always returns results as
**machine-readable JSON** (for the LLM write→verify→repair loop).
It also includes `fslc scenarios`, which generates integration-test scaffolds from a spec.

Specs can be written in **three layered dialects — consulting (business) / requirements / design (spec)** —
chained via refinement so that requirement IDs propagate transparently across all diagnostics. Non-functional requirements are also supported, down to SLAs (discrete time).
For the language specification, semantics, and output JSON see [`docs/LANGUAGE.md`](docs/LANGUAGE.md);
for a map of all the documentation see [`docs/README.md`](docs/README.md).

## First steps

The basic way to use FSL is **not** for a person to memorize the FSL syntax and write it by hand;
instead, you install `fslc` and the Agent Skill and then have an AI agent write the spec,
reading the verification results as it repairs it.

1. **Install FSL and the skill**

   ```bash
   # If you downloaded and unzipped the ZIP from GitHub
   cd ~/Downloads/fsl-main
   bash install.sh

   # If you use the GitHub CLI
   gh repo clone ymm-oss/fsl ~/.fsl
   bash ~/.fsl/install.sh
   ```

   A standard install gives you the verifier `fslc` and the Claude Code skills
   under `~/.claude/skills/`. If you want AI to write FSL in another project too,
   load these skills there.

2. **Ask an AI agent to build it using the requirements skill**

   ```text
   Use $fsl-requirements to write a requirements spec for a cancellation request flow.
   Only approved orders can be canceled; cancellation after shipping is not allowed; refunds must not run twice.
   Verify it, fix any problems, and keep iterating until there are none.
   ```

   For PM use, start with `$fsl-requirements`; for consulting/business-flow work,
   start with `$fsl-business`.
   The AI follows the language reference and repair protocol in the skill,
   creates the `.fsl` file, and verifies it with `fslc`.

   **Note:** what the verifier guarantees is "no contradictions or counterexamples within the scope of what is written in the spec."
   A human should confirm that the spec the AI wrote correctly represents the original business rules, requirements, and exceptional conditions,
   and, when a counterexample appears, that the revised interpretation is reasonable as a matter of business.

3. **If needed, have it generate tests and implementation hookup too**

   Turning acceptance criteria into scenarios, pytest conformance-test scaffolds, and conformance checking
   of existing event logs against the spec can all be chained from the same `.fsl` spec. For this too,
   it is enough to ask the AI: "also build test scaffolds from this spec" or "check whether this log conforms to the spec."

## Directory layout

```
fsl/
├── README.md
├── pyproject.toml          # dependencies (lark, z3-solver) and the fslc command definition
├── docs/
│   ├── README.md           # map of docs (start here)
│   ├── LANGUAGE.md         # language reference — read this if you are writing specs
│   └── DESIGN-*.md         # authoritative design contracts and distilled findings
├── specs/                  # sample specs (*.fsl) — all the correct ones are proved at k=1
│   ├── cart_v1.fsl         #   basic form of Option / ensures / reachable
│   ├── cart_v1_buggy.fsl   #   missing guard — returns the shortest type_bound violation counterexample
│   ├── order_workflow.fsl  #   enum / struct / Set / sum
│   ├── auth_lockout.fsl    #   lockout + ghost variable + auxiliary invariant
│   ├── inventory_reservation.fsl  # conservation-law invariant
│   ├── payment.fsl         #   partial refunds + ledger + auxiliary invariant
│   ├── rate_limiter.fsl    #   token bucket
│   ├── mutex_queue.fsl     #   FIFO mutex (Option + Seq)
│   ├── job_pipeline.fsl    #   job queue with retries (Seq + struct)
│   ├── audit_log.fsl       #   append-only log + Seq aggregation idiom
│   ├── order_system.fsl    #   compose: cart_v1 + payment with synchronized checkout/capture
│   ├── bank{,_impl,_refines,_system}.fsl  # refinement + compose chain
│   ├── seat_booking*.fsl   #   refinement with a conditional mapping
│   ├── repair_loop.fsl     #   self-spec of fslc's own workflow
│   └── cart_{buggy,fixed}.fsl     # v0-compatible samples
├── examples/
│   ├── pm/                 # for PM/PdM (cancellation flow: business + requirements)
│   ├── consulting/         # for consulting (As-Is/To-Be control checks)
│   ├── e2e/                # three-role integration (consulting → PM → engineer → implementation)
│   ├── gallery/            # case-study gallery (valid examples / invalid-example catalog / adversarial)
│   ├── bank/               # conformance tests against a plain Python implementation (8/8)
│   ├── layers/             # three-layer chain (business → requirements → design)
│   └── nfr/                # discrete-time SLA (urgency discipline, time-budget invariant)
├── src/fslc/               # verifier package
│   ├── __init__.py         #   public API: parse / build_spec / verify
│   ├── __main__.py         #   for python -m fslc
│   ├── grammar.py          #   Lark grammar + AST transformer
│   ├── parser.py           #   parse(src) -> AST
│   ├── model.py            #   build_spec / type→Z3 sort / constant evaluation / FslError
│   ├── bmc.py              #   verify / prove (k-induction) / scenarios / trace generation
│   ├── runtime.py          #   Monitor concrete interpreter (no Z3 required)
│   ├── testgen.py          #   pytest conformance-test scaffold generation
│   ├── html_report.py      #   self-contained HTML review report rendering
│   └── cli.py              #   CLI and JSON output / error envelope
└── tests/                  # pytest (v0-compat / v1 / induction / scenarios / runtime /
                            #         dialects / NFR / independent-oracle cross-checking, trace soundness)
```

## Easiest of all: just download the executable (no Python needed)

`fslc` is distributed as a **standalone single binary**. You need neither a Python install,
`pip`, nor `git`. Just grab the one file for your OS from GitHub **Releases**
and it runs.

| OS / arch | File to download |
| --- | --- |
| macOS (Apple Silicon, M1 and later) | `fslc-macos-arm64` |
| Linux (x86_64) | `fslc-linux-x64` |
| Linux (ARM64) | `fslc-linux-arm64` |
| Windows (x64) | `fslc-windows-x64.exe` |

```bash
# Example: macOS (Apple Silicon)
chmod +x fslc-macos-arm64
./fslc-macos-arm64 verify spec.fsl
```

> **macOS note**: a downloaded executable will be blocked by Gatekeeper.
> The first time only, remove the quarantine attribute:
> `xattr -d com.apple.quarantine ./fslc-macos-arm64`
> (or right-click in Finder → "Open" once).

Each file ships with a companion `*.sha256`. You can verify it with
`shasum -a 256 -c fslc-macos-arm64.sha256`.

> This binary bundles even z3's native library, so all features including `verify`
> work with no external dependencies. If you need skill integration or editable development,
> use the setup instructions below.

## Easy setup (for PMs, consultants, and non-engineers)

No programming knowledge is required. Just these three steps:

1. **Download** — open ymm-oss/fsl on GitHub in your browser and click the green
   **"Code" ▾ → "Download ZIP"** (no login needed, since it is a public repository).
   Double-click the downloaded zip to unzip it.
2. **Open a terminal** (on Mac, "Terminal.app"; search your apps for "terminal").
3. In the folder you unzipped, **run the install command**:

   ```bash
   cd ~/Downloads/fsl-main      # adjust to the name of the folder you unzipped
   bash install.sh
   ```

This places FSL itself in `~/.fsl`, the `fslc` command in `~/.local/bin/fslc`, and
the Claude Code skills in `~/.claude/skills/`
(once placed, you can delete the folder you downloaded).

> For those who use the GitHub CLI, or engineers: if you have run `gh auth login`, this one line also works:
> `gh repo clone ymm-oss/fsl ~/.fsl && bash ~/.fsl/install.sh`

What gets installed:

- the native Rust `fslc` and `fslc-lsp` commands (used from `~/.local/bin`; Python is not required)
  — the VSCode extension in `editors/vscode/` launches `fslc-lsp` from `PATH`
- the Claude Code skills (`~/.claude/skills/fsl*`)
- samples for PMs and consultants (`examples/pm/`, `examples/consulting/`)

Windows users should use WSL or refer to the developer instructions (PowerShell).

Uninstall:

```bash
rm -rf ~/.fsl ~/.local/bin/fslc ~/.local/bin/fslc-lsp ~/.claude/skills/fsl ~/.claude/skills/fsl-business ~/.claude/skills/fsl-requirements ~/.claude/skills/fsl-design ~/.claude/skills/fsl-design-review ~/.claude/skills/fsl-delivery
```

Official releases are the checksummed native binaries, VSCode extension, and
Kernel bundles attached to GitHub Releases. The retained Python compatibility
reference is installed from this repository and is not published to PyPI; the
Rust workspace crates are not published to crates.io. Publishing either surface
requires an explicit manifest, workflow, and documentation change.
Maintainers cut releases using the documented [`docs/RELEASE.md`](docs/RELEASE.md)
procedure and the internal [`release` Agent Skill](.claude/skills/release/SKILL.md).

## Developer setup

First get the repository:

```bash
git clone https://github.com/ymm-oss/fsl && cd fsl
```

The commands below install the retained Python reference implementation for differential
testing. There are only two runtime dependencies: `lark` (pure Python) and `z3-solver`
(a prebuilt wheel that bundles the native libz3). **No C++ compiler or separate Z3 install is needed**,
and on Mac / Windows / Linux it is all done with just `pip install` (requires Python 3.9+).

**Mac / Linux:**

```bash
python3 -m venv .venv
source .venv/bin/activate         # for fish: source .venv/bin/activate.fish
pip install -e ".[dev]"           # installs lark, z3-solver, pytest and does an editable install of fslc
```

**Windows (PowerShell):**

```powershell
py -m venv .venv
.venv\Scripts\Activate.ps1        # for cmd: .venv\Scripts\activate.bat
pip install -e ".[dev]"
```

You can also run it directly without activating the venv:
`./.venv/bin/python -m fslc ...` (on Windows, `.venv\Scripts\python -m fslc ...`).

## Using the CLI directly

```bash
fslc check  specs/cart_v1.fsl                    # syntax/types only (fast loop)
fslc verify specs/cart_v1.fsl --depth 8          # BMC: verified + shortest counterexample/witness
fslc verify specs/cart_v1.fsl --engine induction # k-induction: proved (infinite depth)
fslc scenarios specs/cart_v1.fsl                 # generate integration-test scaffold JSON
fslc replay specs/cart_v1.fsl --trace events.json  # conformance check of an event log
fslc testgen specs/cart_v1.fsl -o test_cart_v1.py  # generate a pytest conformance-test scaffold
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
                                                  # check whether the detailed spec refines the abstract spec
                                                  # mapping files can opt into preserve progress for upper leadsTo
fslc chain fsl-project.toml --keep-going          # run business -> requirements -> design -> impl from a manifest
fslc verify specs/order_system.fsl --depth 8    # compose: synchronized composition of cart + payment

# Validation suite (closes the gap between spec ≠ intent; see docs/DESIGN-{forbidden,vacuity,...})
fslc verify specs/cart_v1.fsl --vacuity error   # detect vacuous properties (unreachable antecedent/trigger, always-true requires)
fslc verify specs/cart_v1.fsl --strict-tags     # match untagged declarations (fabrication candidates) and unreferenced requirements (omission candidates)
fslc mutate specs/cart_v1.fsl                    # spec mutation: bounded mutant-set sensitivity of the property net (survivors are a review queue)
fslc explain specs/cart_v1.fsl                   # skeleton enumeration + counterfactuals (what would happen without this rule)
fslc analyze specs/cart_v1.fsl --profile ai-review # structural review findings (not proof failures)
fslc html specs/cart_v1.fsl -o cart_report.html  # self-contained HTML report for team review
fslc typestate specs/order_workflow.fsl --ts    # state machine → applicability check for phantom types + TS scaffold
# (in the requirements dialect, the forbidden block can also write "operation sequences that should be rejected")

# Can also be run as a module without installing
python -m fslc verify specs/cart_v1_buggy.fsl
```

The output is JSON on stdout (`fslc chain` also writes its human status table to
stderr). Exit codes: 0 = verified / proved / refines /
conformant / generated / mutated / explained / analyzed / typestate; 1 = violated /
refinement_failed / reachable_failed / unknown_cti / nonconformant;
2 = spec error (`error`, including vacuity under `--vacuity error`); 3 = internal error.
`cart_v1_buggy.fsl` returns the shortest counterexample trace for the automatic bounds check (`type_bound`).

## Skills for AI agents

Because FSL is a language not present in training data, when an AI agent (such as Claude Code)
writes a spec, the **Agent Skills** supply the language specification, role-specific
workflow, and repair protocol into context. For easy distribution and discovery,
the canonical copies live under [`skills/`](skills/) at the repository root:

- [`skills/fsl/SKILL.md`](skills/fsl/SKILL.md) — shared verifier workflow / repair protocol / minimal syntax
- [`skills/fsl/reference.md`](skills/fsl/reference.md) — condensed, complete language-reference card
- [`skills/fsl-business/SKILL.md`](skills/fsl-business/SKILL.md) — business process, controls, KPIs, and goals
- [`skills/fsl-requirements/SKILL.md`](skills/fsl-requirements/SKILL.md) — PM requirements, acceptance criteria, forbidden flows, and NFRs
- [`skills/fsl-design/SKILL.md`](skills/fsl-design/SKILL.md) — engineering design specs and refinement to requirements
- [`skills/fsl-design-review/SKILL.md`](skills/fsl-design-review/SKILL.md) — design review, variant checks, and substitutability judgment
- [`skills/fsl-delivery/SKILL.md`](skills/fsl-delivery/SKILL.md) — end-to-end workflow orchestration across planning, requirements, design, and implementation conformance

Claude Code working in this repository recognizes them automatically via `.claude/skills/`
(symbolic links to `skills/*`). To use them in another project,
copy the relevant `skills/fsl*` directories into that project's `.claude/skills/` or into `~/.claude/skills/`,
or point the `gh` skill extension at `skills/` as the distribution source.
See [`skills/README.md`](skills/README.md) for details.

## Tests

```bash
pytest
```

301 tests (+69 skipped) verify all features (v0-compat / type system / k-induction / leadsTo /
scenarios / runtime / refine / compose / three-layer dialects / NFR) (about 260 seconds).
The two evaluators (Z3 and the concrete Monitor) cross-check each other via differential tests of witness replay, and in addition
a **Z3-independent brute-force oracle** (`tests/oracle.py`) checks for false negatives (misses where something that should be violated/
refinement_failed is wrongly reported as verified/proved/refines).

## Library API

```python
from fslc import parse, build_spec, verify, prove

spec   = build_spec(parse(open("specs/cart_v1.fsl").read()))
result = verify(spec, depth=8)              # BMC. dict (same structure as the CLI)
result = prove(spec, k_ind=1, base_depth=8) # k-induction (proved / unknown_cti)
```

## License

Distributed under the [Apache License 2.0](LICENSE). Copyright 2026 Ryoichi Izumita.

The dependencies `lark` and `z3-solver` are both under the MIT License (compatible with Apache-2.0).
See [`NOTICE`](NOTICE) for details.
