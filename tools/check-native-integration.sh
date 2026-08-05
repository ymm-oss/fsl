#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

assert_dependency_absent() {
  local package="$1" pattern="$2" message="$3" tree
  tree="$(cargo tree --manifest-path rust/Cargo.toml -p "$package" --edges normal)"
  if grep -E "$pattern" <<<"$tree"; then
    echo "$message" >&2
    exit 1
  fi
}

check_rust() {
  check_stack_parity
  cargo fmt --manifest-path rust/Cargo.toml --all -- --check
  cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --lib --locked
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --test stdio --locked
  cargo test --manifest-path rust/Cargo.toml -p fsl-lsp --test corpus --locked
  cargo test --manifest-path rust/Cargo.toml --workspace --exclude fsl-lsp --locked
  cargo build --manifest-path rust/Cargo.toml --workspace --locked

  check_boundaries
}

# Sharded, non-test half of `check_rust` (issue: CI wall-clock reduction).
# `rust workspace` was the critical-path-adjacent job at 32m43s, of which
# cache restore and compilation together were only ~4 min and sequential test
# execution across 176 binaries was ~29 min -- `cargo test` runs test
# binaries one at a time. This phase carries everything except that test
# execution: it stays cheap and runs once, in the `rust-checks` job, while
# `check_rust_tests` below shards the test execution 3 ways in `rust-tests`
# using `cargo-nextest`, which nextest cannot do for doctests. Doctests
# therefore stay here, explicit and unsharded, rather than silently dropped.
check_rust_checks() {
  check_stack_parity
  cargo fmt --manifest-path rust/Cargo.toml --all -- --check
  cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
  cargo test --manifest-path rust/Cargo.toml --workspace --doc --locked
  cargo build --manifest-path rust/Cargo.toml --workspace --locked

  check_boundaries
}

# Nextest-pinned version. Keep this in sync with the `rust-tests` matrix's
# cache/install step and cache key in `.github/workflows/ci.yml` -- a drift
# between the installed binary and this assertion must fail loudly, not run
# an unverified partitioning behavior. `cargo nextest --version`'s first line
# ("cargo-nextest 0.9.143 (<commit> <date>)") embeds the upstream build's
# commit hash, so compare the stable `release:` line instead.
readonly NEXTEST_VERSION="0.9.143"

# Runs one shard (`spec` = "K/N", 1-based, K <= N) of the workspace's
# non-doctest tests under `cargo-nextest`, after writing the shard's
# completeness evidence: `full.txt` (every non-ignored test in the
# workspace) and `shard.txt` (this shard's slice), both written *before* the
# shard runs so the inventory exists even when a test in it fails. The
# `rust-workspace` aggregator downloads every shard's `full.txt`, asserts
# they are byte-identical (three independently computed listings agreeing),
# and checks the union of every `shard.txt` against one `full.txt` with
# `check-shard-union.sh`.
check_rust_tests() {
  local spec="${1:-}"
  if [[ ! "$spec" =~ ^([1-9][0-9]*)/([1-9][0-9]*)$ ]]; then
    echo "usage: $0 rust-tests K/N (K, N positive integers, K <= N); got '${spec}'" >&2
    exit 2
  fi
  local shard_index="${BASH_REMATCH[1]}" shard_total="${BASH_REMATCH[2]}"
  if [ "$shard_index" -gt "$shard_total" ]; then
    echo "usage: $0 rust-tests K/N: K must be <= N, got '$spec'" >&2
    exit 2
  fi

  local installed
  installed="$(cargo nextest --version 2>&1 | awk -F': ' '/^release:/ {print $2}')"
  if [ "$installed" != "$NEXTEST_VERSION" ]; then
    echo "check-native-integration: requires exactly cargo-nextest $NEXTEST_VERSION; observed release '$installed' (raw: $(cargo nextest --version 2>&1 | head -n1))" >&2
    exit 1
  fi

  local shard_dir="rust/target/test-shards"
  mkdir -p "$shard_dir"
  local full="$shard_dir/full.txt"
  local shard="$shard_dir/shard.txt"

  # `rust-suites` is an object keyed by suite name, not an array; each
  # suite's `binary-id` disambiguates same-named tests across binaries.
  # `--partition` does not shrink the listing -- every suite still lists
  # every test, and each testcase's `filter-match.status` says whether *this*
  # invocation's partition claims it ("matches") or another one does
  # ("mismatch"). Verified directly against this workspace (1389 tests):
  # unpartitioned `ignored == false` gives all 1389; the three
  # `count:K/3` partitions' `filter-match.status == "matches"` sets are
  # pairwise disjoint and their union is exactly those 1389.
  cargo nextest list \
    --manifest-path rust/Cargo.toml --workspace --locked \
    --message-format json \
    | jq -r '
        .["rust-suites"][]
        | ."binary-id" as $bid
        | .testcases
        | to_entries[]
        | select(.value.ignored == false)
        | $bid + "::" + .key' \
    | sort -u >"$full"
  cargo nextest list \
    --manifest-path rust/Cargo.toml --workspace --locked \
    --partition "count:${shard_index}/${shard_total}" \
    --message-format json \
    | jq -r '
        .["rust-suites"][]
        | ."binary-id" as $bid
        | .testcases
        | to_entries[]
        | select(.value.ignored == false and .value["filter-match"].status == "matches")
        | $bid + "::" + .key' \
    | sort -u >"$shard"

  [ -s "$full" ] || {
    echo "check-native-integration: nextest listed zero non-ignored tests for the workspace; refusing to run a shard against an empty inventory" >&2
    exit 1
  }
  [ -s "$shard" ] || {
    echo "check-native-integration: shard $spec listed zero tests -- N is likely larger than the test count, or the partition math is wrong" >&2
    exit 1
  }
  if [ -n "$(comm -23 <(sort -u "$shard") <(sort -u "$full"))" ]; then
    echo "check-native-integration: shard $spec names tests absent from the full workspace listing" >&2
    exit 1
  fi

  cargo nextest run \
    --manifest-path rust/Cargo.toml --workspace --locked \
    --partition "count:${shard_index}/${shard_total}"
}

check_boundaries() {
  assert_dependency_absent fsl-runtime 'fsl-solver|z3' 'fsl-runtime must remain solver-independent'
  assert_dependency_absent fsl-wasm 'fsl-solver-z3 v' 'fsl-wasm must not depend on the native Z3 backend'
}

# Implementation fault operators (#537 C5): would the suite notice if the
# verifier started lying? Deliberately kept out of `check_rust` — it patches a
# scratch checkout and rebuilds `fslc` there once per operator, and
# `docs/DESIGN-conformance-harness.md` puts that rebuild cost in the product
# gate rather than in the phase every pull request runs.
check_fault_operators() {
  ./tools/run-fault-operators.sh
}

check_semantic_mutation() {
  ./tools/run-semantic-mutation-gate.sh "${1:-complete}"
}

check_fsl_logic() {
  ./tools/run-fsl-logic-test.sh "${1:-pr}"
}

# Negative control for issue #617: `fslc` must not depend on the operating
# system's choice of main-thread stack.
#
# Windows gives 1 MiB where Linux and macOS give 8, and `fslc refine` needed
# more than 1 on `examples/agentic_rag`: it answered `refines` on Linux and
# aborted with `has overflowed its stack` on Windows, from the same bytes. The
# fix gives every platform 8 MiB explicitly; this asserts it, on the platforms
# that can ask for a smaller one.
#
# `ulimit -s 1024` makes a Unix runner reproduce the Windows condition, so the
# regression is caught here rather than only post-merge on the Windows matrix
# — which is where it actually escaped to, unnoticed for three merges. Depth 2
# is deliberate: the abort does not depend on `--depth` (it happened at 2 and
# at 4 alike), so the cheap run is as good a control as the expensive one.
check_stack_parity() {
  if ! (ulimit -s 1024) 2>/dev/null; then
    echo "check-native-integration: cannot lower the stack limit here; \
issue #617's control needs a shell that can (Linux/macOS). Not skipping \
silently: run this phase on a platform that can, or the control is absent." >&2
    return 1
  fi
  cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked
  local out
  out=$(
    ulimit -s 1024
    ./rust/target/debug/fslc refine \
      examples/agentic_rag/agentic_rag_design.fsl \
      examples/agentic_rag/agentic_rag_requirements.fsl \
      examples/agentic_rag/agentic_rag_design_refines_requirements.fsl \
      --depth 2 2>&1
  )
  local status=$?
  if [ "$status" -ne 0 ] || printf '%s' "$out" | grep -q "overflowed its stack"; then
    printf '%s\n' "$out" >&2
    echo "check-native-integration: \`fslc refine\` did not survive a 1 MiB \
stack (exit $status). That is the Windows default, so this is issue #617 \
returning: the CLI is back on whatever stack the OS picked." >&2
    return 1
  fi
  echo "stack parity: refine survives a 1 MiB (Windows-sized) stack"
}

check_wasm() {
  npm --prefix rust/spikes/z3js-worker ci
  npm --prefix rust/spikes/z3js-worker run probe
  npm --prefix rust/spikes/z3js-worker run probe:browser
  cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked
  npm --prefix rust/fsl-wasm ci
  npm --prefix rust/fsl-wasm run test:browser
}

case "${1:-all}" in
  rust)
    check_rust
    ;;
  rust-checks)
    check_rust_checks
    ;;
  rust-tests)
    check_rust_tests "${2:-}"
    ;;
  wasm)
    check_wasm
    ;;
  boundaries)
    check_boundaries
    ;;
  fault-operators)
    check_fault_operators
    ;;
  semantic-mutation)
    check_semantic_mutation "${2:-complete}"
    ;;
  fsl-logic)
    check_fsl_logic "${2:-pr}"
    ;;
  stack-parity)
    check_stack_parity
    ;;
  all)
    check_rust
    check_wasm
    check_semantic_mutation complete
    check_fsl_logic scheduled
    ;;
  *)
    echo "usage: $0 [all|rust|rust-checks|rust-tests K/N|wasm|boundaries|fault-operators|semantic-mutation [changed|complete]|fsl-logic [pr|scheduled]|stack-parity]" >&2
    exit 2
    ;;
esac
