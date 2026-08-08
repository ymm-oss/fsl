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

# Sharding (issue: CI wall-clock reduction). `--shard K/N` restricts the no-op
# control and the main operator loop to a round-robin slice of `operators.txt`
# (operator index `i`, 0-based in table order, belongs to shard `K` iff
# `i % N == K - 1`), so three shards can run the ~912s no-op control and the
# ~1350s operator loop in parallel. Everything else -- the whole-table
# validation in `read_table` and the harness's own stale-seam negative control
# -- still runs in every shard: those are cheap and a fault in either one must
# be caught by every shard, not by whichever shard happened to draw it. The
# default `1/1` (no flag) path assigns every operator to the one shard, so its
# behavior is unchanged apart from also writing `shard-manifest.v1.json`.
shard_index=1
shard_total=1
while [ "$#" -gt 0 ]; do
  case "$1" in
    --shard)
      if [ "$#" -lt 2 ]; then
        echo "usage: $0 [--shard K/N]" >&2
        exit 2
      fi
      if [[ "$2" =~ ^([1-9][0-9]*)/([1-9][0-9]*)$ ]]; then
        shard_index="${BASH_REMATCH[1]}"
        shard_total="${BASH_REMATCH[2]}"
        if [ "$shard_index" -gt "$shard_total" ]; then
          echo "usage: $0 [--shard K/N]: K must be <= N, got '$2'" >&2
          exit 2
        fi
      else
        echo "usage: $0 [--shard K/N]: '$2' is not K/N" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--shard K/N]" >&2
      exit 2
      ;;
  esac
done

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
contracts=()
seam_paths=()
seam_anchors=()
expected_changes=()
calibrated_edges=()
source_scopes=()

read_table() {
  local name patch_file contract seam_path seam_anchor primary_target primary_test
  local blind_target blind_test expected_change calibrated_edge source_scope extra
  while IFS='^' read -r name patch_file contract seam_path seam_anchor primary_target primary_test blind_target blind_test expected_change calibrated_edge source_scope extra; do
    name="$(trim "${name:-}")"
    [ -n "$name" ] || continue
    case "$name" in \#*) continue ;; esac
    [ -z "$(trim "${extra:-}")" ] || fail "$table: row '$name' has more than twelve columns"
    patch_file="$(trim "${patch_file:-}")"
    [ -n "$patch_file" ] || fail "$table: row '$name' names no patch file"
    [ -f "$operators_dir/$patch_file" ] || fail "$table: row '$name' names a missing patch file '$patch_file'"
    contract="$(trim "${contract:-}")"
    seam_path="$(trim "${seam_path:-}")"
    seam_anchor="$(trim "${seam_anchor:-}")"
    expected_change="$(trim "${expected_change:-}")"
    calibrated_edge="$(trim "${calibrated_edge:-}")"
    source_scope="$(trim "${source_scope:-}")"
    [ -n "$contract" ] || fail "$table: row '$name' names no violated contract"
    [ -n "$seam_path" ] || fail "$table: row '$name' names no seam path"
    [ -n "$seam_anchor" ] || fail "$table: row '$name' names no exact seam anchor"
    [ -n "$expected_change" ] || fail "$table: row '$name' names no expected semantic change"
    [ -n "$calibrated_edge" ] || fail "$table: row '$name' names no calibrated edge"
    [ -n "$source_scope" ] || fail "$table: row '$name' names no source scope"
    names+=("$name")
    patch_files+=("$operators_dir/$patch_file")
    contracts+=("$contract")
    seam_paths+=("$seam_path")
    seam_anchors+=("$seam_anchor")
    primary_targets+=("$(trim "${primary_target:-}")")
    primary_tests+=("$(trim "${primary_test:-}")")
    blind_targets+=("$(trim "${blind_target:-}")")
    blind_tests+=("$(trim "${blind_test:-}")")
    expected_changes+=("$expected_change")
    calibrated_edges+=("$calibrated_edge")
    source_scopes+=("$source_scope")
  done <"$table"
  [ "${#names[@]}" -gt 0 ] || fail "$table declares no operators"

  # An operator whose row was dropped but whose patch file stayed behind is a
  # silently skipped operator, which is the failure mode this harness exists to
  # prevent. Controls live in `controls/` and are named directly below.
  local file
  for file in "$operators_dir"/*.patch; do
    grep -q "\^[[:space:]]*$(basename "$file")[[:space:]]*\^" "$table" ||
      fail "$file is not referenced by any row in $table. Either add its row or
  delete the patch -- an operator that runs nowhere is not an operator."
  done
}

assigned_indices=()
assign_shard() {
  local index
  for index in "${!names[@]}"; do
    if [ $((index % shard_total)) -eq $((shard_index - 1)) ]; then
      assigned_indices+=("$index")
    fi
  done
  [ "${#assigned_indices[@]}" -gt 0 ] || fail "shard $shard_index/$shard_total is
  assigned no operators among the ${#names[@]} rows in $table. Either the shard
  count exceeds the table size or the round-robin assignment is wrong -- an
  empty shard would silently run nothing and still read green."
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

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# The path cargo linked for the target just built, read back from cargo's own
# `--no-run` output rather than reconstructed from the target name: `cargo test
# --no-run` prints one `Executable <target> (<path>)` line per selected target,
# whether or not it relinked, so this is the binary the detector is about to
# execute -- not an inference about it. CI colourizes cargo's output, so the
# escapes come off first.
executable_from_build_log() {
  sed -e 's/'$'\x1b''\[[0-9;]*m//g' "$1" \
    | sed -n 's/^ *Executable .*(\(.*\))$/\1/p' \
    | tail -n 1
}

# Runs one named detector in the scratch checkout and reports "ok" or "failed"
# in $detector_result. A detector that does not run at all is a loud failure,
# never a pass: a renamed or deleted test must not read as green, and a
# compile error must not be mistaken for the operator taking effect.
# $detector_binary_hash carries the executed binary's digest back to the
# caller, which is what lets run_operator prove the fault reached the thing it
# measured instead of assuming it did (#753).
detector_result=""
detector_binary_hash=""
run_detector() {
  local target="$1" name="$2" log="$3" status=0
  # `$target` is a cargo test-target selector ("--lib", "--test <name>") and
  # is deliberately word-split.
  # shellcheck disable=SC2086
  if ! (cd "$scratch" && cargo test --manifest-path rust/Cargo.toml -p fslc-rust $target --locked --no-run) >"$log.build" 2>&1; then
    tail -40 "$log.build" >&2
    fail "the patched checkout does not compile (target: $target). Full log: $log.build"
  fi
  local binary
  binary="$(executable_from_build_log "$log.build")"
  [ -n "$binary" ] && [ -f "$binary" ] || fail "cargo reported no executable for
  target '$target', so nothing here can prove which binary the detector ran.
  Full log: $log.build"
  detector_binary_hash="$(hash_file "$binary")"
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

# The source witness's own negative control (#753): a patch that applies
# cleanly and changes nothing must be refused by assert_fault_reached_source.
# Needs no build either, so it also runs before the expensive controls. The
# subshell is load-bearing -- assert_fault_reached_source reports through
# `fail`, which exits, and this control needs to observe that exit rather than
# inherit it.
check_source_witness_control() {
  local started=$SECONDS control="$operators_dir/controls/identical-after-apply.patch"
  sync_scratch
  apply_operator_patch "$control" "$logs/identical-after-apply.apply.log"
  if (assert_fault_reached_source "$control" "identical-after-apply") >/dev/null 2>&1; then
    fail "the identical-after-apply control satisfied the source witness. A
  patch that applies cleanly while leaving the bytes unchanged is now
  indistinguishable from a real fault, so every 'primary still passed' verdict
  below is ambiguous again between a detector gap and a fault that never
  arrived."
  fi
  echo "control identical-after-apply: source witness refused it, as required ($((SECONDS - started))s)"
}

# Every named detector must be green under a patch that changes no behavior.
check_no_op_control() {
  local started=$SECONDS index
  sync_scratch
  apply_operator_patch "$operators_dir/controls/no-op.patch" "$logs/no-op.apply.log"
  for index in "${assigned_indices[@]}"; do
    run_detector "${primary_targets[$index]}" "${primary_tests[$index]}" \
      "$logs/no-op.${names[$index]}.primary.log"
    # The unfaulted digest of the exact binary this operator's primary detector
    # will be measured on. Recorded here, under the no-op control, because that
    # is the one point in the run where the scratch is known to carry no fault.
    no_op_primary_hashes[index]="$detector_binary_hash"
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
  echo "control no-op: all ${#assigned_indices[@]} operators' detectors green ($((SECONDS - started))s)"
}

failures=()
no_op_primary_hashes=()

# Two fail-closed witnesses that the fault this run reports on actually reached
# the thing it measured (#753). Without them the harness infers that from a
# clean `git apply` and a clean build, and a `primary=ok` verdict is then
# ambiguous between the two things it must never conflate: a detector that
# genuinely does not cover the seam (a real, reportable defect) and a detector
# that never saw the fault at all (a harness defect reported as the former).
# That ambiguity is exactly what made the same operator return different
# verdicts on different runs of the same revision.
#
# Witness 1, source: every file the patch names must differ from the pristine
# working-tree copy once the patch is applied. `git apply` exiting zero is not
# that evidence -- it says the patch was accepted, not that the bytes under
# the compiler changed.
#
# Witness 2, binary: the primary detector's executable must differ from the
# digest recorded for the same target under the no-op control. A fault that
# reaches the source but not the linked binary produces a byte-identical
# executable, which is unambiguous: no compilation nondeterminism can make a
# genuinely faulted binary equal the unfaulted one, so this only ever fires on
# a real reuse, never on a flaky digest.
assert_fault_reached_source() {
  local file="$1" name="$2" patched seen=0
  while IFS= read -r patched; do
    [ -n "$patched" ] || continue
    seen=$((seen + 1))
    [ -f "$scratch/$patched" ] || fail "operator '$name': $patched is missing
  from the scratch after its patch applied."
    if cmp -s "$root/$patched" "$scratch/$patched"; then
      fail "operator '$name': $patched in the scratch is byte-identical to the
  working tree after \`git apply\` reported success, so the fault never reached
  the source the detector is about to be built from. Any verdict below would be
  a property of the harness, not of the fault."
    fi
  done < <(sed -n 's/^+++ b\///p' "$file" | sort -u)
  [ "$seen" -gt 0 ] || fail "operator '$name': its patch names no files, so
  there is nothing to verify reached the scratch."
}

run_operator() {
  local index="$1" name="${names[$1]}" started=$SECONDS
  sync_scratch
  apply_operator_patch "${patch_files[$index]}" "$logs/$name.apply.log"
  assert_fault_reached_source "${patch_files[$index]}" "$name"

  run_detector "${primary_targets[$index]}" "${primary_tests[$index]}" \
    "$logs/$name.primary.log"
  if [ "$detector_binary_hash" = "${no_op_primary_hashes[$index]}" ]; then
    fail "operator '$name': the primary detector's binary is byte-identical to
  the one built under the no-op control ($detector_binary_hash), so the fault
  never reached the binary the verdict came from. This is a harness defect, not
  a detector gap: do not record '$name' as uncalibrated on this evidence.
  Full log: $logs/$name.primary.log.build"
  fi
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
assign_shard
check_stale_seam_control
check_source_witness_control
check_no_op_control

for index in "${assigned_indices[@]}"; do
  run_operator "$index"
done

if [ "${#failures[@]}" -gt 0 ]; then
  printf 'fault-operators: %s\n' "${failures[@]}" >&2
  exit 1
fi

# Per-shard completeness evidence (issue: CI wall-clock reduction). Written
# only on success, so a failed shard's manifest never masquerades as
# completed evidence. The aggregator downloads every shard's manifest, checks
# `base_revision` and `table_operators` agree across shards, and requires the
# disjoint union of `executed_operators` to equal `table_operators` exactly --
# the same fail-closed shape as `check-shard-union.sh`.
mkdir -p "$logs"
executed_names=()
for index in "${assigned_indices[@]}"; do
  executed_names+=("${names[$index]}")
done
table_operators_json="$(printf '%s\n' "${names[@]}" | jq -R . | jq -s .)"
executed_operators_json="$(printf '%s\n' "${executed_names[@]}" | jq -R . | jq -s .)"
jq -n \
  --arg schema "fslc.fault-operator-shard-manifest.v1" \
  --argjson schema_version 1 \
  --arg base_revision "$(git -C "$root" rev-parse HEAD)" \
  --argjson shard_index "$shard_index" \
  --argjson shard_total "$shard_total" \
  --argjson table_operators "$table_operators_json" \
  --argjson executed_operators "$executed_operators_json" \
  '{
    schema: $schema,
    schema_version: $schema_version,
    base_revision: $base_revision,
    shard: {index: $shard_index, total: $shard_total},
    table_operators: $table_operators,
    executed_operators: $executed_operators
  }' >"$logs/shard-manifest.v1.json"

echo "fault-operators: ${#assigned_indices[@]} operators calibrated (${SECONDS}s total)"
