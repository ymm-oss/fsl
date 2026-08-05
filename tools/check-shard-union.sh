#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Generic, line-set based shard-completeness guard (issue: CI wall-clock
# reduction via job splitting; see docs/DESIGN-ci.md, "Sharded pre-merge Linux
# evidence"). A scheduling change that splits one set of tests/operators/
# mutants across N shards must prove, mechanically, that the shards are a
# partition of the original set: every entry lands in exactly one shard, none
# is dropped, none is duplicated, and none is invented. Sharding is purely a
# scheduling change; it must never silently narrow what runs.
#
# Usage:
#   check-shard-union.sh <full-list> <shard-list>...
#     Fails closed, naming the offending entries, unless:
#       - every named file exists and is non-empty
#       - each shard is a subset of full
#       - shards are pairwise disjoint
#       - the union of all shards equals full exactly
#
#   check-shard-union.sh selftest
#     Exercises an accepting case (a clean N-way split) and four rejecting
#     cases (a full-only entry covered by no shard; an entry duplicated across
#     two shards; a shard entry absent from full; an empty shard list), each of
#     which must make this script exit non-zero.

set -euo pipefail

fail() {
  echo "check-shard-union: $*" >&2
  exit 1
}

# Prints the sorted, de-duplicated, non-blank line set of a file. Fails
# closed if the file is missing or empty -- an absent inventory is not
# evidence of an empty shard, it is evidence the run never produced one.
line_set() {
  local path="$1"
  [ -f "$path" ] || fail "'$path' does not exist"
  [ -s "$path" ] || fail "'$path' is empty"
  sort -u "$path"
}

check_union() {
  local full_path="$1"
  shift
  [ "$#" -gt 0 ] || fail "no shard lists given"

  local full
  full="$(line_set "$full_path")"

  local -a shard_paths=("$@")
  local -a shard_sets=()
  local index
  for index in "${!shard_paths[@]}"; do
    shard_sets+=("$(line_set "${shard_paths[$index]}")")
  done

  # Each shard must be a subset of full.
  for index in "${!shard_paths[@]}"; do
    local extra
    extra="$(comm -23 <(printf '%s\n' "${shard_sets[$index]}") <(printf '%s\n' "$full") || true)"
    if [ -n "$extra" ]; then
      fail "shard '${shard_paths[$index]}' names entries absent from '$full_path': $(printf '%s' "$extra" | tr '\n' ' ')"
    fi
  done

  # Shards must be pairwise disjoint.
  local i j
  for ((i = 0; i < ${#shard_paths[@]}; i++)); do
    for ((j = i + 1; j < ${#shard_paths[@]}; j++)); do
      local overlap
      overlap="$(comm -12 <(printf '%s\n' "${shard_sets[$i]}") <(printf '%s\n' "${shard_sets[$j]}") || true)"
      if [ -n "$overlap" ]; then
        fail "shard '${shard_paths[$i]}' and shard '${shard_paths[$j]}' both name: $(printf '%s' "$overlap" | tr '\n' ' ')"
      fi
    done
  done

  # The union of every shard must equal full exactly -- no entry left uncovered.
  local union
  union="$(printf '%s\n' "${shard_sets[@]}" | sort -u)"
  local missing
  missing="$(comm -23 <(printf '%s\n' "$full") <(printf '%s\n' "$union") || true)"
  if [ -n "$missing" ]; then
    fail "$(printf '%s' "$missing" | grep -c . ) entr$([ "$(printf '%s' "$missing" | grep -c .)" = 1 ] && echo y || echo ies) in '$full_path' covered by no shard (silent coverage loss): $(printf '%s' "$missing" | tr '\n' ' ')"
  fi

  local extra_union
  extra_union="$(comm -13 <(printf '%s\n' "$full") <(printf '%s\n' "$union") || true)"
  if [ -n "$extra_union" ]; then
    fail "union names entries absent from '$full_path': $(printf '%s' "$extra_union" | tr '\n' ' ')"
  fi

  echo "check-shard-union: PASS -- $(printf '%s\n' "$full" | grep -c .) entries in '$full_path', $(( ${#shard_paths[@]} )) shard(s), union matches exactly"
}

selftest() {
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  # Accepting: a clean 3-way split of a 6-entry full list.
  printf 'a\nb\nc\nd\ne\nf\n' >"$tmp/full.txt"
  printf 'a\nb\n' >"$tmp/shard1.txt"
  printf 'c\nd\n' >"$tmp/shard2.txt"
  printf 'e\nf\n' >"$tmp/shard3.txt"
  # check_union calls fail(), which calls `exit`, an unconditional process
  # exit that `if`'s condition protection does not catch. Every invocation
  # below therefore runs in its own subshell so a rejecting case's `exit`
  # only ends that subshell, not this selftest.
  if ! (check_union "$tmp/full.txt" "$tmp/shard1.txt" "$tmp/shard2.txt" "$tmp/shard3.txt") >/dev/null; then
    echo "check-shard-union selftest: FAIL: accepting case was rejected" >&2
    return 1
  fi

  # Rejecting (a): an entry in full covered by no shard.
  printf 'a\nb\nc\nd\ne\n' >"$tmp/shard1-missing.txt"
  if (check_union "$tmp/full.txt" "$tmp/shard1-missing.txt" "$tmp/shard2.txt") >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: silent coverage loss was accepted" >&2
    return 1
  fi

  # Rejecting (b): an entry duplicated across two shards.
  printf 'a\nb\nc\n' >"$tmp/shard1-dup.txt"
  printf 'c\nd\ne\nf\n' >"$tmp/shard2-dup.txt"
  if (check_union "$tmp/full.txt" "$tmp/shard1-dup.txt" "$tmp/shard2-dup.txt") >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: duplicate entry across shards was accepted" >&2
    return 1
  fi

  # Rejecting (c): a shard entry absent from full.
  printf 'a\nb\nzz\n' >"$tmp/shard1-extra.txt"
  printf 'c\nd\ne\nf\n' >"$tmp/shard2-full.txt"
  if (check_union "$tmp/full.txt" "$tmp/shard1-extra.txt" "$tmp/shard2-full.txt") >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: invented shard entry was accepted" >&2
    return 1
  fi

  # Rejecting (d): an empty shard list (no shard arguments at all).
  if (check_union "$tmp/full.txt") >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: empty shard list was accepted" >&2
    return 1
  fi

  echo "check-shard-union selftest: all assertions passed"
}

case "${1:-}" in
  selftest)
    selftest
    ;;
  "")
    echo "usage: $0 <full-list> <shard-list>... | $0 selftest" >&2
    exit 2
    ;;
  *)
    check_union "$@"
    ;;
esac
