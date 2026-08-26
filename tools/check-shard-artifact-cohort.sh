#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "check-shard-artifact-cohort.sh requires Bash 4 or newer" >&2; exit 1; }
# SPDX-License-Identifier: Apache-2.0

# Validate one downloaded cohort of stable-name sharded artifacts before the
# lane-specific exact-union check. Mixed attempts are intentional: a partial
# rerun may produce [N,N+1,N], but every artifact must belong to this run and
# revision, identify the expected logical shard, and match its payload hashes.

set -euo pipefail

script_dir="$(dirname "$0")" || {
  echo "check-shard-artifact-cohort: script directory resolution failed" >&2
  exit 1
}
root="$(cd "$script_dir/.." && pwd)" || {
  echo "check-shard-artifact-cohort: repository root resolution failed" >&2
  exit 1
}

fail() {
  echo "check-shard-artifact-cohort: $*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

canonical_array() {
  local manifest="$1" field="$2" destination="$3"
  jq -er --arg field "$field" '.[$field] | if type == "array" and all(.[]; type == "string") then .[] else error($field + " must be an array of strings") end' \
    "$manifest" | sort -u >"$destination" || fail "'$manifest' field '$field' must be an array of strings"
  [ -s "$destination" ] || fail "'$manifest' field '$field' produced an empty canonical payload"
}

expect_equal() {
  local label="$1" expected="$2" actual="$3" artifact="$4"
  [ "$actual" = "$expected" ] || fail "$artifact: $label mismatch: expected '$expected', actual '$actual'"
}

enumerate_sorted_paths() {
  local destination="$1" label="$2"
  shift 2
  local unsorted="$destination.unsorted"
  find "$@" -print0 >"$unsorted" || fail "$label enumeration failed"
  sort -z "$unsorted" >"$destination" || fail "$label sorting failed"
}

checked_sort_file() {
  local source="$1" destination="$2" label="$3"
  shift 3
  sort "$@" "$source" >"$destination" || fail "$label sorting failed"
}

checked_join_file() {
  local source="$1" destination="$2" delimiter="$3" label="$4"
  paste -sd "$delimiter" "$source" >"$destination" || fail "$label joining failed"
}

check_cohort() {
  local mode="$1" cohort="$2" expected_run_id="$3" current_attempt="$4" expected_revision="$5" expected_total="$6"
  [[ "$mode" = rust || "$mode" = semantic ]] || fail "mode must be 'rust' or 'semantic', got '$mode'"
  [ -d "$cohort" ] || fail "cohort directory '$cohort' does not exist"
  [[ "$expected_run_id" =~ ^[1-9][0-9]*$ ]] || fail "expected run_id must be a positive integer, got '$expected_run_id'"
  [[ "$current_attempt" =~ ^[1-9][0-9]*$ ]] || fail "current attempt must be a positive integer, got '$current_attempt'"
  [[ "$expected_total" =~ ^[1-9][0-9]*$ ]] || fail "expected shard total must be a positive integer, got '$expected_total'"

  local expected_lane prefix
  if [ "$mode" = rust ]; then
    expected_lane="rust-tests"
    prefix="rust-test-shard"
  else
    expected_lane="semantic-mutation-operators"
    prefix="semantic-mutation-operators"
  fi

  local work
  work="$(mktemp -d)" || fail "$expected_lane: temporary directory creation failed"
  local -a artifact_dirs=() recursive_provenances=() provenances=()
  enumerate_sorted_paths "$work/artifact-directories" "$expected_lane: artifact directory" \
    "$cohort" -mindepth 1 -maxdepth 1 -type d
  mapfile -d '' -t artifact_dirs <"$work/artifact-directories" \
    || fail "$expected_lane: artifact directory result read failed"
  if [ "${#artifact_dirs[@]}" -ne "$expected_total" ]; then
    fail "$expected_lane: expected $expected_total artifact directories, found ${#artifact_dirs[@]}"
  fi
  enumerate_sorted_paths "$work/recursive-provenances" "$expected_lane: recursive provenance" \
    "$cohort" -type f -name artifact-provenance.v1.json
  mapfile -d '' -t recursive_provenances <"$work/recursive-provenances" \
    || fail "$expected_lane: recursive provenance result read failed"
  if [ "${#recursive_provenances[@]}" -ne "$expected_total" ]; then
    fail "$expected_lane: expected $expected_total provenance sidecars, found ${#recursive_provenances[@]}"
  fi

  local -a seen=() attempts=() fulls=() shards=() manifests=()
  local artifact_dir artifact_ordinal=0
  for artifact_dir in "${artifact_dirs[@]}"; do
    artifact_ordinal=$((artifact_ordinal + 1))
    local artifact_name
    artifact_name="$(basename "$artifact_dir")" \
      || fail "$expected_lane: artifact name extraction failed"
    if [[ ! "$artifact_name" =~ ^${prefix}-([1-9][0-9]*)-${expected_run_id}$ ]]; then
      fail "$artifact_name: artifact_name mismatch: expected '$prefix-<shard.index>-$expected_run_id', actual '$artifact_name'"
    fi

    local -a direct_provenances=()
    enumerate_sorted_paths "$work/direct-provenances-$artifact_ordinal" "$artifact_name: direct provenance" \
      "$artifact_dir" -mindepth 1 -maxdepth 1 -type f -name artifact-provenance.v1.json
    mapfile -d '' -t direct_provenances <"$work/direct-provenances-$artifact_ordinal" \
      || fail "$artifact_name: direct provenance result read failed"
    if [ "${#direct_provenances[@]}" -ne 1 ]; then
      fail "$artifact_name: expected exactly 1 direct provenance sidecar, found ${#direct_provenances[@]}"
    fi
    local provenance="${direct_provenances[0]}"
    provenances+=("$provenance")

    local provenance_row
    provenance_row="$(jq -er '
      select(
        type == "object" and
        .schema == "fslc.shard-artifact-provenance.v1" and
        (.lane | type == "string") and
        (.run_id | type == "string" and test("^[1-9][0-9]*$")) and
        (.run_attempt | type == "number" and floor == .) and
        (.head_revision | type == "string" and length > 0) and
        (.shard | type == "object") and
        (.shard.index | type == "number" and floor == .) and
        (.shard.total | type == "number" and floor == .) and
        (.full_sha256 | type == "string" and test("^[0-9a-f]{64}$")) and
        (.shard_sha256 | type == "string" and test("^[0-9a-f]{64}$"))
      ) |
      [.lane, .run_id, .run_attempt, .head_revision, .shard.index, .shard.total,
       .full_sha256, .shard_sha256] | @tsv
    ' "$provenance")" || fail "'$provenance' does not match provenance schema fslc.shard-artifact-provenance.v1"

    local lane run_id attempt revision index total expected_full_hash expected_shard_hash expected_name
    IFS=$'\t' read -r lane run_id attempt revision index total expected_full_hash expected_shard_hash <<<"$provenance_row"
    expected_name="$prefix-$index-$expected_run_id"

    expect_equal lane "$expected_lane" "$lane" "$artifact_name"
    expect_equal run_id "$expected_run_id" "$run_id" "$artifact_name"
    expect_equal head_revision "$expected_revision" "$revision" "$artifact_name"
    expect_equal shard.total "$expected_total" "$total" "$artifact_name"
    if [[ "$index" =~ ^[1-9][0-9]*$ ]] && [ "$index" -le "$expected_total" ]; then
      :
    else
      fail "$artifact_name: shard.index out of range: expected '1..$expected_total', actual '$index'"
    fi
    expect_equal artifact_name "$expected_name" "$artifact_name" "$artifact_name"
    if [ "$attempt" -ge 1 ] && [ "$attempt" -le "$current_attempt" ]; then
      :
    else
      fail "$artifact_name: run_attempt out of range: expected '1..$current_attempt', actual '$attempt'"
    fi
    if [[ " ${seen[*]-} " = *" $index "* ]]; then
      fail "$artifact_name: duplicate shard.index: expected unique '1..$expected_total', actual '$index'"
    fi
    seen+=("$index")
    attempts+=("$attempt")

    local full_payload shard_payload
    if [ "$mode" = rust ]; then
      full_payload="$artifact_dir/full.txt"
      shard_payload="$artifact_dir/shard.txt"
      [ -s "$full_payload" ] || fail "$artifact_name: expected non-empty payload '$full_payload'"
      [ -s "$shard_payload" ] || fail "$artifact_name: expected non-empty payload '$shard_payload'"
      fulls+=("$full_payload")
      shards+=("$shard_payload")
    else
      local manifest="$artifact_dir/shard-manifest.v1.json"
      [ -s "$manifest" ] || fail "$artifact_name: expected non-empty payload '$manifest'"
      jq -e '
        type == "object" and
        .schema == "fslc.fault-operator-shard-manifest.v1" and
        (.base_revision | type == "string" and length > 0) and
        (.shard.index | type == "number" and floor == .) and
        (.shard.total | type == "number" and floor == .) and
        (.table_operators | type == "array" and length > 0 and all(.[]; type == "string")) and
        (.executed_operators | type == "array" and length > 0 and all(.[]; type == "string"))
      ' "$manifest" >/dev/null || fail "'$manifest' does not match semantic shard-manifest schema"
      local manifest_index manifest_total
      manifest_index="$(jq -r '.shard.index' "$manifest")" \
        || fail "'$manifest' shard.index extraction failed"
      manifest_total="$(jq -r '.shard.total' "$manifest")" \
        || fail "'$manifest' shard.total extraction failed"
      expect_equal manifest.shard.index "$index" "$manifest_index" "$artifact_name"
      expect_equal manifest.shard.total "$total" "$manifest_total" "$artifact_name"
      full_payload="$work/full-$index.txt"
      shard_payload="$work/shard-$index.txt"
      canonical_array "$manifest" table_operators "$full_payload"
      canonical_array "$manifest" executed_operators "$shard_payload"
      fulls+=("$full_payload")
      shards+=("$shard_payload")
      manifests+=("$manifest")
    fi

    local actual_full_hash actual_shard_hash
    actual_full_hash="$(sha256_file "$full_payload")" \
      || fail "'$full_payload' hashing failed"
    actual_shard_hash="$(sha256_file "$shard_payload")" \
      || fail "'$shard_payload' hashing failed"
    expect_equal full_sha256 "$expected_full_hash" "$actual_full_hash" "$artifact_name"
    expect_equal shard_sha256 "$expected_shard_hash" "$actual_shard_hash" "$artifact_name"
  done

  printf '%s\0' "${provenances[@]}" >"$work/selected-provenances.unsorted" \
    || fail "$expected_lane: selected direct provenance serialization failed"
  checked_sort_file "$work/selected-provenances.unsorted" "$work/selected-provenances" \
    "$expected_lane: selected direct provenance" -z -u
  if ! cmp -s "$work/recursive-provenances" "$work/selected-provenances"; then
    fail "$expected_lane: provenance sidecar set mismatch: recursive cohort set must equal selected direct sidecar set"
  fi

  printf '%s\n' "${seen[@]}" >"$work/seen-indices.unsorted" \
    || fail "$expected_lane: observed shard index serialization failed"
  checked_sort_file "$work/seen-indices.unsorted" "$work/seen-indices" \
    "$expected_lane: observed shard index" -n
  checked_join_file "$work/seen-indices" "$work/seen-indices.csv" , \
    "$expected_lane: observed shard index"
  local sorted_indices
  IFS= read -r sorted_indices <"$work/seen-indices.csv" \
    || fail "$expected_lane: observed shard index result read failed"
  seq 1 "$expected_total" >"$work/expected-indices" \
    || fail "$expected_lane: expected shard index generation failed"
  checked_join_file "$work/expected-indices" "$work/expected-indices.csv" , \
    "$expected_lane: expected shard index"
  local expected_indices
  IFS= read -r expected_indices <"$work/expected-indices.csv" \
    || fail "$expected_lane: expected shard index result read failed"
  expect_equal shard.indices "$expected_indices" "$sorted_indices" "$expected_lane"

  local first_full_hash index
  first_full_hash="$(sha256_file "${fulls[0]}")" \
    || fail "'${fulls[0]}' baseline full-universe hashing failed"
  for index in "${!fulls[@]}"; do
    local actual_universe_hash universe_parent universe_artifact
    actual_universe_hash="$(sha256_file "${fulls[$index]}")" \
      || fail "'${fulls[$index]}' full-universe hashing failed"
    universe_parent="${fulls[$index]%/*}"
    universe_artifact="${universe_parent##*/}"
    expect_equal full_universe_sha256 "$first_full_hash" "$actual_universe_hash" "$universe_artifact"
  done

  if [ "$mode" = rust ]; then
    for index in 1 2; do
      if ! cmp -s "${fulls[0]}" "${fulls[$index]}"; then
        fail "rust-tests: full payload byte mismatch: expected '${fulls[0]}', actual '${fulls[$index]}'"
      fi
    done
  else
    local expected_base expected_table_operators
    expected_base="$(jq -er '.base_revision | select(type == "string" and length > 0)' "${manifests[0]}")" \
      || fail "'${manifests[0]}' has invalid base_revision"
    expected_table_operators="$(jq -ec '.table_operators' "${manifests[0]}")" \
      || fail "'${manifests[0]}' has invalid table_operators"
    for index in "${!manifests[@]}"; do
      local actual_base actual_table_operators
      actual_base="$(jq -er '.base_revision | select(type == "string" and length > 0)' "${manifests[$index]}")" \
        || fail "'${manifests[$index]}' has invalid base_revision"
      actual_table_operators="$(jq -ec '.table_operators' "${manifests[$index]}")" \
        || fail "'${manifests[$index]}' has invalid table_operators"
      local manifest_parent manifest_artifact
      manifest_parent="${manifests[$index]%/*}"
      manifest_artifact="${manifest_parent##*/}"
      expect_equal base_revision "$expected_base" "$actual_base" "$manifest_artifact"
      expect_equal table_operators "$expected_table_operators" "$actual_table_operators" "$manifest_artifact"
    done
  fi

  "$root/tools/check-shard-union.sh" "${fulls[0]}" "${shards[@]}"
  printf '%s\n' "${attempts[@]}" >"$work/attempts" \
    || fail "$expected_lane: attempt serialization failed"
  FSL_COHORT_JOIN_CONTEXT=attempts \
    checked_join_file "$work/attempts" "$work/attempts.csv" , "$expected_lane: attempt display"
  local attempts_csv
  IFS= read -r attempts_csv <"$work/attempts.csv" \
    || fail "$expected_lane: attempt display result read failed"
  echo "check-shard-artifact-cohort: PASS -- lane=$expected_lane run_id=$expected_run_id attempts=$attempts_csv shards=$expected_indices"
  rm -rf "$work"
}

write_provenance() {
  local artifact="$1" lane="$2" run_id="$3" attempt="$4" revision="$5" index="$6" total="$7" full="$8" shard="$9"
  local full_sha256 shard_sha256
  full_sha256="$(sha256_file "$full")" || fail "'$full' provenance hashing failed"
  shard_sha256="$(sha256_file "$shard")" || fail "'$shard' provenance hashing failed"
  jq -n --arg lane "$lane" --arg run_id "$run_id" --argjson attempt "$attempt" \
    --arg revision "$revision" --argjson index "$index" --argjson total "$total" \
    --arg full_sha256 "$full_sha256" --arg shard_sha256 "$shard_sha256" \
    '{schema:"fslc.shard-artifact-provenance.v1", lane:$lane, run_id:$run_id,
      run_attempt:$attempt, head_revision:$revision, shard:{index:$index,total:$total},
      full_sha256:$full_sha256, shard_sha256:$shard_sha256}' >"$artifact/artifact-provenance.v1.json"
}

make_rust_fixture() {
  local directory="$1" attempt_list="$2"
  local -a attempt_values
  IFS=, read -r -a attempt_values <<<"$attempt_list"
  local index
  for index in 1 2 3; do
    local artifact="$directory/rust-test-shard-$index-77"
    mkdir -p "$artifact"
    printf 'a\nb\nc\nd\ne\nf\n' >"$artifact/full.txt"
    case "$index" in
      1) printf 'a\nb\n' >"$artifact/shard.txt" ;;
      2) printf 'c\nd\n' >"$artifact/shard.txt" ;;
      3) printf 'e\nf\n' >"$artifact/shard.txt" ;;
    esac
    write_provenance "$artifact" rust-tests 77 "${attempt_values[$((index - 1))]}" rev-good "$index" 3 "$artifact/full.txt" "$artifact/shard.txt"
  done
}

make_semantic_fixture() {
  local directory="$1" attempt_list="$2"
  local -a attempt_values
  IFS=, read -r -a attempt_values <<<"$attempt_list"
  local index
  for index in 1 2 3; do
    local artifact="$directory/semantic-mutation-operators-$index-77"
    mkdir -p "$artifact"
    local executed
    case "$index" in
      1) executed='["op-a","op-b"]' ;;
      2) executed='["op-c","op-d"]' ;;
      3) executed='["op-e","op-f"]' ;;
    esac
    jq -n --argjson index "$index" --argjson executed "$executed" \
      '{schema:"fslc.fault-operator-shard-manifest.v1",schema_version:1,
        base_revision:"base-good",shard:{index:$index,total:3},
        table_operators:["op-a","op-b","op-c","op-d","op-e","op-f"],
        executed_operators:$executed}' >"$artifact/shard-manifest.v1.json"
    local full="$tmp/semantic-full-$index.txt" shard="$tmp/semantic-shard-$index.txt"
    canonical_array "$artifact/shard-manifest.v1.json" table_operators "$full"
    canonical_array "$artifact/shard-manifest.v1.json" executed_operators "$shard"
    write_provenance "$artifact" semantic-mutation-operators 77 "${attempt_values[$((index - 1))]}" rev-good "$index" 3 "$full" "$shard"
  done
}

expect_rejected() {
  local label="$1" expected="$2"
  shift 2
  local output status=0
  output="$("$@" 2>&1)" || status=$?
  if [ "$status" -eq 0 ]; then
    echo "check-shard-artifact-cohort selftest: FAIL: $label mutation was accepted" >&2
    return 1
  fi
  if [[ "$output" != *"$expected"* ]]; then
    echo "check-shard-artifact-cohort selftest: FAIL: $label produced '$output', expected diagnostic containing '$expected'" >&2
    return 1
  fi
  echo "detector $label: produced='$output'; expected_contains='$expected'"
}

selftest() {
  local tmp
  tmp="$(mktemp -d)" || fail "selftest temporary directory creation failed"
  trap 'chmod u+rwx "$tmp/unreadable-nested/rust-test-shard-1-77/nested" 2>/dev/null || true; chmod -R u+rwx "$tmp" 2>/dev/null || true; rm -rf "$tmp"' RETURN

  make_rust_fixture "$tmp/partial" 1,2,1
  check_cohort rust "$tmp/partial" 77 2 rev-good 3 >/dev/null
  make_rust_fixture "$tmp/current" 2,2,2
  check_cohort rust "$tmp/current" 77 2 rev-good 3 >/dev/null
  make_semantic_fixture "$tmp/semantic-partial" 1,2,1
  check_cohort semantic "$tmp/semantic-partial" 77 2 rev-good 3 >/dev/null
  make_semantic_fixture "$tmp/semantic-current" 2,2,2
  check_cohort semantic "$tmp/semantic-current" 77 2 rev-good 3 >/dev/null

  local real_sort real_paste
  real_sort="$(command -v sort)" || fail "selftest could not resolve sort"
  real_paste="$(command -v paste)" || fail "selftest could not resolve paste"
  mkdir -p "$tmp/failing-sort" "$tmp/failing-paste"
  # shellcheck disable=SC2016 # generated wrapper expands these variables when it runs
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'for argument in "$@"; do' \
    '  if [ "$argument" = "-u" ]; then' \
    '    "$REAL_SORT" "$@"' \
    '    echo "injected comparison sort failure" >&2' \
    '    exit 42' \
    '  fi' \
    'done' \
    'exec "$REAL_SORT" "$@"' >"$tmp/failing-sort/sort" \
    || fail "selftest comparison-sort wrapper creation failed"
  # shellcheck disable=SC2016 # generated wrapper expands these variables when it runs
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'if [ "${FSL_COHORT_JOIN_CONTEXT:-}" = "attempts" ]; then' \
    '  "$REAL_PASTE" "$@"' \
    '  echo "injected attempt paste failure" >&2' \
    '  exit 44' \
    'fi' \
    'exec "$REAL_PASTE" "$@"' >"$tmp/failing-paste/paste" \
    || fail "selftest attempt-paste wrapper creation failed"
  chmod +x "$tmp/failing-sort/sort" "$tmp/failing-paste/paste"
  expect_rejected comparison-sort-failure \
    "rust-tests: selected direct provenance sorting failed" \
    env PATH="$tmp/failing-sort:$PATH" REAL_SORT="$real_sort" \
    "$root/tools/check-shard-artifact-cohort.sh" rust "$tmp/partial" 77 2 rev-good 3
  expect_rejected attempt-paste-failure \
    "rust-tests: attempt display joining failed" \
    env PATH="$tmp/failing-paste:$PATH" REAL_PASTE="$real_paste" \
    "$root/tools/check-shard-artifact-cohort.sh" rust "$tmp/partial" 77 2 rev-good 3

  cp -R "$tmp/partial" "$tmp/missing"
  rm -rf "$tmp/missing/rust-test-shard-3-77"
  expect_rejected missing-shard "expected 3 artifact directories, found 2" check_cohort rust "$tmp/missing" 77 2 rev-good 3

  cp -R "$tmp/partial" "$tmp/checksum"
  printf 'changed\n' >>"$tmp/checksum/rust-test-shard-1-77/full.txt"
  expect_rejected stale-payload-checksum "full_sha256 mismatch: expected" check_cohort rust "$tmp/checksum" 77 2 rev-good 3

  cp -R "$tmp/partial" "$tmp/universe"
  printf 'z\n' >>"$tmp/universe/rust-test-shard-1-77/full.txt"
  write_provenance "$tmp/universe/rust-test-shard-1-77" rust-tests 77 1 rev-good 1 3 \
    "$tmp/universe/rust-test-shard-1-77/full.txt" "$tmp/universe/rust-test-shard-1-77/shard.txt"
  expect_rejected incompatible-stale-universe "full_universe_sha256 mismatch: expected" check_cohort rust "$tmp/universe" 77 2 rev-good 3

  local field expected actual
  for field in lane run_id head_revision; do
    cp -R "$tmp/partial" "$tmp/foreign-$field"
    case "$field" in
      lane) expected="lane mismatch: expected 'rust-tests', actual 'semantic-mutation-operators'"; actual=semantic-mutation-operators ;;
      run_id) expected="run_id mismatch: expected '77', actual '88'"; actual=88 ;;
      head_revision) expected="head_revision mismatch: expected 'rev-good', actual 'rev-foreign'"; actual=rev-foreign ;;
    esac
    jq --arg value "$actual" ".${field} = \$value" "$tmp/foreign-$field/rust-test-shard-1-77/artifact-provenance.v1.json" >"$tmp/mutated.json"
    mv "$tmp/mutated.json" "$tmp/foreign-$field/rust-test-shard-1-77/artifact-provenance.v1.json"
    expect_rejected "foreign-$field" "$expected" check_cohort rust "$tmp/foreign-$field" 77 2 rev-good 3
  done

  cp -R "$tmp/partial" "$tmp/foreign-index"
  jq '.shard.index = 2' "$tmp/foreign-index/rust-test-shard-1-77/artifact-provenance.v1.json" >"$tmp/mutated.json"
  mv "$tmp/mutated.json" "$tmp/foreign-index/rust-test-shard-1-77/artifact-provenance.v1.json"
  expect_rejected foreign-shard-index "artifact_name mismatch: expected 'rust-test-shard-2-77', actual 'rust-test-shard-1-77'" check_cohort rust "$tmp/foreign-index" 77 2 rev-good 3

  cp -R "$tmp/partial" "$tmp/nested-foreign"
  local index
  for index in 1 2 3; do
    mv "$tmp/nested-foreign/rust-test-shard-$index-77" "$tmp/nested-foreign/rust-test-shard-foreign-$index-77"
    mkdir -p "$tmp/nested-foreign/rust-test-shard-foreign-$index-77/rust-test-shard-$index-77"
    find "$tmp/nested-foreign/rust-test-shard-foreign-$index-77" -mindepth 1 -maxdepth 1 \
      ! -name "rust-test-shard-$index-77" \
      -exec mv {} "$tmp/nested-foreign/rust-test-shard-foreign-$index-77/rust-test-shard-$index-77/" \;
  done
  expect_rejected nested-foreign-artifact-name \
    "artifact_name mismatch: expected 'rust-test-shard-<shard.index>-77', actual 'rust-test-shard-foreign-1-77'" \
    check_cohort rust "$tmp/nested-foreign" 77 2 rev-good 3

  cp -R "$tmp/partial" "$tmp/surplus-nested-provenance"
  mkdir -p "$tmp/surplus-nested-provenance/rust-test-shard-1-77/nested"
  jq '.run_id = "88"' \
    "$tmp/surplus-nested-provenance/rust-test-shard-1-77/artifact-provenance.v1.json" \
    >"$tmp/surplus-nested-provenance/rust-test-shard-1-77/nested/artifact-provenance.v1.json"
  expect_rejected surplus-nested-provenance \
    "expected 3 provenance sidecars, found 4" \
    check_cohort rust "$tmp/surplus-nested-provenance" 77 2 rev-good 3

  cp -R "$tmp/partial" "$tmp/unreadable-nested"
  mkdir -p "$tmp/unreadable-nested/rust-test-shard-1-77/nested"
  cp "$tmp/unreadable-nested/rust-test-shard-1-77/artifact-provenance.v1.json" \
    "$tmp/unreadable-nested/rust-test-shard-1-77/nested/artifact-provenance.v1.json"
  chmod 000 "$tmp/unreadable-nested/rust-test-shard-1-77/nested"
  expect_rejected unreadable-nested-provenance \
    "rust-tests: recursive provenance enumeration failed" \
    check_cohort rust "$tmp/unreadable-nested" 77 2 rev-good 3
  chmod u+rwx "$tmp/unreadable-nested/rust-test-shard-1-77/nested"

  cp -R "$tmp/partial" "$tmp/future"
  jq '.run_attempt = 3' "$tmp/future/rust-test-shard-2-77/artifact-provenance.v1.json" >"$tmp/mutated.json"
  mv "$tmp/mutated.json" "$tmp/future/rust-test-shard-2-77/artifact-provenance.v1.json"
  expect_rejected future-attempt "run_attempt out of range: expected '1..2', actual '3'" check_cohort rust "$tmp/future" 77 2 rev-good 3

  cp -R "$tmp/semantic-partial" "$tmp/semantic-base"
  jq '.base_revision = "base-foreign"' "$tmp/semantic-base/semantic-mutation-operators-2-77/shard-manifest.v1.json" >"$tmp/mutated.json"
  mv "$tmp/mutated.json" "$tmp/semantic-base/semantic-mutation-operators-2-77/shard-manifest.v1.json"
  expect_rejected semantic-base-revision "base_revision mismatch: expected 'base-good', actual 'base-foreign'" check_cohort semantic "$tmp/semantic-base" 77 2 rev-good 3

  cp -R "$tmp/semantic-partial" "$tmp/semantic-table-exact"
  jq '.table_operators += ["op-a"]' \
    "$tmp/semantic-table-exact/semantic-mutation-operators-2-77/shard-manifest.v1.json" >"$tmp/mutated.json"
  mv "$tmp/mutated.json" "$tmp/semantic-table-exact/semantic-mutation-operators-2-77/shard-manifest.v1.json"
  expect_rejected semantic-table-operators-exact \
    "table_operators mismatch: expected '[\"op-a\",\"op-b\",\"op-c\",\"op-d\",\"op-e\",\"op-f\"]', actual '[\"op-a\",\"op-b\",\"op-c\",\"op-d\",\"op-e\",\"op-f\",\"op-a\"]'" \
    check_cohort semantic "$tmp/semantic-table-exact" 77 2 rev-good 3

  echo "check-shard-artifact-cohort selftest: all assertions passed"
}

case "${1:-}" in
  rust|semantic)
    [ "$#" -eq 6 ] || fail "usage: $0 {rust|semantic} <cohort-dir> <run-id> <current-attempt> <head-revision> <shard-total>"
    check_cohort "$@"
    ;;
  selftest)
    [ "$#" -eq 1 ] || fail "selftest takes no arguments"
    selftest
    ;;
  *)
    fail "usage: $0 {rust|semantic} <cohort-dir> <run-id> <current-attempt> <head-revision> <shard-total> | selftest"
    ;;
esac
