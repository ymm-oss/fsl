#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
runner_version="cargo-mutants 27.1.0"

# Lane split (issue: CI wall-clock reduction). With no `--lane` flag every
# check below still runs, in the same order, exactly as before -- that is
# the contract this file's local and CI default callers depend on.
# `--lane operators` runs only the curated fault-operator half (optionally
# sharded with `--shard K/N`, forwarded verbatim to
# `run-fault-operators.sh`); `--lane mutants` runs everything else: the
# generic cargo-mutants half plus the manifest/schema checks that are cheap
# enough to run in both lanes. `--shard` without `--lane operators` is
# refused rather than silently ignored.
mode=""
lane=""
shard_spec=""
usage() {
  echo "usage: $0 [changed|complete] [--lane operators|mutants] [--shard K/N]" >&2
  exit 2
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    --lane)
      [ "$#" -ge 2 ] || usage
      case "$2" in
        operators|mutants) lane="$2" ;;
        *) usage ;;
      esac
      shift 2
      ;;
    --shard)
      [ "$#" -ge 2 ] || usage
      shard_spec="$2"
      shift 2
      ;;
    changed|complete)
      [ -z "$mode" ] || usage
      mode="$1"
      shift
      ;;
    *)
      usage
      ;;
  esac
done
mode="${mode:-complete}"

if [ -n "$shard_spec" ]; then
  [ "$lane" = "operators" ] || usage
  [[ "$shard_spec" =~ ^([1-9][0-9]*)/([1-9][0-9]*)$ ]] || usage
  [ "${BASH_REMATCH[1]}" -le "${BASH_REMATCH[2]}" ] || usage
fi

case "$mode" in
  changed|complete) ;;
  *)
    usage
    ;;
esac

run_manifest_test() {
  # Validates the operator inventory, the P2 scope/equivalents manifests, and
  # the mutation runner config together. Cheap (compile-dominated, ~1 min)
  # and idempotent, so it runs once for the unsharded default path and once
  # per invocation when split into lanes -- both lanes depend on parts of it
  # (operators.txt for the operators lane, scope.v1.json/equivalents.v1.json
  # for the mutants lane) and neither half is expensive enough to be worth
  # splitting further.
  cargo test --manifest-path "$root/rust/Cargo.toml" -p fslc-rust \
    --test implementation_mutation_manifest --locked
}

run_operators_lane() {
  # Curated controls. They cover semantic faults that a token-level mutator
  # cannot express and prove that stale seams fail loudly.
  local shard_args=()
  [ -z "$shard_spec" ] || shard_args=(--shard "$shard_spec")
  "$root/tools/run-fault-operators.sh" "${shard_args[@]}"
}

if [ "$lane" != "operators" ]; then
  if [ "$(cargo mutants --version)" != "$runner_version" ]; then
    echo "semantic-mutation: requires exactly $runner_version; observed $(cargo mutants --version 2>&1)" >&2
    exit 1
  fi
fi

run_manifest_test

if [ "$lane" = "operators" ]; then
  run_operators_lane
  echo "semantic-mutation: mode=$mode lane=operators shard=${shard_spec:-1/1} complete"
  exit 0
fi

if [ "$lane" != "mutants" ]; then
  run_operators_lane
fi

# Prior invocations' evidence directories and scratch build trees can survive
# into this checkout by way of a restored `Swatinem/rust-cache` entry: the
# cache saves `rust/target` wholesale, so anything left there by a past run
# reappears here before this run creates its own differently-named copy.
# Clear them before this run's own evidence exists, so a restored cache never
# carries more than one run's worth of this script's output forward into the
# next save (docs/DESIGN-ci.md, "Actions cache budget").
rm -rf "$root"/rust/target/semantic-mutation.* "$root/rust/target/semantic-mutation-build"

mkdir -p "$root/rust/target"
output="$(mktemp -d "$root/rust/target/semantic-mutation.${mode}.XXXXXX")"
scratch="$output/checkout"
mkdir -p "$scratch"
rsync -a --delete \
  --exclude=target/ --exclude=.git --exclude=node_modules/ \
  "$root/" "$scratch/"
git -C "$scratch" init --quiet
mkdir -p "$scratch/rust/target"
# A target shared by disposable checkouts can retain a mutant artifact whose
# source timestamp is newer than the freshly copied baseline. Give each run a
# fresh build directory, so the baseline can never reuse another run's
# mutation. This scratch tree is disposable working state, not evidence: it
# sits outside `rust/target` (unlike the `$output` evidence directory above,
# which the artifact-upload glob `rust/target/semantic-mutation.*/**` in
# ci.yml requires), so `Swatinem/rust-cache` never saves it and a run-scoped
# name here does not accumulate as cache dead weight the way it did living
# under `rust/target/semantic-mutation-build` (docs/DESIGN-ci.md, "Actions
# cache budget").
export CARGO_TARGET_DIR
CARGO_TARGET_DIR="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/fsl-semantic-mutation-build.XXXXXX")"
args=(
  --config .cargo/mutants.toml
  --in-place
  --build-timeout 600
  --timeout 120
  --no-shuffle
  --no-times
  --output "$output"
)
diff_args=()
scope="$root/rust/fslc/tests/implementation_mutation/scope.v1.json"
equivalents="$root/rust/fslc/tests/implementation_mutation/equivalents.v1.json"

if [ "$mode" = changed ]; then
  diff_file="${FSL_MUTATION_DIFF:-}"
  if [ -z "$diff_file" ] || [ ! -f "$diff_file" ]; then
    echo "semantic-mutation: changed mode requires an FSL_MUTATION_DIFF file" >&2
    exit 1
  fi
  diff_args=(--in-diff "$diff_file")
fi

# cargo-mutants intentionally emits no output directory when a diff intersects
# no selected source mutation. That is a valid changed-tier result only after
# the declared detector tests pass on the unmutated tree, and it still needs a
# complete, revision-bound report rather than silently disappearing.
if [ "$mode" = changed ]; then
  filtered="$output/filtered-mutants.txt"
  (
    cd "$scratch/rust"
    cargo mutants "${args[@]}" "${diff_args[@]}" --list
  ) >"$filtered"
  if [ ! -s "$filtered" ]; then
    changed_scope_paths="$(comm -12 \
      <(sed -n 's|^+++ b/||p' "$diff_file" | sort -u) \
      <(jq -r '.decisions[].path | sub("^rust/"; "")' "$scope" | sort -u))"
    if [ -z "$changed_scope_paths" ]; then
      (
        cd "$scratch/rust"
        cargo test --locked -p fsl-verifier -p fslc-rust \
          --test solver_fail_closed --test triangulated_assurance
      )
      paths_json="$(sed -n 's|^+++ b/||p' "$diff_file" | sort -u | jq -Rsc 'split("\n") | map(select(length > 0) | "rust/" + .)')"
      jq -n \
        --arg revision "$(git -C "$root" rev-parse HEAD)" \
        --arg diff_base "${FSL_MUTATION_BASE:-unknown}" \
        --argjson paths "$paths_json" \
        '{
          schema: "fslc.implementation-mutation-report.v1",
          schema_version: 1,
          base_revision: $revision,
          diff_scope: {mode: "changed", base: $diff_base, paths: $paths},
          runner: {name: "cargo-mutants", version: "27.1.0"},
          configuration: "rust/.cargo/mutants.toml",
          complete: true,
          mutants: []
        }' >"$output/implementation-mutation-report.v1.json"
      FSL_IMPLEMENTATION_MUTATION_REPORT="$output/implementation-mutation-report.v1.json" \
        cargo test --manifest-path "$root/rust/Cargo.toml" -p fslc-rust \
          --test implementation_mutation_manifest --locked \
          emitted_report_matches_schema_when_requested -- --exact
      printf 'semantic-mutation: mode=%s mutants=0 report=%s\n' "$mode" "$output"
      if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf 'report=%s\n' "$output" >>"$GITHUB_OUTPUT"
      fi
      exit 0
    fi
    echo "semantic-mutation: changed critical path produced no line mutant; escalating to complete P2 scope"
  else
    args+=("${diff_args[@]}")
  fi
fi

set +e
(
  cd "$scratch/rust"
  cargo mutants "${args[@]}"
)
runner_status=$?
set -e
case "$runner_status" in
  0|2|3) ;;
  *)
    echo "semantic-mutation: cargo-mutants failed before classifiable outcomes (exit $runner_status)" >&2
    exit "$runner_status"
    ;;
esac

# A missing or incomplete outcome is a failed gate, even if the runner process
# happened to return zero. The raw directory is the reproducible report: it
# includes the exact command, logs, mutants, and machine-readable outcomes.
raw="$output/mutants.out"
for required in outcomes.json mutants.json; do
  if [ ! -s "$raw/$required" ]; then
    echo "semantic-mutation: runner omitted $required from $raw" >&2
    exit 1
  fi
done
for required in caught.txt missed.txt timeout.txt unviable.txt; do
  if [ ! -f "$raw/$required" ]; then
    echo "semantic-mutation: runner omitted $required from $raw" >&2
    exit 1
  fi
done

unknown_outcomes="$(jq '[
  .outcomes[]
  | select(.scenario != "Baseline")
  | .summary as $summary
  | select((["CaughtMutant", "MissedMutant", "Timeout", "Unviable"] | index($summary)) == null)
] | length' "$raw/outcomes.json")"
if [ "$unknown_outcomes" -ne 0 ]; then
  echo "semantic-mutation: runner emitted $unknown_outcomes unknown outcome classification(s)" >&2
  exit 1
fi
timeout_outcomes="$(jq '[.outcomes[] | select(.summary == "Timeout")] | length' "$raw/outcomes.json")"
timeout_list="$(awk 'NF { count++ } END { print count + 0 }' "$raw/timeout.txt")"
if [ "$timeout_outcomes" -ne "$timeout_list" ]; then
  echo "semantic-mutation: timeout outcomes ($timeout_outcomes) disagree with timeout.txt ($timeout_list)" >&2
  exit 1
fi

base_revision="$(git -C "$root" rev-parse HEAD)"
if [ "$mode" = changed ]; then
  diff_base="${FSL_MUTATION_BASE:-unknown}"
  paths_json="$(sed -n 's|^+++ b/||p' "$diff_file" | sort -u | jq -Rsc 'split("\n") | map(select(length > 0) | "rust/" + .)')"
else
  diff_base=""
  paths_json="$(jq -c '[.decisions[].path] | unique' "$scope")"
fi

if [ "$mode" = complete ]; then
  stale_equivalents="$(jq --slurpfile equivalents "$equivalents" '
    . as $root
    | [
        $equivalents[0].entries[].mutant_id
        | select(. as $id | ([$root.outcomes[] | select(.scenario != "Baseline") | .scenario.Mutant.name] | index($id)) == null)
      ]
    | length
  ' "$raw/outcomes.json")"
  if [ "$stale_equivalents" -ne 0 ]; then
    echo "semantic-mutation: $stale_equivalents reviewed-equivalent mutant ID(s) are stale" >&2
    exit 1
  fi
fi

# Resolve every runtime mutant back to the manifest's stable decision ID. The
# source lines are derived from exact maintained anchors at run time, so line
# movement cannot silently turn a decision ID into a bare function name.
decision_anchors='[]'
while IFS= read -r decision; do
  decision_id="$(jq -r '.id' <<<"$decision")"
  decision_path="$(jq -r '.path' <<<"$decision")"
  decision_function="$(jq -r '.function' <<<"$decision")"
  decision_anchor="$(jq -r '.anchor' <<<"$decision")"
  decision_line="$(jq -nr \
    --rawfile source "$root/$decision_path" \
    --arg anchor "$decision_anchor" '
      ($source | index($anchor)) as $offset
      | if $offset == null then error("decision anchor is stale")
        else ($source[0:$offset] | split("\n") | length)
        end
    ')"
  decision_anchors="$(jq -c \
    --arg id "$decision_id" \
    --arg path "${decision_path#rust/}" \
    --arg function "$decision_function" \
    --argjson line "$decision_line" \
    '. + [{id: $id, path: $path, function: $function, line: $line}]' \
    <<<"$decision_anchors")"
done < <(jq -c '.decisions[]' "$scope")

jq \
  --arg revision "$base_revision" \
  --arg mode "$mode" \
  --arg diff_base "$diff_base" \
  --argjson paths "$paths_json" \
  --argjson decision_anchors "$decision_anchors" \
  --slurpfile equivalents "$equivalents" '
  def decision_anchor($mutant):
    ([
      $decision_anchors[]
      | select(.path == $mutant.file and .function == $mutant.function.function_name)
    ]) as $matches
    | if ($matches | length) == 0 then
        error("mutant has no maintained source decision anchor: " + $mutant.name)
      elif ($matches | length) == 1 then
        $matches[0].id
      else
        (([
          $matches[] | select(.line <= $mutant.span.start.line)
        ] | sort_by(.line) | last) // ($matches | sort_by(.line) | first)).id
      end;
  {
    schema: "fslc.implementation-mutation-report.v1",
    schema_version: 1,
    base_revision: $revision,
    diff_scope: {
      mode: $mode,
      base: (if $mode == "changed" then $diff_base else null end),
      paths: $paths
    },
    runner: {name: "cargo-mutants", version: .cargo_mutants_version},
    configuration: "rust/.cargo/mutants.toml",
    complete: (
      .end_time != null and
      .total_mutants == (.caught + .missed + .timeout + .unviable + .success)
    ),
    mutants: [
      .outcomes[]
      | select(.scenario != "Baseline")
      | .scenario.Mutant as $mutant
      | {
          id: $mutant.name,
          source_decision_anchor: decision_anchor($mutant),
          # Deletion mutants legitimately use an empty replacement string.
          # The report contract requires a non-empty reviewable description,
          # so use the stable runner name for both null and empty replacements.
          mutation: (
            if (($mutant.replacement // "") | length) > 0 then $mutant.replacement
            else $mutant.name
            end
          ),
          test_command: (([.phase_results[] | select(.phase == "Test") | .argv] | last) // (.phase_results[-1].argv)),
          classification: (
            if .summary == "CaughtMutant" then "killed"
            elif .summary == "MissedMutant" and (($equivalents[0].entries | map(.mutant_id) | index($mutant.name)) != null) then "reviewed_equivalent"
            elif .summary == "MissedMutant" then "survived"
            elif .summary == "Timeout" then "timeout"
            elif .summary == "Unviable" then "unbuildable"
            else error("unknown cargo-mutants outcome: " + (.summary | tostring))
            end
          ),
          elapsed_ms: ([.phase_results[].duration] | add * 1000 | floor),
          primary_failing_test: null,
          reproducer: ("cargo mutants --config .cargo/mutants.toml --in-place --no-shuffle --re " + ($mutant.name | @sh)),
          reviewed_rationale: (($equivalents[0].entries[]? | select(.mutant_id == $mutant.name) | .rationale) // null)
        }
    ]
  }' "$raw/outcomes.json" >"$output/implementation-mutation-report.v1.json"

# cargo-mutants keeps the exact test process in outcomes.json and the named
# failing tests in each mutant's log. A caught classification without the
# detector that actually failed is not reviewable mutation evidence, so enrich
# every killed row and fail closed if the runner log cannot supply one.
while IFS=$'\t' read -r mutant_index log_path; do
  case "$log_path" in
    log/*) ;;
    *)
      echo "semantic-mutation: caught mutant has unsafe or missing log path '$log_path'" >&2
      exit 1
      ;;
  esac
  primary_failing_test="$(
    awk '/^test .* \.\.\. FAILED$/ {
      sub(/^test /, "");
      sub(/ \.\.\. FAILED$/, "");
      print;
      exit
    }' "$raw/$log_path"
  )"
  if [ -z "$primary_failing_test" ]; then
    echo "semantic-mutation: caught mutant log '$log_path' names no failing test" >&2
    exit 1
  fi
  report_tmp="$output/implementation-mutation-report.v1.json.tmp"
  jq \
    --argjson mutant_index "$mutant_index" \
    --arg primary_failing_test "$primary_failing_test" \
    '.mutants[$mutant_index].primary_failing_test = $primary_failing_test' \
    "$output/implementation-mutation-report.v1.json" >"$report_tmp"
  mv "$report_tmp" "$output/implementation-mutation-report.v1.json"
done < <(
  jq -r '
    [.outcomes[] | select(.scenario != "Baseline")]
    | to_entries[]
    | select(.value.summary == "CaughtMutant")
    | [.key, .value.log_path]
    | @tsv
  ' "$raw/outcomes.json"
)

FSL_IMPLEMENTATION_MUTATION_REPORT="$output/implementation-mutation-report.v1.json" \
  cargo test --manifest-path "$root/rust/Cargo.toml" -p fslc-rust \
    --test implementation_mutation_manifest --locked \
    emitted_report_matches_schema_when_requested -- --exact

unreviewed_survivors="$(jq --slurpfile equivalents "$equivalents" '[
  .outcomes[]
  | select(.summary == "MissedMutant")
  | .scenario.Mutant.name as $id
  | select(($equivalents[0].entries | map(.mutant_id) | index($id)) == null)
] | length' "$raw/outcomes.json")"

if [ "$unreviewed_survivors" -ne 0 ] || [ "$timeout_outcomes" -ne 0 ]; then
  echo "semantic-mutation: surviving or timed-out mutant in $raw" >&2
  exit 1
fi

printf 'semantic-mutation: mode=%s report=%s\n' "$mode" "$output"
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  printf 'report=%s\n' "$output" >>"$GITHUB_OUTPUT"
fi
