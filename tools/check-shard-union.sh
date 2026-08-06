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
#   check-shard-union.sh check-groups <groups-file> <known-binaries-file> <shard-total>
#     Validates a checked-in duration-aware binary->shard pinning file (e.g.
#     tools/rust-test-shard-groups.txt) against the live set of binary IDs
#     cargo-nextest actually reports, and against itself. Fails closed unless:
#       - every pinned binary-id is present in <known-binaries-file> (a
#         pinned name absent from the live set means the grouping went stale
#         after a rename/removal and nobody updated it)
#       - no binary-id is pinned to more than one shard
#       - every shard index is a positive integer <= <shard-total>
#     A binary *not* named in the groups file is not an error here -- the
#     caller's fallback count-partition covers it automatically, so this
#     check only protects against a wrong pin, never against an unpinned
#     (i.e. uncovered) binary; final coverage is still proven end to end by
#     the <full-list>/<shard-list> form above, which check_rust_tests also
#     runs against the resulting shard.
#
#   check-shard-union.sh selftest
#     Exercises the <full-list>/<shard-list> form's accepting case (a clean
#     N-way split) and five rejecting cases (a full-only entry covered by no
#     shard; an entry duplicated across two shards; a shard entry absent from
#     full; an empty shard list; an entire binary's tests dropped from every
#     shard), plus check-groups's accepting case and two rejecting cases (an
#     unknown pinned binary-id; a binary-id pinned to two shards) -- each
#     rejecting case must make this script exit non-zero.

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

# See the "check-groups" usage block above. Format: non-comment, non-blank
# lines are "<shard> <binary-id>", 1-based shard index <= shard_total.
check_groups() {
  local groups_path="$1" known_path="$2" shard_total="$3"
  [ -f "$groups_path" ] || fail "'$groups_path' does not exist"
  [ -f "$known_path" ] || fail "'$known_path' does not exist"
  [[ "$shard_total" =~ ^[1-9][0-9]*$ ]] || fail "shard total must be a positive integer, got '$shard_total'"

  local known
  known="$(sort -u "$known_path")"

  local -a seen_binaries=()
  local line_no=0 line
  while IFS= read -r line || [ -n "$line" ]; do
    line_no=$((line_no + 1))
    local trimmed="${line%%#*}"
    trimmed="$(printf '%s' "$trimmed" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    [ -z "$trimmed" ] && continue

    local shard_idx binary_id
    read -r shard_idx binary_id <<<"$trimmed"
    if [[ ! "$shard_idx" =~ ^[1-9][0-9]*$ ]] || [ -z "${binary_id:-}" ]; then
      fail "'$groups_path' line $line_no is malformed (want '<shard> <binary-id>'): $line"
    fi
    if [ "$shard_idx" -gt "$shard_total" ]; then
      fail "'$groups_path' line $line_no pins '$binary_id' to shard $shard_idx, but shard_total is $shard_total"
    fi
    if ! grep -qxF "$binary_id" <<<"$known"; then
      fail "'$groups_path' names an unknown binary-id '$binary_id' (line $line_no) -- absent from the live cargo-nextest binary set; the binary was renamed or removed and this grouping went stale"
    fi
    if [ "${#seen_binaries[@]}" -gt 0 ]; then
      local prior
      for prior in "${seen_binaries[@]}"; do
        if [ "$prior" = "$binary_id" ]; then
          fail "'$groups_path' pins binary-id '$binary_id' more than once (line $line_no)"
        fi
      done
    fi
    seen_binaries+=("$binary_id")
  done <"$groups_path"

  echo "check-shard-union: PASS -- '$groups_path' pins ${#seen_binaries[@]} binary(s), all known, none duplicated"
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

  # Rejecting (e): an entire binary's tests dropped from every shard, not
  # just one stray entry. A duration-aware grouping pins whole binaries, so
  # its most likely failure mode is losing every entry belonging to one
  # binary at once; this proves check_union catches that shape, not only a
  # single missing line.
  printf 'binW::t1\nbinW::t2\nbinX::t1\nbinX::t2\nbinY::t1\n' >"$tmp/full-bin.txt"
  printf 'binW::t1\nbinW::t2\n' >"$tmp/shard1-bin.txt"
  printf 'binY::t1\n' >"$tmp/shard2-bin.txt"
  if (check_union "$tmp/full-bin.txt" "$tmp/shard1-bin.txt" "$tmp/shard2-bin.txt") >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: an entire binary dropped from every shard was accepted" >&2
    return 1
  fi

  # check-groups accepting: every pinned binary-id is known, none duplicated.
  printf 'binA\nbinB\nbinC\n' >"$tmp/known.txt"
  printf '1 binA\n2 binB\n' >"$tmp/groups.txt"
  if ! (check_groups "$tmp/groups.txt" "$tmp/known.txt" 3) >/dev/null; then
    echo "check-shard-union selftest: FAIL: an accepting group config was rejected" >&2
    return 1
  fi

  # check-groups rejecting (a): a pinned binary-id absent from the live
  # binary set -- the grouping went stale after a rename/removal.
  printf '1 binA\n2 binZZZ\n' >"$tmp/groups-unknown.txt"
  if (check_groups "$tmp/groups-unknown.txt" "$tmp/known.txt" 3) >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: an unknown pinned binary-id was accepted" >&2
    return 1
  fi

  # check-groups rejecting (b): the same binary-id pinned to two shards.
  printf '1 binA\n2 binA\n' >"$tmp/groups-dup.txt"
  if (check_groups "$tmp/groups-dup.txt" "$tmp/known.txt" 3) >/dev/null 2>&1; then
    echo "check-shard-union selftest: FAIL: a binary-id pinned to two shards was accepted" >&2
    return 1
  fi

  echo "check-shard-union selftest: all assertions passed"
}

case "${1:-}" in
  selftest)
    selftest
    ;;
  check-groups)
    shift
    [ "$#" -eq 3 ] || {
      echo "usage: $0 check-groups <groups-file> <known-binaries-file> <shard-total>" >&2
      exit 2
    }
    check_groups "$@"
    ;;
  "")
    echo "usage: $0 <full-list> <shard-list>... | $0 check-groups <groups-file> <known-binaries-file> <shard-total> | $0 selftest" >&2
    exit 2
    ;;
  *)
    check_union "$@"
    ;;
esac
