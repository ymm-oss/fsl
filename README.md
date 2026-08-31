# FSL — AI-Native Formal Specification Language

FSL is a formal specification language for application development, designed with
the primary goal of being **written, verified, and repaired by generative AI**.

The verifier is **`fslc`**, distributed as a native Rust executable with **Z3 bundled in**.
It performs **bounded model checking (BMC)** and **infinite-depth proofs via k-induction**
(plus a Z3-free explicit-state engine for the solver-independent path), and always returns
results as **machine-readable JSON**, for the LLM write → verify → repair loop. It also
includes `fslc scenarios` and `fslc testgen`, which generate integration-test scaffolds
from a spec.

Specs can be written in **three layered dialects — consulting (business) / requirements / design (spec)** —
chained via refinement so that requirement IDs propagate transparently across all diagnostics.
Non-functional requirements are also supported, down to SLAs (discrete time).

For the language specification, semantics, and output JSON see [`docs/LANGUAGE.md`](docs/LANGUAGE.md);
for a map of all the documentation see [`docs/README.md`](docs/README.md).

> **A note on this repository's two implementations.** The native Rust workspace under
> [`rust/`](rust/) is the authoritative implementation — it is what every command below
> runs, and what ships in Releases. `src/fslc/` is a **frozen Python compatibility
> reference** kept for differential testing; it is not the product, and new behavior does
> not land there. See [`AGENTS.md`](AGENTS.md) if you are contributing.

## Install

Most people should use **the install script** — it sets up `fslc`, `fslc-lsp`, and the
Claude Code Agent Skills together, and is the only route that also gets you skill
integration. Use one of the other two routes only if it specifically fits you:

- Just want the `fslc` binary, nothing else (no PATH setup, no skills)? [Download a single
  executable](#download-a-single-executable-instead).
- Already use Nix? [Nix (Flakes)](#nix-flakes) builds `fslc` from source with Z3 bundled in.
- Building `fslc` yourself, or need the frozen Python reference for compatibility work?
  [Developer setup](#developer-setup-building-from-source).

### The install script (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/ymm-oss/fsl/main/install.sh | bash
```

No programming knowledge or Git installation is required — on Mac, open Terminal.app and
run the command above. Windows users should use WSL, or use the developer instructions
(PowerShell) below.

This places only the versioned release payload under `${XDG_DATA_HOME:-~/.local/share}/fsl`,
links the commands from `~/.local/bin`, and links the Claude Code skills from
`~/.claude/skills/`. It does not clone the repository or install examples, documentation,
Python sources, or Rust sources. `FSL_DATA_DIR` overrides the release-payload location
(taking priority over `XDG_DATA_HOME`) and `FSL_BIN_DIR` overrides the command-link
location.

What gets installed:

- the native Rust `fslc` and `fslc-lsp` commands (used from `~/.local/bin`; Python is not
  required) — the [VSCode extension](#editor-integration) launches `fslc-lsp` from `PATH`
- the Claude Code skills (`~/.claude/skills/fsl*`)

The installer verifies both native checksums and rejects a binary whose reported version
differs from the latest Release tag. It also migrates recognized command links that point
at the old `~/.fsl/.venv/bin` or `~/.fsl/.native/bin` installs (the pre-Rust, fslc
2.7-era layout). Use `--no-skill` to skip creating the Claude Code skill links.

Uninstall:

```bash
rm -rf ~/.local/share/fsl ~/.local/bin/fslc ~/.local/bin/fslc-lsp ~/.claude/skills/fsl ~/.claude/skills/fsl-business ~/.claude/skills/fsl-requirements ~/.claude/skills/fsl-design ~/.claude/skills/fsl-design-review ~/.claude/skills/fsl-delivery
```

See the [Releases](https://github.com/ymm-oss/fsl/releases) page for the current version. Official releases contain checksummed native binaries,
the VSCode extension, and Kernel bundles; every release after v3.0.0 also contains a
checksummed Agent Skill bundle, and the installer separately verifies a pinned
source-archive checksum for the v3.0.0 compatibility path (v3.0.0 predates the checksummed
skill bundle). The Python compatibility reference
remains in this repository at version 2.7.0, but this installer does not install it and it
is not published to PyPI. The Rust workspace crates are not published to crates.io.
Publishing either surface requires an explicit manifest, workflow, and documentation
change.

Maintainers cut releases using the documented [`docs/RELEASE.md`](docs/RELEASE.md)
procedure and the internal [`release` Agent Skill](.claude/skills/release/SKILL.md).

### Download a single executable instead

`fslc` is a **single native executable**. You need neither a Python install, `pip`, nor a
separately installed Z3 — grab the one file for your OS from GitHub **Releases** and it
runs, with no skill integration and no PATH setup.

| OS / arch | File to download |
| --- | --- |
| macOS (Apple Silicon, M1 and later) | `fslc-macos-arm64` |
| Linux (x86_64, glibc 2.39+) | `fslc-linux-x64` |
| Linux (ARM64, glibc 2.39+) | `fslc-linux-arm64` |
| Windows (x64) | `fslc-windows-x64.exe` |

```bash
# Example: macOS (Apple Silicon)
chmod +x fslc-macos-arm64
./fslc-macos-arm64 verify spec.fsl
```

> **macOS note**: a downloaded executable will be blocked by Gatekeeper. The first time
> only, remove the quarantine attribute: `xattr -d com.apple.quarantine ./fslc-macos-arm64`
> (or right-click in Finder → "Open" once).

Each file ships with a companion `*.sha256`. You can verify it with
`shasum -a 256 -c fslc-macos-arm64.sha256`. The Linux binaries target the Ubuntu 24.04 ABI
baseline (glibc 2.39 or newer).

> This binary bundles Z3, so all features including `verify` work without a separately
> installed solver. Normal operating-system runtime libraries still apply.

### Nix (Flakes)

The project provides optional Nix flake outputs for users who already use Nix. The flake builds `fslc` (and `fslc-lsp`) from source with Z3 bundled in.

```bash
# Latest source from default branch
nix run github:ymm-oss/fsl

# Specific release (uses the flake at that git tag)
nix run github:ymm-oss/fsl/v4.4.1

# Named outputs: #fslc, #source
nix run github:ymm-oss/fsl#fslc
nix run github:ymm-oss/fsl#source

# Build / develop
nix build github:ymm-oss/fsl
nix develop github:ymm-oss/fsl
```

The flake exposes `packages.<system>.fslc` / `.default` / `.source`, `apps.<system>.fslc` / `.default`, `devShells.<system>.default`, and `overlays.default`.

Update through the same Nix workflow you used to install. For profile installs, run `nix profile list` and then `nix profile upgrade <index-or-name>`. For flake inputs, run `nix flake update fsl` in your own flake and rebuild.

### Devbox

For a reproducible development environment (Rust toolchain + `cmake` for the bundled Z3 build), use [Devbox](https://www.jetify.com/devbox):

```bash
# Install Devbox first (if not already installed)
curl -fsSL https://get.jetify.dev/devbox | bash

# Initialize the environment
devbox shell

# Build the project (uses --manifest-path rust/Cargo.toml)
devbox run build

# Run the verifier on a spec
devbox run check
```

Or install Devbox via Homebrew:

```bash
brew install jetify-com/devbox/devbox
```

> **Note for x86_64-darwin (Intel Mac) users:** Devbox currently uses
> nixpkgs-unstable internally, which has dropped x86_64-darwin support.
> Use the Nix flake dev shell instead:
>
> ```bash
> nix develop --extra-experimental-features 'nix-command flakes'
> ```

## Write your first spec with an AI agent

The basic way to use FSL is **not** for a person to memorize the FSL syntax and write it
by hand; instead, you have an AI agent write the spec, reading the verification results
as it repairs it.

```text
Use $fsl-requirements to write a requirements spec for a cancellation request flow.
Only approved orders can be canceled; cancellation after shipping is not allowed; refunds must not run twice.
Verify it, fix any problems, and keep iterating until there are none.
```

For PM use, start with `$fsl-requirements`; for consulting/business-flow work, start with
`$fsl-business`. The AI follows the language reference and repair protocol in the skill,
creates the `.fsl` file, and verifies it with `fslc`.

**Note:** what the verifier guarantees is "no contradictions or counterexamples within the
scope of what is written in the spec." A human should confirm that the spec the AI wrote
correctly represents the original business rules, requirements, and exceptional
conditions, and, when a counterexample appears, that the revised interpretation is
reasonable as a matter of business.

Turning acceptance criteria into scenarios, test-scaffold generation in several target
languages, and conformance checking of existing event logs against the spec can all be
chained from the same `.fsl` spec. For this too, it is enough to ask the AI: "also build
test scaffolds from this spec" or "check whether this log conforms to the spec."

## Using the CLI directly

Every command below was run against this repository's own `specs/` (the `document` and
`ledger` examples below run against `examples/pm/` instead). Except where noted, output
is JSON on stdout (`fslc chain` also writes a human status table to stderr).

### Check and verify

```bash
fslc check  specs/cart_v1.fsl                        # syntax/types only (fast loop)
fslc verify specs/cart_v1.fsl --depth 8               # BMC: verified + shortest counterexample/witness
fslc verify specs/cart_v1.fsl --engine induction      # k-induction: proved (infinite depth)
fslc verify specs/cart_v1.fsl --engine explicit       # Z3-free explicit-state BFS
fslc verify specs/cart_v1.fsl --engine auto           # explicit-state first, transparent BMC fallback
```

`cart_v1_buggy.fsl` returns the shortest counterexample trace for the automatic bounds
check (`type_bound`). `verify` also has `--explicit-budget` (exceeding the explicit
engine's visited-state budget returns `unknown_budget` rather than a false `verified`),
`--property`/`--exclude-property` to scope which invariant/`trans`/`leadsTo`/`reachable`
obligation runs, `--instances`/`--values` to override verify-block bounds, `--lemma` for
induction candidate auxiliary invariants, `--from-state` to continue BMC from a captured
Monitor/replay state snapshot, `--requirements` to scope by requirement id, `--deadlock
{warn,error,ignore}`, and `--edition {current,next}`. See `fslc verify --help` for the
full, current list.

### Scenarios, test generation, and log conformance

```bash
fslc scenarios specs/cart_v1.fsl                          # generate integration-test scaffold JSON
fslc testgen   specs/cart_v1.fsl -o test_cart_v1.py       # pytest conformance-test scaffold (default target)
fslc testgen   specs/cart_v1.fsl --target vitest -o cart_v1.test.js
fslc replay    specs/cart_v1.fsl --trace events.json      # conformance check of a captured full-state
                                                            # trace; events.json is a trace you capture or
                                                            # build yourself (schema: docs/DESIGN-replay-trace.md)
fslc replay    specs/cart_v1.fsl --from-log events.jsonl --mapping log_mapping.fsl
                                                            # conformance check of a production event log,
                                                            # mapped into the spec's actions/state by a
                                                            # refinement-syntax mapping file you author
                                                            # (see docs/DESIGN-log-replay.md)
```

`testgen --target` also accepts `swift`, `kotlin`, `dart`, and `phpunit`.

### Refinement and composition

```bash
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
                                                  # check whether the detailed spec refines the abstract spec
                                                  # mapping files can opt into preserve progress for upper leadsTo
fslc verify specs/order_system.fsl --depth 8     # compose: synchronized composition of cart + payment
```

`fslc chain` runs an entire business → requirements → design → impl pipeline from a
project manifest (a small TOML file naming each layer's spec, its refinement mapping, and
an optional implementation-conformance command) that you write for your own project —
there is no shared example manifest at the repository root, since a manifest is inherently
project-specific. `docs/DESIGN-*.md` and the `fsl-delivery` skill describe the manifest
shape; running it looks like:

```bash
fslc chain fsl-project.toml --keep-going
```

### Validation suite (closes the gap between spec ≠ intent)

```bash
fslc verify  specs/cart_v1.fsl --vacuity error    # detect vacuous properties (unreachable antecedent/trigger, always-true requires)
fslc verify  specs/cart_v1.fsl --strict-tags      # match untagged declarations (fabrication candidates) and unreferenced requirements (omission candidates)
fslc mutate  specs/cart_v1.fsl                    # bounded mutant-set sensitivity of the property net (survivors are a review queue)
fslc explain specs/cart_v1.fsl                    # skeleton enumeration + counterfactuals (what would happen without this rule)
fslc analyze specs/cart_v1.fsl --profile ai-review  # structural review findings (not proof failures)
```

`analyze` also emits structural projections (`--projection {tsg, action_dependency_graph,
action_state_graph, code_audit, impact_graph, property_state_graph,
requirement_property_graph, refinement_graph, traceability_graph}`) in `--format {dot,
json, mermaid}`, and `--export tag-review` for declaration-level tag/formula review
tuples.

### Reports, requirements documents, and audit ledgers

```bash
fslc html      specs/cart_v1.fsl -o cart_report.html       # self-contained HTML report for team review
fslc document generate examples/pm/cancel_system.fsl -o cancel_flow.md
                                                              # render a controlled-language requirements
                                                              # document (ja/en) from a checked spec
fslc document claims  examples/pm/cancel_system.fsl          # emit the same content as a Requirement Claim IR (RCIR) JSON set
fslc document check   examples/pm/cancel_system.fsl cancel_flow.md
                                                              # detect drift between a generated document and
                                                              # a fresh re-render of its spec
fslc ledger    examples/pm/cancel_system.fsl -o cancel_flow_ledger.md
                                                              # business audit ledger (markdown) by requirement id,
                                                              # optionally scored against an implementation trace
                                                              # (--impl-log) or checked digest-bound approvals
```

`fslc typestate specs/order_workflow.fsl --ts` decides whether typestate (ghost types)
applies to a design spec and, with `--ts`, emits the TypeScript scaffold directly on
stdout — TypeScript, not JSON, unlike every other command in this section.

### The Public Kernel JSON contract

```bash
fslc kernel specs/cart_v1.fsl              # normalized public Kernel JSON (schema fslc/kernel/kernel.v1)
```

Since the Rust crates are not published, **the supported programmatic surface is the
CLI's JSON envelope and this Public Kernel contract**, not a library import. See
[`docs/DESIGN-kernel-contract.md`](docs/DESIGN-kernel-contract.md).

### Editor integration

Every release ships an `fslc-lsp` binary and a VSCode extension (`fsl-vscode.vsix`).
Install the extension from the Release page (`code --install-extension
fsl-vscode.vsix`, or "Extensions: Install from VSIX…"); it is a thin LSP client
that launches `fslc-lsp` from `PATH` (the install script above puts it there) and gives you
syntax highlighting, diagnostics, outline, and go-to-definition. See
[`editors/vscode/README.md`](editors/vscode/README.md).

### Everything else

`fslc` has commands not covered above — `version`, `lint`, `migrate`, `fmt`, `sweep`,
`conformance`, `approval`, and `diff` among them. Run `fslc --help` or `fslc <command>
--help` for their current arguments, or see [`docs/README.md`](docs/README.md) for the
design document covering most of them (`sweep` has no entry there yet).

`fslc` also has dialect-specific command groups — `ai` (AI hard-contract/agent-structure
dialect), `domain` (Functional DDD / async effect dialect), `db` (database
compatibility dialect), `compat` (shared compatibility commands), and `causal`
(review-only causal hypothesis graphs). These exist and are documented under `docs/`
(`DESIGN-ai-hard.md`, `DESIGN-domain.md`, `DESIGN-db.md`, `DESIGN-causal.md`); this README
does not tutorialize them.

### Exit codes

0 = verified / proved / refines / conformant / generated / mutated / explained / analyzed
/ typestate; 1 = violated / refinement_failed / reachable_failed / unknown_cti /
unknown_budget / nonconformant; 2 = spec error (`error`, including vacuity under
`--vacuity error`); 3 = internal error.

## Skills for AI agents

Because FSL is a language not present in training data, when an AI agent (such as Claude
Code) writes a spec, the **Agent Skills** supply the language specification,
role-specific workflow, and repair protocol into context. For easy distribution and
discovery, the canonical copies live under [`skills/`](skills/) at the repository root:

- [`skills/fsl/SKILL.md`](skills/fsl/SKILL.md) — shared verifier workflow / repair protocol / minimal syntax
- `skills/fsl/references/` — topical FSL language and verifier references, indexed from the core skill
- [`skills/fsl-business/SKILL.md`](skills/fsl-business/SKILL.md) — business process, controls, KPIs, and goals
- [`skills/fsl-requirements/SKILL.md`](skills/fsl-requirements/SKILL.md) — PM requirements, acceptance criteria, forbidden flows, and NFRs
- [`skills/fsl-design/SKILL.md`](skills/fsl-design/SKILL.md) — engineering design specs and refinement to requirements
- [`skills/fsl-design-review/SKILL.md`](skills/fsl-design-review/SKILL.md) — design review, variant checks, and substitutability judgment
- [`skills/fsl-delivery/SKILL.md`](skills/fsl-delivery/SKILL.md) — end-to-end workflow orchestration across planning, requirements, design, and implementation conformance
- [`skills/fsl-from-code/SKILL.md`](skills/fsl-from-code/SKILL.md) — reverse-engineer a design-layer spec from existing source code
- [`skills/fsl-requirements-document/SKILL.md`](skills/fsl-requirements-document/SKILL.md) — generate and re-verify a human-readable requirements document from a checked spec

The install script above links the first six into `~/.claude/skills/`; `fsl-from-code` and
`fsl-requirements-document` are not part of that installed set today, but are checked into
this repository's own `.claude/skills/` and `.agents/skills/` as symlinks to `skills/*`. To
use any of them in another project, copy the relevant `skills/fsl*` directories into that
project's `.claude/skills/` or into `~/.claude/skills/`, or point the `gh` skill extension
at `skills/` as the distribution source. See [`skills/README.md`](skills/README.md) for
details.

## Repository layout

```
fsl/
├── README.md
├── rust/                   # authoritative implementation — see AGENTS.md "Project structure"
│   ├── fsl-syntax/         #   lexer, parsers, source locations, and surface AST
│   ├── fsl-core/           #   typed kernel model, validation, resolution, and dialect lowering
│   ├── fsl-runtime/        #   solver-independent Monitor and explicit-state/BFS behavior
│   ├── fsl-solver*/        #   backend-neutral solver boundary plus native and browser Z3 backends
│   ├── fsl-verifier/       #   BMC, induction, refinement, liveness, and scenarios
│   ├── fsl-tools/          #   analysis, mutation, report, typestate, and test generation tools
│   ├── fslc/               #   native CLI and JSON/process contract
│   ├── fsl-wasm/           #   browser Worker surface
│   └── fsl-lsp/            #   native language server and document index
├── src/fslc/               # frozen Python compatibility reference; do not add product behavior here
├── docs/                   # docs/README.md maps all of it; LANGUAGE.md is the language reference
├── specs/                  # sample specs (*.fsl); most carry a header comment naming the
│                           #   language feature/idiom they exercise (not all do — browse the directory)
├── examples/               # FSL corpus by audience and topic — pm/, consulting/, e2e/, gallery/, bank/,
│                           #   layers/, nfr/, domain/, db/, ai/, causal/, and more; see the directory itself
├── skills/                 # canonical Agent Skills; .claude/skills/fsl* and .agents/skills/fsl* symlink here
└── tests/                  # Python-driven Rust contract, parity, and compatibility tests
```

`specs/` and `examples/` are FSL corpus and reproducing cases; browse them directly rather
than relying on a hand-maintained inventory here, since a per-file list is exactly what
went stale in this document before. Two specs worth knowing up front because other
sections use them: `specs/cart_v1.fsl` (basic `Option`/`ensures`/`reachable`, used
throughout this README) and `specs/order_system.fsl` (a `compose` of cart + payment).

## Developer setup (building from source)

```bash
git clone https://github.com/ymm-oss/fsl && cd fsl
```

The native Rust workspace under `rust/` is what you build and test:

```bash
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- check specs/cart_v1.fsl
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- verify specs/cart_v1.fsl --depth 8
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- verify specs/cart_v1.fsl --engine induction
```

The complete required product gate is Rust-native and does not execute Python:

```bash
./tools/check-native-integration.sh
```

See [`AGENTS.md`](AGENTS.md) for the full CI-equivalent gate (`cargo fmt`, `cargo clippy
--workspace --all-targets -D warnings`, `cargo test --workspace`, `cargo build
--workspace`) and the narrower gates for solver, logic-semantics, and corpus changes.

Python is optional and used only for changes explicitly scoped to the frozen
compatibility reference, or for Python-based repository hooks:

**Mac / Linux:**

```bash
python3 -m venv .venv
source .venv/bin/activate         # for fish: source .venv/bin/activate.fish
pip install -e ".[dev]"           # installs lark, z3-solver, pytest, and an editable fslc for differential testing
```

**Windows (PowerShell):**

```powershell
py -m venv .venv
.venv\Scripts\Activate.ps1        # for cmd: .venv\Scripts\activate.bat
pip install -e ".[dev]"
```

You can also run it directly without activating the venv:
`./.venv/bin/python -m fslc ...` (on Windows, `.venv\Scripts\python -m fslc ...`).

`tests/` drives Rust-contract, parity, and frozen-reference compatibility checks from
Python; it is not the product gate above — run it (`pytest`) only if you are working on
the compatibility surface. Alongside the differential tests that cross-check the two
evaluators (Z3-backed BMC and the concrete Monitor), `tests/oracle.py` is a
**Z3-independent brute-force oracle**: it enumerates bounded reachable states by driving
the Monitor directly, to catch false negatives — a case wrongly reported
`verified`/`proved`/`refines` when the oracle finds a state that should have been
`violated`/`refinement_failed`.

## License

Distributed under the [Apache License 2.0](LICENSE). Copyright 2026 Ryoichi Izumita.

The frozen Python reference's dependencies, `lark` and `z3-solver`, are both under the MIT
License (compatible with Apache-2.0). See [`NOTICE`](NOTICE) for details.
