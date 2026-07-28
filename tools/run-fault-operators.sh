#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Implementation fault operators (#537 C5). See
# `docs/DESIGN-conformance-harness.md` "Implementation fault operators".
#
# `injection_detector_matrix.rs` asks "can this detector see a bad spec?".
# This harness asks the other question: would the test suite notice if the
# *verifier* started lying? The defect lives in Rust, not in a `.fsl` file, so
# each operator is a **patch file, not code**. Nothing is injected at runtime
# and no fault-injection hook exists in the shipped binary, under a feature
# flag or otherwise: a switch that makes a verifier lie about verdicts must not
# exist in the codebase whose purpose is to prevent exactly that.
#
# For each operator the harness copies the working tree to a scratch checkout,
# applies the patch, rebuilds `fslc` there, and requires the operator's primary
# detector to fail while its blind detector still passes.
#
# Three properties are load-bearing:
#
#   1. The no-op control runs first and must leave *every* named detector
#      green. Without it a harness failing for its own reasons would read as
#      "the operator worked" and every cell below would be meaningless.
#   2. The blind detector is measured, not assumed. An operator that breaks
#      everything proves nothing about the detector it claims to calibrate.
#   3. A patch that no longer applies is a loud failure, never a skip. The
#      stale-seam control proves the harness still refuses one.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
operators_dir="$root/rust/fslc/tests/fault_operators"
table="$operators_dir/operators.txt"
# Inside `rust/target/`, which is git-ignored and excluded from the copy below,
# so the scratch tree never dirties the working tree and survives between runs
# (an unchanged file keeps its mtime, so only the patched crate rebuilds).
work="$root/rust/target/fault-operators"
scratch="$work/checkout"
logs="$work/logs"
export CARGO_TARGET_DIR="$work/target"

# Third scratch-fidelity defect closed here (after the missing repo root and
# the mtime-surviving faulted rlibs): two invocations sharing one scratch.
# A second runner applying and reverting patches while the first one's no-op
# control is measuring makes "primary failed without any fault applied" a lie
# on both sides, and the interleaved cargo output ("Blocking waiting for file
# lock") is the tell. `mkdir` is atomic on every platform this runs on, so it
# is the lock; a stale lock from a crashed run names its holder and is removed
# by hand, never automatically -- silently stealing a lock is how two runners
# end up back in one scratch.
lock="$work/lock"
mkdir -p "$work"
if ! mkdir "$lock" 2>/dev/null; then
  echo "fault-operators: another invocation holds $lock ($(cat "$lock/holder" 2>/dev/null || echo unknown)).
  If that run crashed, verify no cargo/fslc process is alive and remove the
  directory by hand. Not proceeding: two runners in one scratch make every
  verdict in the matrix meaningless." >&2
  exit 1
fi
echo "pid $$ started $(date -u +%FT%TZ)" >"$lock/holder"
trap 'rm -rf "$lock"' EXIT

fail() {
  echo "fault-operators: $*" >&2
  exit 1
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  printf '%s' "${value%"${value##*[![:space:]]}"}"
}

names=()
patch_files=()
primary_targets=()
primary_tests=()
blind_targets=()
blind_tests=()

read_table() {
  local name patch_file primary_target primary_test blind_target blind_test extra
  while IFS='|' read -r name patch_file primary_target primary_test blind_target blind_test extra; do
    name="$(trim "${name:-}")"
    [ -n "$name" ] || continue
    case "$name" in \#*) continue ;; esac
    [ -z "$(trim "${extra:-}")" ] || fail "$table: row '$name' has more than six columns"
    patch_file="$(trim "${patch_file:-}")"
    [ -n "$patch_file" ] || fail "$table: row '$name' names no patch file"
    [ -f "$operators_dir/$patch_file" ] || fail "$table: row '$name' names a missing patch file '$patch_file'"
    names+=("$name")
    patch_files+=("$operators_dir/$patch_file")
    primary_targets+=("$(trim "${primary_target:-}")")
    primary_tests+=("$(trim "${primary_test:-}")")
    blind_targets+=("$(trim "${blind_target:-}")")
    blind_tests+=("$(trim "${blind_test:-}")")
  done <"$table"
  [ "${#names[@]}" -gt 0 ] || fail "$table declares no operators"

  # An operator whose row was dropped but whose patch file stayed behind is a
  # silently skipped operator, which is the failure mode this harness exists to
  # prevent. Controls live in `controls/` and are named directly below.
  local file
  for file in "$operators_dir"/*.patch; do
    grep -q "|[[:space:]]*$(basename "$file")[[:space:]]*|" "$table" ||
      fail "$file is not referenced by any row in $table. Either add its row or
  delete the patch -- an operator that runs nowhere is not an operator."
  done
}

sync_scratch() {
  mkdir -p "$scratch" "$logs"
  rsync -a --delete \
    --exclude=target/ --exclude=.git --exclude=node_modules/ \
    "$root/" "$scratch/"
  # The scratch tree must be its own repository root. `portable_cli_source_path`
  # (`rust/fslc/src/main.rs`) resolves a public Kernel v2 `spec.source.file` by
  # walking ancestors for a `.git`, and the scratch lives *inside* `rust/target/`
  # of the real tree -- so without a marker of its own that walk escapes upward
  # and every such path gains a `rust/target/fault-operators/checkout/` prefix.
  # An infidelity like that makes a detector's verdict a property of the harness
  # rather than of the fault. `rsync --delete` leaves excluded paths alone, so
  # this survives the next sync.
  [ -e "$scratch/.git" ] || git -C "$scratch" init --quiet
  # Second scratch-fidelity defect this harness has had to close (the .git
  # marker above was the first): a *faulted* build can outlive the fault.
  # `rsync -a` preserves the worktree's mtimes, and after an operator run the
  # scratch target still holds rlibs compiled *with the fault applied*. When
  # the next run syncs the reverted sources back, their preserved mtimes can be
  # older than those rlibs' fingerprints, so cargo judges the crates fresh and
  # links the faulted rlibs into the no-op control's binary. First observed
  # with `unguarded-recursion`, the first operator to patch a library crate
  # (`fsl-syntax`) rather than the `fslc` bin: cold run green, every warm run
  # red, `cargo clean -p fsl-syntax` green again. Touching every file any
  # patch references makes cargo's mtime comparison come out on the side of
  # rebuilding, deterministically, without discarding the rest of the warm
  # cache. The no-op control is what caught this -- a harness that trusted its
  # cache would have reported the operator as calibrated while measuring the
  # fault itself.
  #
  # A missing target is left for `apply_patch` to report: it names the seam and
  # says what to do about it, where an abort here would say nothing. The `if`
  # rather than `[ -f ... ] && touch ...` is load-bearing for that -- as the
  # last command in the loop body the `&&` form yields a non-zero status when
  # the file is absent, and `set -e` would end the run silently on it.
  sed -n 's/^+++ b\///p' "$operators_dir"/*.patch "$operators_dir"/controls/*.patch \
    | sort -u \
    | while IFS= read -r patched; do
        if [ -f "$scratch/$patched" ]; then
          touch "$scratch/$patched"
        fi
      done
}

# Applies one patch to the scratch checkout. Exact context only: a seam that
# moved must be refused, not absorbed by fuzz.
#
# `git apply` rather than `patch`, because this harness's verdicts must be a
# property of the fault and not of the machine. BSD `patch` (macOS) accepted the
# no-op control against a hunk ending at end-of-file; GNU `patch` (ubuntu-latest)
# rejected the identical hunk against the identical bytes, so the matrix went
# green locally and red on its first CI run. `git apply` is one implementation
# everywhere git is, applies zero fuzz by default, and tolerates the prose
# preamble each patch file carries. The scratch already has a repository of its
# own -- `sync_scratch` gives it one so `portable_cli_source_path` cannot walk
# out into the enclosing tree -- so `git -C` has a work tree to apply into.
apply_patch() {
  local file="$1" log="$2" status=0
  git -C "$scratch" apply --whitespace=nowarn "$file" >"$log" 2>&1 || status=$?
  return "$status"
}

apply_operator_patch() {
  local file="$1" log="$2"
  if ! apply_patch "$file" "$log"; then
    cat "$log" >&2
    fail "operator patch '$file' no longer applies. The seam it targets moved:
  confirm the fault is still possible there and re-target the patch. Do not
  delete the operator to make this green, and do not skip it -- a silently
  skipped operator is how a detector matrix rots into decoration."
  fi
}

# Runs one named detector in the scratch checkout and reports "ok" or "failed"
# in $detector_result. A detector that does not run at all is a loud failure,
# never a pass: a renamed or deleted test must not read as green, and a
# compile error must not be mistaken for the operator taking effect.
detector_result=""
run_detector() {
  local target="$1" name="$2" log="$3" status=0
  # `$target` is a cargo test-target selector ("--lib", "--test <name>") and
  # is deliberately word-split.
  # shellcheck disable=SC2086
  if ! (cd "$scratch" && cargo test --manifest-path rust/Cargo.toml -p fslc-rust $target --locked --no-run) >"$log.build" 2>&1; then
    tail -40 "$log.build" >&2
    fail "the patched checkout does not compile (target: $target). Full log: $log.build"
  fi
  # shellcheck disable=SC2086
  (cd "$scratch" && cargo test --manifest-path rust/Cargo.toml -p fslc-rust $target --locked -- --exact "$name") >"$log" 2>&1 || status=$?
  if ! grep -q -- "^test ${name} \.\.\. " "$log"; then
    tail -20 "$log" >&2
    fail "detector '$name' did not run under \`cargo test -p fslc-rust $target\`.
  It was renamed, deleted, or its target moved. A detector that cannot run
  must never read as a pass. Full log: $log"
  fi
  if [ "$status" -eq 0 ]; then
    detector_result="ok"
  else
    detector_result="failed"
  fi
}

# The harness's own negative control: the stale-seam patch must be refused.
# Needs no build, so it runs first and costs nothing.
check_stale_seam_control() {
  local started=$SECONDS
  sync_scratch
  if apply_patch "$operators_dir/controls/stale-seam.patch" "$logs/stale-seam.log"; then
    fail "the stale-seam control patch applied cleanly. The harness can no
  longer tell a moved seam from a live one, so no operator below it is
  trustworthy."
  fi
  echo "control stale-seam: refused, as required ($((SECONDS - started))s)"
}

# Every named detector must be green under a patch that changes no behavior.
check_no_op_control() {
  local started=$SECONDS index
  sync_scratch
  apply_operator_patch "$operators_dir/controls/no-op.patch" "$logs/no-op.apply.log"
  for index in "${!names[@]}"; do
    run_detector "${primary_targets[$index]}" "${primary_tests[$index]}" \
      "$logs/no-op.${names[$index]}.primary.log"
    [ "$detector_result" = "ok" ] || fail "no-op control: primary detector
  '${primary_tests[$index]}' (operator ${names[$index]}) failed without any
  fault applied. Every cell in this matrix is meaningless until it passes.
  Full log: $logs/no-op.${names[$index]}.primary.log"
    run_detector "${blind_targets[$index]}" "${blind_tests[$index]}" \
      "$logs/no-op.${names[$index]}.blind.log"
    [ "$detector_result" = "ok" ] || fail "no-op control: blind detector
  '${blind_tests[$index]}' (operator ${names[$index]}) failed without any fault
  applied. Full log: $logs/no-op.${names[$index]}.blind.log"
  done
  echo "control no-op: all ${#names[@]} operators' detectors green ($((SECONDS - started))s)"
}

failures=()

run_operator() {
  local index="$1" name="${names[$1]}" started=$SECONDS
  sync_scratch
  apply_operator_patch "${patch_files[$index]}" "$logs/$name.apply.log"

  run_detector "${primary_targets[$index]}" "${primary_tests[$index]}" \
    "$logs/$name.primary.log"
  local primary="$detector_result"
  run_detector "${blind_targets[$index]}" "${blind_tests[$index]}" \
    "$logs/$name.blind.log"
  local blind="$detector_result"

  if [ "$primary" != "failed" ]; then
    failures+=("$name: primary detector '${primary_tests[$index]}' still passed
  under the fault. The suite would not notice this defect returning. Either the
  detector no longer covers the seam, or the patch no longer reaches it.
  Full log: $logs/$name.primary.log")
  fi
  if [ "$blind" != "ok" ]; then
    failures+=("$name: blind detector '${blind_tests[$index]}' also failed. An
  operator that breaks everything calibrates nothing -- narrow the patch.
  Full log: $logs/$name.blind.log")
  fi
  printf 'operator %s: primary=%s blind=%s (%ss)\n' \
    "$name" "$primary" "$blind" "$((SECONDS - started))"
}

read_table
check_stale_seam_control
check_no_op_control

for index in "${!names[@]}"; do
  run_operator "$index"
done

if [ "${#failures[@]}" -gt 0 ]; then
  printf 'fault-operators: %s\n' "${failures[@]}" >&2
  exit 1
fi

echo "fault-operators: ${#names[@]} operators calibrated (${SECONDS}s total)"
