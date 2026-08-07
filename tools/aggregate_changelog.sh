#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Aggregates checked-in changelog fragments under `changelog.d/` into
# `CHANGELOG.md`'s `[Unreleased]` section at release time, and enforces the
# six fail-closed controls docs/DESIGN-changelog-fragments.md requires of
# this mechanism (issue #737). Stdlib-only (bash + coreutils), matching the
# `merge readiness / automation contracts` lane's dependency contract
# (tools/check-merge-readiness.sh's own comment) and this repository's
# existing pattern for that lane (tools/check-product-gate-scope.sh,
# tools/check-shard-union.sh) rather than the lane's other pattern
# (`node --test` on `.github/scripts/*.test.mjs`): the closest sibling tools
# by purpose -- a repo-structural diff/name classifier with a `selftest`
# subcommand -- are both bash, and this tool is the same shape.
#
# Fragment shape: one file per change, `changelog.d/<id>-<slug>.<category>.md`,
# where `<id>` is the issue or pull-request number (leading zeros fold to the
# same id: `0691-a.added.md` and `691-b.added.md` declare the same id 691) and
# `<category>` is the fragment's bullet-lead-word category (below). A
# fragment's body is exactly the text that would follow "- " in the rendered
# bullet -- e.g. `Fixed (#691): ... .` -- so the aggregator never invents or
# derives bullet text; it only prepends the "- "/"  " Markdown list markers
# and sorts. `changelog.d/README.md` is the human-facing copy of this
# contract and is never treated as a fragment.
#
# <category> is this repository's own vocabulary (the bullet lead word), not
# Keep a Changelog's six names -- measured from `CHANGELOG.md`'s current
# `[Unreleased]` body: `Added`, `Decided`, `Documented`, `Exempted`, `Fixed`,
# `Replaced`, `Required`, `Reverted`, `Sharded`, `Unified` (ten; `### `
# subheadings inside `[Unreleased]`: zero, so this aggregator emits bullets
# only, never subheadings).
#
# `Changed` (review correction, #737, comment 2026-08-07): an earlier version
# of this comment called `Changed` "unmeasured" and told authors not to add
# it. That was wrong on this repository's own terms. `Changed` is measured --
# 12 of `CHANGELOG.md`'s 77 `### ` subheadings across the file's full history
# use it (`Fixed` 27, `Added` 21, `Changed` 12, `Documentation` 5, `Removed`
# 1; `git grep -c '^### ' CHANGELOG.md` reproduces the 77). Those 12 predate
# this mechanism's bullet-lead-word convention -- they are `### ` subheadings
# from the file's earlier Keep-a-Changelog-style sections, not bullet lead
# words inside a current, subheading-free section -- but the word itself
# names a real, recurring category of change in this project's own history,
# the same standing the other ten words have. Excluding a word the project
# has actually used before, on the theory that its historical instances used
# a different rendering mechanism, is exactly the "routine false positive"
# reversal condition (a) below treats as grounds for no-go: a contributor
# whose change modifies existing behavior without being a defect fix
# (`fixed`), a mechanism swap (`replaced`), or an undo (`reverted`) has
# nowhere else in the declared set to put it. `DECLARED_CATEGORY_ORDER` below
# is therefore eleven words, not ten, and `changelog.d/README.md` documents
# `changed` on the same footing as the other ten. `Removed` (1 occurrence) is
# left out: one occurrence four major versions ago is not "real, recurring
# usage" the way 12 current-era occurrences are, and nothing in this
# mechanism's post-Rust history has needed it. Growing the set further is
# still a contract change to docs/DESIGN-changelog-fragments.md, the same way
# growing tools/check-product-gate-scope.sh's exempt-path list is (see
# docs/DESIGN-ci.md, "Agent-configuration exemption"): name the new word,
# show it is measured from real usage, and update changelog.d/README.md in
# the same change.
#
# DECLARED_CATEGORY_ORDER (the first sort-key component, control 3) groups
# user-facing behavior changes first (added/changed/fixed/replaced/reverted),
# then process/CI-shape changes (required/exempted/unified/sharded), then
# documentation/decision records last (documented/decided). `changed` sits
# right after `added`, matching Keep a Changelog's own Added-then-Changed
# convention, even though the rest of this vocabulary is not Keep a
# Changelog's. The order is otherwise arbitrary but fixed, is not derivable
# from the category names themselves (that is the point -- a lexicographic
# order would silently pass every determinism check while still being
# nonconforming), and is written down here, next to the set, per the
# decision record.
#
# Usage:
#   aggregate_changelog.sh check
#       Validates every fragment currently in changelog.d/: name shape and
#       category membership (control 6), and duplicate (id, category) pairs
#       (control 2).
#   aggregate_changelog.sh check-pr
#       The merge-readiness pre-merge checks: everything `check` does, plus
#       control 1 (a product-surface diff with no new, non-empty fragment)
#       and control 4's direct-edit-forbidden half, both computed from
#       BASE_SHA/HEAD_SHA in the environment (the same convention
#       tools/check-product-gate-scope.sh uses).
#   aggregate_changelog.sh check-stale
#       Control 4's release-time half: fails if changelog.d/ still has any
#       fragment at tag time.
#   aggregate_changelog.sh release --version X.Y.Z --date YYYY-MM-DD
#       Aggregates every fragment into a new `## [X.Y.Z] - YYYY-MM-DD`
#       section (preceded by whatever the existing `[Unreleased]` body still
#       holds, moved verbatim -- the migration pull request deliberately
#       left it there, see docs/DESIGN-changelog-fragments.md), verifies
#       conservation (control 5), rewrites CHANGELOG.md, and deletes the
#       consumed fragments -- all before returning, so the authority
#       handover from fragment to version section is a single atomic step
#       for the caller to commit.
#   aggregate_changelog.sh selftest
#       Executes the accepting and rejecting fixtures for all six controls,
#       printing each rejecting fixture's exit status.

set -euo pipefail
export LC_ALL=C

# Absolute path to this script, resolved once, up front -- selftest's
# end-to-end control-1/control-4 fixtures `cd` into a throwaway git repo and
# re-invoke this tool, and `$0` alone would break there if the script was
# invoked with a relative path.
SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"

# This repository's bullet-lead-word vocabulary. See the header comment.
DECLARED_CATEGORY_ORDER=(
  added
  changed
  fixed
  replaced
  reverted
  required
  exempted
  unified
  sharded
  documented
  decided
)

fail() {
  echo "$1" >&2
  exit 1
}

category_is_declared() {
  local cat="$1" c
  for c in "${DECLARED_CATEGORY_ORDER[@]}"; do
    [ "$c" = "$cat" ] && return 0
  done
  return 1
}

category_index() {
  local cat="$1" i
  for i in "${!DECLARED_CATEGORY_ORDER[@]}"; do
    if [ "${DECLARED_CATEGORY_ORDER[$i]}" = "$cat" ]; then
      echo "$i"
      return 0
    fi
  done
  return 1
}

# Prints "<numeric-id>\t<category>" for a conforming fragment basename
# (control 6), or fails with `changelog-fragment-name-invalid: <name>`.
parse_fragment_name() {
  local name="$1"
  if [[ "$name" =~ ^([0-9]+)-[a-z0-9]+(-[a-z0-9]+)*\.([a-z]+)\.md$ ]]; then
    local id="${BASH_REMATCH[1]}" cat="${BASH_REMATCH[3]}"
    if category_is_declared "$cat"; then
      printf '%s\t%s\n' "$((10#$id))" "$cat"
      return 0
    fi
  fi
  fail "changelog-fragment-name-invalid: $name"
}

# Non-README `*.md` basenames directly under $1, one per line. Enumeration
# order is filesystem-dependent and deliberately not relied on: every
# downstream consumer sorts explicitly (control 3).
list_fragment_files() {
  local dir="$1" f b
  [ -d "$dir" ] || return 0
  for f in "$dir"/*.md; do
    [ -e "$f" ] || continue
    b="$(basename "$f")"
    [ "$b" = "README.md" ] && continue
    printf '%s\n' "$b"
  done
}

validate_fragment_names() {
  local dir="$1"
  local -a files=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$(list_fragment_files "$dir" | sort)"
  # `"${files[@]}"` on a zero-element array is an unbound-variable error
  # under `set -u` on bash < 4.4 (macOS's default bash is 3.2, unlike
  # ubuntu-latest's bash 5 that merge-readiness actually runs on), so guard
  # the empty case -- an empty `changelog.d/` is trivially valid, not an
  # error, here.
  [ "${#files[@]}" -eq 0 ] && return 0
  for f in "${files[@]}"; do
    parse_fragment_name "$f" >/dev/null
  done
}

check_duplicates() {
  # Deliberately a plain indexed-array linear scan, not an associative
  # array: this keeps the script running on bash 3.2 (macOS's default,
  # unlike ubuntu-latest's bash 5, which is what merge-readiness actually
  # runs on) so a contributor can execute it locally without a newer bash.
  # Fragment counts are small (dozens at most between releases), so the
  # O(n^2) scan is not a performance concern.
  local dir="$1"
  local -a files=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$(list_fragment_files "$dir" | sort)"
  [ "${#files[@]}" -eq 0 ] && return 0
  local -a seen_keys=() seen_files=()
  for f in "${files[@]}"; do
    local meta id cat key i
    meta="$(parse_fragment_name "$f")"
    id="${meta%%$'\t'*}"
    cat="${meta##*$'\t'}"
    key="${id}:${cat}"
    for i in "${!seen_keys[@]}"; do
      if [ "${seen_keys[$i]}" = "$key" ]; then
        fail "duplicate-fragment-id: $id $cat (${seen_files[$i]}, $f)"
      fi
    done
    seen_keys+=("$key")
    seen_files+=("$f")
  done
}

# Prints fragment basenames in $1, sorted by (declared category order,
# numeric id, filename bytes) -- control 3's full three-component key.
sort_fragments() {
  local dir="$1"
  local -a files=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$(list_fragment_files "$dir")"
  [ "${#files[@]}" -eq 0 ] && return 0
  local keyed=""
  for f in "${files[@]}"; do
    local meta id cat idx
    meta="$(parse_fragment_name "$f")"
    id="${meta%%$'\t'*}"
    cat="${meta##*$'\t'}"
    idx="$(category_index "$cat")" || fail "changelog-fragment-name-invalid: $f"
    keyed+="${idx}"$'\t'"${id}"$'\t'"${f}"$'\n'
  done
  [ -z "$keyed" ] && return 0
  printf '%s' "$keyed" | sort -t "$(printf '\t')" -k1,1n -k2,2n -k3,3 | cut -f3
}

# Trims leading and trailing blank (or whitespace-only) lines from stdin.
trim_blank_edges() {
  awk '
    { lines[NR] = $0 }
    END {
      start = 1
      while (start <= NR && lines[start] ~ /^[ \t]*$/) start++
      end = NR
      while (end >= start && lines[end] ~ /^[ \t]*$/) end--
      for (i = start; i <= end; i++) print lines[i]
    }
  '
}

trimmed_fragment_body() {
  trim_blank_edges <"$1"
}

fragment_is_empty() {
  local body
  body="$(trimmed_fragment_body "$1")"
  [ -z "$body" ]
}

# Renders one fragment's trimmed body as a single Markdown bullet: "- " on
# the first line, "  " on every subsequent non-blank line, blank lines left
# blank. This is the only transformation control 5's "reaches" predicate is
# allowed to see between a fragment's content and the written section.
render_fragment() {
  trimmed_fragment_body "$1" | awk '
    NR == 1 { print "- " $0; next }
    /^[ \t]*$/ { print ""; next }
    { print "  " $0 }
  '
}

aggregate_section_body() {
  local dir="$1"
  local -a sorted=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$(sort_fragments "$dir")"
  [ "${#sorted[@]}" -eq 0 ] && return 0
  local out="" f_out
  for f in "${sorted[@]}"; do
    f_out="$(render_fragment "$dir/$f")"
    if [ -z "$out" ]; then
      out="$f_out"
    else
      out="${out}"$'\n'"${f_out}"
    fi
  done
  printf '%s' "$out"
}

# Splits $1 into its top-level bullet blocks, NUL-separated on stdout (a
# block begins at a line matching `^- ` and continues through every line
# before the next such line, or EOF). Used only by verify_conservation, to
# turn "produced" back into the same per-bullet units render_fragment
# produces, for an exact positional comparison.
split_bullets() {
  awk '
    /^- / { if (started) printf "%s%c", block, 0; block = $0; started = 1; next }
    { block = block "\n" $0 }
    END { if (started) printf "%s%c", block, 0 }
  ' <<<"$1"
}

# Control 5 (conservation): the produced text must contain exactly one
# top-level bullet per fragment, and bullet *i* (in control 3's declared
# order) must be byte-for-byte render_fragment's output for fragment *i* --
# not merely "produced contains this block somewhere". A substring/
# containment check (this function's previous shape) is satisfiable by an
# aggregator that drops fragment A and emits fragment B twice, where A's
# rendered block is a byte-for-byte prefix of B's: the bullet count still
# matches and the substring scan still finds A's block -- inside the
# duplicate, not in A's own position. Per-position identity closes that: the
# dropped fragment's own position no longer holds its own block, so it
# fails.
verify_conservation() {
  local dir="$1" produced="$2"
  local -a sorted=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$(sort_fragments "$dir")"
  local -a actual=()
  local seg
  while IFS= read -r -d '' seg; do actual+=("$seg"); done < <(split_bullets "$produced")
  if [ "${#actual[@]}" -ne "${#sorted[@]}" ]; then
    fail "fragment-dropped: bullet count mismatch (expected ${#sorted[@]} fragment(s), produced ${#actual[@]} top-level bullet(s))"
  fi
  [ "${#sorted[@]}" -eq 0 ] && return 0
  local i expected_block
  for i in "${!sorted[@]}"; do
    expected_block="$(render_fragment "$dir/${sorted[$i]}")"
    if [ "${actual[$i]}" != "$expected_block" ]; then
      fail "fragment-dropped: ${sorted[$i]}"
    fi
  done
}

# ---- CHANGELOG.md section extraction -------------------------------------

before_unreleased() {
  awk '
    /^## \[Unreleased\]$/ { exit }
    { print }
  ' "$1"
}

# Raw block strictly between the "## [Unreleased]" heading and the next
# "## [" heading -- includes its own leading/trailing blank line(s), if any,
# verbatim. This is the exact same extraction used to measure the 620-line /
# 27-bullet / 11-with-id / 16-without-id baseline cited in the decision
# record's implementation correction.
unreleased_raw_body() {
  awk '
    /^## \[Unreleased\]$/ { found = 1; next }
    found && /^## \[/ { exit }
    found { print }
  ' "$1"
}

# Everything from the first "## [" heading following "## [Unreleased]"
# onward, to EOF -- inclusive of that heading line.
after_unreleased() {
  awk '
    /^## \[Unreleased\]$/ { seen = 1; next }
    seen && /^## \[/ { print; after = 1; next }
    after { print }
  ' "$1"
}

# Given stdin "heading1\nbody...\nheading2\nrest...", drops heading1 and its
# body, printing heading2 onward (or nothing, if there is no heading2).
strip_first_section() {
  awk '
    NR == 1 { next }
    !seen && /^## \[/ { seen = 1 }
    seen { print }
  '
}

# Drops a trailing contiguous block of Markdown link-reference lines
# ("[X]: url"), which docs/RELEASE.md step 7 updates on every release.
strip_link_ref_tail() {
  awk '
    { lines[NR] = $0 }
    END {
      end = NR
      while (end >= 1 && lines[end] ~ /^\[[^]]+\]:/) end--
      for (i = 1; i <= end; i++) print lines[i]
    }
  '
}

# ---- control 1: missing or empty fragment --------------------------------

is_product_surface_path() {
  case "$1" in
    rust/*|src/fslc/*|specs/*|examples/*|docs/LANGUAGE*|skills/fsl/reference.md) return 0 ;;
    *) return 1 ;;
  esac
}

# Reads "<status>\t<path>" lines (git diff --name-status shape) on stdin.
# Fails `changelog-fragment-missing: <paths>` if any changed path is a
# product surface and no fragment was added; the caller checks emptiness of
# each added fragment separately (needs blob content, not just the diff).
classify_product_diff() {
  local status path
  local -a product=() added_fragments=()
  while IFS=$'\t' read -r status path; do
    [ -z "${status:-}" ] && continue
    if is_product_surface_path "$path"; then
      product+=("$path")
    fi
    if [ "$status" = "A" ] && [[ "$path" == changelog.d/*.md ]] && [ "$(basename "$path")" != "README.md" ]; then
      added_fragments+=("$path")
    fi
  done
  if [ "${#product[@]}" -gt 0 ] && [ "${#added_fragments[@]}" -eq 0 ]; then
    fail "changelog-fragment-missing: ${product[*]}"
  fi
  # `"${arr[@]}"` on an empty array is an unbound-variable error under
  # `set -u` on bash < 4.4 (macOS's default bash is 3.2), so guard the
  # common empty case explicitly instead of relying on a modern bash.
  if [ "${#added_fragments[@]}" -gt 0 ]; then
    printf '%s\n' "${added_fragments[@]}"
  fi
}

# ---- control 4b: direct-edit-forbidden, with the single release exclusion -

# Pure predicate over two whole-file CHANGELOG.md snapshots. See
# docs/DESIGN-changelog-fragments.md, control 4, and the migration
# correction: the only excluded diff shape left is the release move (the
# migration pull request does not touch the body at all).
check_direct_edit_files() {
  local base="$1" head="$2"

  grep -qxF '## [Unreleased]' "$base" || fail "changelog-direct-edit-forbidden: '## [Unreleased]' heading missing from the base revision"
  grep -qxF '## [Unreleased]' "$head" || fail "changelog-direct-edit-forbidden: '## [Unreleased]' heading was removed or renamed"

  local old_body new_body
  old_body="$(unreleased_raw_body "$base")"
  new_body="$(unreleased_raw_body "$head")"

  [ "$old_body" = "$new_body" ] && return 0

  local new_bullets
  new_bullets="$(printf '%s\n' "$new_body" | trim_blank_edges)"
  if [ -n "$new_bullets" ]; then
    fail "changelog-direct-edit-forbidden: the [Unreleased] body was edited directly"
  fi

  local old_bullets
  old_bullets="$(printf '%s\n' "$old_body" | trim_blank_edges)"

  local base_rest head_rest
  base_rest="$(after_unreleased "$base")"
  head_rest="$(after_unreleased "$head")"

  # First-line extraction via parameter expansion, not `| head -n1`: piping
  # a large variable into `head` lets `head` close the pipe early and send
  # the writer SIGPIPE, which `pipefail` turns into a nonzero exit that
  # `set -e` would otherwise abort this script on.
  local head_first_heading base_first_heading
  head_first_heading="${head_rest%%$'\n'*}"
  base_first_heading="${base_rest%%$'\n'*}"

  if [ -z "$head_first_heading" ] || [ "$head_first_heading" = "$base_first_heading" ]; then
    fail "changelog-direct-edit-forbidden: [Unreleased] was emptied with no new version section added"
  fi
  [[ "$head_first_heading" =~ ^\#\#\ \[[0-9]+\.[0-9]+\.[0-9]+\]\ -\ [0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] \
    || fail "changelog-direct-edit-forbidden: new heading '$head_first_heading' is not a version heading"

  # `stop`, not `exit`, inside the awk: an early `exit` here would close the
  # pipe before `tail`/`printf` finish writing, which sends them SIGPIPE and
  # (under `pipefail`) fails this whole function under `set -e` -- the same
  # hazard `head -n1` had above, just one step removed.
  local new_section_body
  new_section_body="$(printf '%s\n' "$head_rest" | tail -n +2 | awk '/^## \[/ { stop = 1 } !stop { print }' | trim_blank_edges)"

  if [ -n "$old_bullets" ] && [[ "$new_section_body" != "$old_bullets"* ]]; then
    fail "changelog-direct-edit-forbidden: the new version section does not start with the moved [Unreleased] content"
  fi

  local head_remainder
  head_remainder="$(printf '%s\n' "$head_rest" | strip_first_section)"
  local base_rest_stripped head_remainder_stripped
  base_rest_stripped="$(printf '%s\n' "$base_rest" | strip_link_ref_tail)"
  head_remainder_stripped="$(printf '%s\n' "$head_remainder" | strip_link_ref_tail)"
  if [ "$base_rest_stripped" != "$head_remainder_stripped" ]; then
    fail "changelog-direct-edit-forbidden: the release commit changed content outside the new version section"
  fi
}

check_direct_edit() {
  local changelog="${1:-CHANGELOG.md}"
  : "${BASE_SHA:?BASE_SHA is required}"
  : "${HEAD_SHA:?HEAD_SHA is required}"
  local tmp
  tmp="$(mktemp -d)"
  # `trap ... RETURN` is a single global registration, not scoped to this
  # function: left as-is, it would fire again on the *next* function's
  # return too, referencing a `$tmp` that has since gone out of scope and
  # aborting with "unbound variable" under `set -u`. Clearing it inside its
  # own body makes it fire exactly once.
  trap 'rm -rf "$tmp"; trap - RETURN' RETURN
  git show "$BASE_SHA:$changelog" >"$tmp/base.md" 2>/dev/null || fail "changelog-direct-edit-forbidden: cannot read $changelog at $BASE_SHA"
  git show "$HEAD_SHA:$changelog" >"$tmp/head.md" 2>/dev/null || fail "changelog-direct-edit-forbidden: cannot read $changelog at $HEAD_SHA"
  check_direct_edit_files "$tmp/base.md" "$tmp/head.md"
}

check_missing_or_empty_fragment() {
  : "${BASE_SHA:?BASE_SHA is required}"
  : "${HEAD_SHA:?HEAD_SHA is required}"
  # `classify_product_diff` runs as the right side of a pipe inside this
  # command substitution, which is its own subshell: its `fail()`'s `exit 1`
  # only ends that subshell, and feeding the substitution straight into
  # `<<<` for a `while read` loop discards the substitution's own exit
  # status entirely (`done <<<...`'s status is the loop's, not the
  # substitution's). `pipefail` (set globally) still lets the assignment
  # below observe the failure -- capture and re-raise it explicitly, or a
  # missing/empty fragment silently passes instead of failing closed.
  local diff_output status
  set +e
  diff_output="$(git diff --name-status "$BASE_SHA...$HEAD_SHA" | classify_product_diff)"
  status=$?
  set -e
  [ "$status" -eq 0 ] || exit 1
  local -a added=()
  local f
  while IFS= read -r f; do
    [ -n "$f" ] && added+=("$f")
  done <<<"$diff_output"
  [ "${#added[@]}" -eq 0 ] && return 0
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"; trap - RETURN' RETURN
  for f in "${added[@]}"; do
    git show "$HEAD_SHA:$f" >"$tmp/frag.md" 2>/dev/null || fail "changelog-fragment-missing: cannot read $f at $HEAD_SHA"
    if fragment_is_empty "$tmp/frag.md"; then
      fail "changelog-fragment-empty: $f"
    fi
  done
}

# ---- subcommands ----------------------------------------------------------

check() {
  local dir="${1:-changelog.d}"
  validate_fragment_names "$dir"
  check_duplicates "$dir"
  echo "aggregate_changelog: check PASS -- every fragment in '$dir' has a conforming name and no duplicate (id, category)"
}

check_pr() {
  local dir="${1:-changelog.d}"
  validate_fragment_names "$dir"
  check_duplicates "$dir"
  check_missing_or_empty_fragment
  check_direct_edit
  echo "aggregate_changelog: check-pr PASS"
}

check_stale() {
  local dir="${1:-changelog.d}"
  local -a files=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$(list_fragment_files "$dir")"
  if [ "${#files[@]}" -gt 0 ]; then
    fail "stale-fragments-present: ${files[*]}"
  fi
  echo "aggregate_changelog: check-stale PASS -- '$dir' has no fragments left"
}

release() {
  local version="" date="" changelog="CHANGELOG.md" fragdir="changelog.d"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version) version="$2"; shift 2 ;;
      --date) date="$2"; shift 2 ;;
      --changelog) changelog="$2"; shift 2 ;;
      --fragments-dir) fragdir="$2"; shift 2 ;;
      *) echo "usage: $0 release --version X.Y.Z --date YYYY-MM-DD [--changelog FILE] [--fragments-dir DIR]" >&2; exit 2 ;;
    esac
  done
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "release: --version must be SemVer X.Y.Z, got '$version'"
  [[ "$date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || fail "release: --date must be YYYY-MM-DD, got '$date'"
  [ -f "$changelog" ] || fail "release: '$changelog' not found"

  validate_fragment_names "$fragdir"
  check_duplicates "$fragdir"

  local -a sorted=()
  local f
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$(sort_fragments "$fragdir")"
  # Checked before the loop below: an empty `changelog.d/` must fail with
  # `no-fragments-to-aggregate` rather than a bash unbound-variable error
  # from iterating an empty array under `set -u` on bash < 4.4.
  [ "${#sorted[@]}" -gt 0 ] || fail "no-fragments-to-aggregate"
  for f in "${sorted[@]}"; do
    fragment_is_empty "$fragdir/$f" && fail "changelog-fragment-empty: $fragdir/$f"
  done

  local frag_block
  frag_block="$(aggregate_section_body "$fragdir")"

  local prefix old_body rest
  prefix="$(before_unreleased "$changelog")"
  old_body="$(unreleased_raw_body "$changelog")"
  rest="$(after_unreleased "$changelog")"

  local old_bullets
  old_bullets="$(printf '%s\n' "$old_body" | trim_blank_edges)"

  # Conservation (control 5) only governs the fragments being consumed here
  # -- the pre-existing [Unreleased] body is moved verbatim, unmodified, and
  # is not itself a fragment.
  verify_conservation "$fragdir" "$frag_block"

  local combined="$old_bullets"
  if [ -n "$combined" ] && [ -n "$frag_block" ]; then
    combined="${combined}"$'\n'"${frag_block}"
  elif [ -z "$combined" ]; then
    combined="$frag_block"
  fi

  {
    printf '%s\n\n' "$prefix"
    printf '## [Unreleased]\n\n'
    printf '## [%s] - %s\n\n' "$version" "$date"
    printf '%s\n\n' "$combined"
    printf '%s\n' "$rest"
  } >"$changelog.new"
  mv "$changelog.new" "$changelog"

  for f in "${sorted[@]}"; do
    rm -f "$fragdir/$f"
  done

  echo "aggregate_changelog: released $version, aggregated ${#sorted[@]} fragment(s), '$fragdir' is now empty"
}

# ---- selftest --------------------------------------------------------------
#
# Every control below has both an accepting and a rejecting fixture, and
# every rejecting fixture is executed -- its exit status is captured and
# printed, not merely asserted true/false by an `if`. `fail()` calls `exit`
# unconditionally, which an `if cmd; then` guard does not catch (`exit`
# terminates the whole process, not just the failed pipeline stage), so
# every case below that is expected to fail runs in its own `( subshell )`,
# the same pattern tools/check-shard-union.sh's selftest uses and documents.

ST_FAILURES=0

st_report() {
  local label="$1" expect="$2" status="$3"
  if { [ "$expect" = "pass" ] && [ "$status" -eq 0 ]; } || { [ "$expect" = "fail" ] && [ "$status" -ne 0 ]; }; then
    echo "selftest: PASS -- $label (expected $expect, exit=$status)"
  else
    echo "selftest: FAIL -- $label (expected $expect, got exit=$status)" >&2
    ST_FAILURES=$((ST_FAILURES + 1))
  fi
}

st_expect_pass() {  # $1=label, $2..=command
  local label="$1"; shift
  local status
  # `set +e`/`set -e` bracket the call: a nonzero exit here is data this
  # function reports on, not a real failure of the selftest script itself,
  # and under `set -e` a plain (non-`if`-guarded) nonzero simple command --
  # which is exactly what capturing `$?` after the fact requires -- would
  # abort the whole selftest before st_report ever ran.
  set +e
  ( "$@" ) >/dev/null 2>&1
  status=$?
  set -e
  st_report "$label" pass "$status"
}

st_expect_fail() {  # $1=label, $2..=command
  local label="$1"; shift
  local out status
  set +e
  out="$( ( "$@" ) 2>&1 1>/dev/null )"
  status=$?
  set -e
  echo "selftest: rejecting fixture '$label' exited $status: $out"
  st_report "$label" fail "$status"
}

st_setup_frag() {  # $1=dir $2=basename $3...=body lines
  local dir="$1" name="$2"
  shift 2
  printf '%s\n' "$@" >"$dir/$name"
}

# Control 6: nonconforming fragment name.
selftest_control6() {
  local tmp; tmp="$(mktemp -d)"
  local cat
  for cat in "${DECLARED_CATEGORY_ORDER[@]}"; do
    st_expect_pass "control6 accepting: 691-x.$cat.md" parse_fragment_name "691-x.$cat.md"
  done
  st_expect_fail "control6 rejecting: no leading digits (foo-bar.added.md)" parse_fragment_name "foo-bar.added.md"
  st_expect_fail "control6 rejecting: undeclared category (691-x.chore.md)" parse_fragment_name "691-x.chore.md"
  rm -rf "$tmp"
}

# Control 2: duplicate (id, category).
selftest_control2() {
  local tmp; tmp="$(mktemp -d)"

  st_setup_frag "$tmp" "691-x.added.md" "Added (#691): x."
  st_setup_frag "$tmp" "691-y.fixed.md" "Fixed (#691): y."
  st_expect_pass "control2 accepting: one issue, two sections" check_duplicates "$tmp"
  rm -f "$tmp"/*.md

  st_setup_frag "$tmp" "691-a.added.md" "Added (#691): a."
  st_setup_frag "$tmp" "691-b.added.md" "Added (#691): b."
  st_expect_fail "control2 rejecting: same (id, category) twice" check_duplicates "$tmp"
  rm -f "$tmp"/*.md

  st_setup_frag "$tmp" "0691-a.added.md" "Added (#691): a, zero-padded."
  st_setup_frag "$tmp" "691-b.added.md" "Added (#691): b."
  st_expect_fail "control2 rejecting: zero-padded alias (0691 == 691)" check_duplicates "$tmp"

  rm -rf "$tmp"
}

# Control 3: nondeterministic or nonconforming order -- both sort-key
# components, each calibrated against a real lexicographic-sort sham that
# must fail the golden, plus determinism/idempotence under reversed physical
# file-creation order.
selftest_control3() {
  local tmp; tmp="$(mktemp -d)"

  # (a) numeric id golden: 9, 10, 100 in one category. A byte/lexicographic
  # sort would order them 10, 100, 9.
  st_setup_frag "$tmp" "9-alpha.added.md" "Added (#9): alpha."
  st_setup_frag "$tmp" "10-beta.added.md" "Added (#10): beta."
  st_setup_frag "$tmp" "100-gamma.added.md" "Added (#100): gamma."
  local golden_numeric=$'9-alpha.added.md\n10-beta.added.md\n100-gamma.added.md'
  local real_numeric
  real_numeric="$(sort_fragments "$tmp")"
  if [ "$real_numeric" = "$golden_numeric" ]; then
    st_report "control3 accepting: real sort_fragments matches the 9/10/100 numeric golden" pass 0
  else
    st_report "control3 accepting: real sort_fragments matches the 9/10/100 numeric golden" pass 1
  fi
  local lexicographic_numeric
  lexicographic_numeric="$(list_fragment_files "$tmp" | LC_ALL=C sort)"
  echo "selftest: rejecting fixture 'control3 rejecting: lexicographic sham vs numeric golden' produced: $(printf '%s' "$lexicographic_numeric" | tr '\n' ' ')"
  if [ "$lexicographic_numeric" = "$golden_numeric" ]; then
    st_report "control3 rejecting: lexicographic sham vs numeric golden (aggregation-order-wrong)" fail 0
  else
    st_report "control3 rejecting: lexicographic sham vs numeric golden (aggregation-order-wrong)" fail 1
  fi
  rm -f "$tmp"/*.md

  # (b) category order golden: a pair lexicographic order would swap.
  # Declared order has "fixed" (index 1) before "decided" (index 9);
  # lexicographically "decided" < "fixed".
  st_setup_frag "$tmp" "1-a.decided.md" "Decided (#1): a."
  st_setup_frag "$tmp" "2-b.fixed.md" "Fixed (#2): b."
  local golden_category=$'2-b.fixed.md\n1-a.decided.md'
  local real_category
  real_category="$(sort_fragments "$tmp")"
  if [ "$real_category" = "$golden_category" ]; then
    st_report "control3 accepting: real sort_fragments matches the fixed/decided category golden" pass 0
  else
    st_report "control3 accepting: real sort_fragments matches the fixed/decided category golden" pass 1
  fi
  local lexicographic_category
  lexicographic_category="$(list_fragment_files "$tmp" | LC_ALL=C sort)"
  echo "selftest: rejecting fixture 'control3 rejecting: lexicographic-category sham vs category golden' produced: $(printf '%s' "$lexicographic_category" | tr '\n' ' ')"
  if [ "$lexicographic_category" = "$golden_category" ]; then
    st_report "control3 rejecting: lexicographic-category sham vs category golden (aggregation-order-wrong)" fail 0
  else
    st_report "control3 rejecting: lexicographic-category sham vs category golden (aggregation-order-wrong)" fail 1
  fi
  rm -f "$tmp"/*.md

  # (c) determinism under a shuffled/reversed physical enumeration: build
  # the identical fragment set in two directories, creating the files in
  # different orders, and require sort_fragments to still agree.
  local tmp_a tmp_b
  tmp_a="$(mktemp -d)"; tmp_b="$(mktemp -d)"
  st_setup_frag "$tmp_a" "100-gamma.added.md" "Added (#100): gamma."
  st_setup_frag "$tmp_a" "9-alpha.added.md" "Added (#9): alpha."
  st_setup_frag "$tmp_a" "10-beta.added.md" "Added (#10): beta."
  st_setup_frag "$tmp_b" "9-alpha.added.md" "Added (#9): alpha."
  st_setup_frag "$tmp_b" "10-beta.added.md" "Added (#10): beta."
  st_setup_frag "$tmp_b" "100-gamma.added.md" "Added (#100): gamma."
  local out_a out_b
  out_a="$(sort_fragments "$tmp_a")"
  out_b="$(sort_fragments "$tmp_b")"
  if [ "$out_a" = "$out_b" ]; then
    st_report "control3 accepting: sort_fragments is byte-identical under reversed file-creation order" pass 0
  else
    st_report "control3 accepting: sort_fragments is byte-identical under reversed file-creation order" pass 1
  fi

  # (d) idempotence: two consecutive runs on the same directory agree.
  local out_a2
  out_a2="$(sort_fragments "$tmp_a")"
  if [ "$out_a" = "$out_a2" ]; then
    st_report "control3 accepting: sort_fragments is idempotent (two consecutive runs agree)" pass 0
  else
    st_report "control3 accepting: sort_fragments is idempotent (two consecutive runs agree)" pass 1
  fi
  rm -rf "$tmp_a" "$tmp_b"

  # A former case (e) here compared two hard-coded, already-different string
  # literals for equality as a stand-in for "a readdir-ordered sham fails the
  # determinism check". It never called sort_fragments, list_fragment_files,
  # or any other function this script defines, so its outcome was knowable
  # from reading the two assignments -- it could not have failed regardless
  # of this file's implementation, and its PASS was not evidence the
  # determinism check works (review finding, #737, comment 2026-08-07).
  # Removed rather than patched: bash's own pathname expansion always
  # returns glob matches in sorted order (POSIX), so list_fragment_files
  # cannot be driven into readdir order through this script's own primitives
  # to build a genuine, non-fabricated sham here, and a synthetic one is what
  # case (e) already tried and failed to be honestly. Cases (a) and (b) above
  # already are genuine goldens with a real sham that sorts differently
  # (lexicographic id order, lexicographic category order) and are
  # confirmed, independently, to reject it; case (c)'s reversed-physical-
  # creation-order fixture already exercises the real sort_fragments/
  # list_fragment_files pair against actual filesystem enumeration order,
  # which is the genuine version of what case (e) was reaching for.

  rm -rf "$tmp"
}

# Control 5: silent drop at aggregation (conservation).
selftest_control5() {
  local tmp; tmp="$(mktemp -d)"
  st_setup_frag "$tmp" "1-a.added.md" "Added (#1): first."
  st_setup_frag "$tmp" "2-b.added.md" "Fixed (#2): second, multi-line." "  continuation line kept as its own bullet line."
  # parse_fragment_name rejects a non-declared category; use "fixed" lead
  # word text inside a body that still lives in the "added" category file
  # is fine -- category membership is a filename property, not a body one.
  st_setup_frag "$tmp" "3-c.added.md" "Added (#3): third."

  local faithful
  faithful="$(aggregate_section_body "$tmp")"
  st_expect_pass "control5 accepting: faithful aggregation of 3 fragments (one multi-line)" verify_conservation "$tmp" "$faithful"

  # Rejecting (a): a sham that drops one of three fragments.
  local dropped
  dropped="$(render_fragment "$tmp/1-a.added.md")"$'\n'"$(render_fragment "$tmp/3-c.added.md")"
  st_expect_fail "control5 rejecting: sham drops one of three fragments (fragment-dropped)" verify_conservation "$tmp" "$dropped"

  # Rejecting (b): a sham that copies only each fragment's first line,
  # contributing "something" from every fragment while keeping the bullet
  # count equal to the fragment count -- a "contributes" test with no byte
  # scope would pass this; verify_conservation must not.
  local truncated
  truncated="- Added (#1): first."$'\n'"- Fixed (#2): second, multi-line."$'\n'"- Added (#3): third."
  st_expect_fail "control5 rejecting: sham copies only each fragment's first line" verify_conservation "$tmp" "$truncated"
  rm -f "$tmp"/*.md

  # Rejecting (c): a sham that drops fragment A and emits fragment B twice,
  # where A's entire rendered block is a byte-for-byte prefix of B's --
  # review finding, #737, comment 2026-08-07. Bullet count still matches (2
  # expected, 2 produced) and a *substring* scan still finds A's block
  # (inside the first copy of B), which is exactly why the previous
  # containment-based verify_conservation passed this: it never checked that
  # the bullet in A's own *position* was A's own block. Per-bullet identity
  # must reject it.
  st_setup_frag "$tmp" "1-a.added.md" "Fixed (#1): a."
  st_setup_frag "$tmp" "2-b.added.md" "Fixed (#1): a." "more detail."
  local duplicated
  duplicated="$(render_fragment "$tmp/2-b.added.md")"$'\n'"$(render_fragment "$tmp/2-b.added.md")"
  st_expect_fail "control5 rejecting: sham drops fragment A and emits fragment B (whose block starts with A's) twice" \
    verify_conservation "$tmp" "$duplicated"

  rm -rf "$tmp"
}

# Control 1: missing or empty fragment.
selftest_control1() {
  st_expect_pass "control1 accepting: product-surface diff with a new fragment" \
    bash -c 'printf "M\trust/fsl-core/src/lib.rs\nA\tchangelog.d/701-x.added.md\n" | classify_product_diff'
  st_expect_fail "control1 rejecting: product-surface diff with no fragment (changelog-fragment-missing)" \
    bash -c 'printf "M\trust/fsl-core/src/lib.rs\n" | classify_product_diff'
  st_expect_pass "control1 accepting: non-product-surface diff needs no fragment" \
    bash -c 'printf "M\tdocs/README.md\n" | classify_product_diff'

  local tmp; tmp="$(mktemp -d)"
  printf 'Added (#1): real content.\n' >"$tmp/nonempty.md"
  printf '   \n\t\n' >"$tmp/whitespace-only.md"
  st_expect_pass "control1 accepting: non-empty fragment content" bash -c "! fragment_is_empty '$tmp/nonempty.md'"
  # fragment_is_empty returns 0 (shell "true") exactly when the fragment IS
  # empty, so the "rejecting fixture" here is a whitespace-only fragment
  # correctly being detected as empty, not a nonzero exit from a
  # diagnostic-emitting command; assert its detection directly.
  if fragment_is_empty "$tmp/whitespace-only.md"; then
    echo "selftest: rejecting fixture 'control1 rejecting: whitespace-only fragment is detected empty' exited 0 (correctly detected as empty)"
    st_report "control1 rejecting: whitespace-only fragment is detected empty" fail 1
  else
    st_report "control1 rejecting: whitespace-only fragment is detected empty" fail 0
  fi
  rm -rf "$tmp"
}

# Control 4: unaggregated at release (4a) and direct-edit-forbidden with the
# single release exclusion (4b) -- both the pure file-based predicate and
# the real git-backed wrapper, exercised end to end in a throwaway repo.
selftest_control4() {
  local tmp; tmp="$(mktemp -d)"

  # 4a: check-stale.
  mkdir -p "$tmp/4a-empty" "$tmp/4a-stale"
  st_expect_pass "control4a accepting: empty changelog.d/ at tag time" check_stale "$tmp/4a-empty"
  printf 'Added (#1): x.\n' >"$tmp/4a-stale/1-x.added.md"
  st_expect_fail "control4a rejecting: fragment still present at tag time (stale-fragments-present)" check_stale "$tmp/4a-stale"

  # 4b, pure predicate: exercised extensively above in-development; repeat
  # the core shapes here so `selftest` alone is sufficient evidence.
  local base head
  base="$tmp/base.md"; head="$tmp/head.md"
  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n- Fixed (#2): two.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$base"

  cp "$base" "$head"
  st_expect_pass "control4b accepting: identical [Unreleased] body" check_direct_edit_files "$base" "$head"

  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n- Fixed (#2): two.\n- Added (#3): a sneaky direct addition.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_fail "control4b rejecting: a line added directly to [Unreleased] (changelog-direct-edit-forbidden)" check_direct_edit_files "$base" "$head"

  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_fail "control4b rejecting: a line deleted directly from [Unreleased] (changelog-direct-edit-forbidden)" check_direct_edit_files "$base" "$head"

  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [2.0.0] - 2026-08-07\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v2.0.0...HEAD\n[2.0.0]: https://example/compare/v1.0.0...v2.0.0\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_fail "control4b rejecting: release move that silently drops one bullet (changelog-direct-edit-forbidden)" check_direct_edit_files "$base" "$head"

  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [2.0.0] - 2026-08-07\n\n- Added (#1): one.\n- Fixed (#2): two.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v2.0.0...HEAD\n[2.0.0]: https://example/compare/v1.0.0...v2.0.0\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_pass "control4b accepting: a valid release move (body preserved verbatim under a new version heading)" check_direct_edit_files "$base" "$head"

  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [2.0.0] - 2026-08-07\n\n- Added (#1): one.\n- Fixed (#2): two.\n- Added (#3): a fragment aggregated in the same release.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v2.0.0...HEAD\n[2.0.0]: https://example/compare/v1.0.0...v2.0.0\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_pass "control4b accepting: a valid release move with aggregated fragments appended" check_direct_edit_files "$base" "$head"

  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [2.0.0] - 2026-08-07\n\n- Added (#1): one.\n- Fixed (#2): two.\n\n## [1.0.0] - 2026-01-01\n\n- SNEAKILY EDITED Old.\n\n[Unreleased]: https://example/compare/v2.0.0...HEAD\n[2.0.0]: https://example/compare/v1.0.0...v2.0.0\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_fail "control4b rejecting: release commit also tampers with historical content" check_direct_edit_files "$base" "$head"

  # End-to-end: the real BASE_SHA/HEAD_SHA-driven wrapper, in a throwaway
  # git repository, proving the wiring (not only the pure predicate).
  local repo="$tmp/repo"
  mkdir -p "$repo/changelog.d" "$repo/rust"
  (
    cd "$repo"
    git init -q
    git config user.email "selftest@example.invalid"
    git config user.name "selftest"
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n' >CHANGELOG.md
    printf 'fn main() {}\n' >rust/lib.rs
    git add -A
    git commit -q -m base
  )
  local base_sha; base_sha="$(cd "$repo" && git rev-parse HEAD)"

  (
    cd "$repo"
    printf 'fn main() { /* changed */ }\n' >rust/lib.rs
    printf 'Added (#2): a real product change with its fragment.\n' >changelog.d/2-x.added.md
    git add -A
    git commit -q -m "with fragment"
  )
  local head_sha_ok; head_sha_ok="$(cd "$repo" && git rev-parse HEAD)"
  st_expect_pass "control1+4b end-to-end accepting: check-pr on a product change with its fragment" \
    bash -c "cd '$repo' && BASE_SHA='$base_sha' HEAD_SHA='$head_sha_ok' '$SELF' check-pr"

  (
    cd "$repo"
    git checkout -q "$base_sha" -- rust/lib.rs changelog.d 2>/dev/null || true
    rm -f changelog.d/2-x.added.md
    printf 'fn main() { /* changed again, no fragment */ }\n' >rust/lib.rs
    git add -A
    git commit -q -m "no fragment"
  )
  local head_sha_missing; head_sha_missing="$(cd "$repo" && git rev-parse HEAD)"
  st_expect_fail "control1 end-to-end rejecting: check-pr on a product change with no fragment (changelog-fragment-missing)" \
    bash -c "cd '$repo' && BASE_SHA='$head_sha_ok' HEAD_SHA='$head_sha_missing' '$SELF' check-pr"

  (
    cd "$repo"
    printf 'Added (#3): a direct edit bypassing changelog.d.\n' >>CHANGELOG.md
    sed -i.bak 's/^- Added (#1): one\.$/- Added (#1): one.\n- Added (#4): sneaked in directly./' CHANGELOG.md
    rm -f CHANGELOG.md.bak
    git add -A
    git commit -q -m "direct edit"
  )
  local head_sha_direct; head_sha_direct="$(cd "$repo" && git rev-parse HEAD)"
  st_expect_fail "control4b end-to-end rejecting: check-pr on a direct [Unreleased] edit (changelog-direct-edit-forbidden)" \
    bash -c "cd '$repo' && BASE_SHA='$head_sha_missing' HEAD_SHA='$head_sha_direct' '$SELF' check-pr"

  rm -rf "$tmp"
}

selftest() {
  # Several fixtures below invoke `bash -c '... | classify_product_diff'`
  # (control 1) or `bash -c "... '$SELF' check-pr"` (the control 1/4b
  # end-to-end cases) so a pipeline or a fresh process can reach a function
  # defined in this script. A `bash -c` child is a brand-new process, not a
  # subshell of this one, so it does not see this script's functions unless
  # they are exported; without this, every such fixture fails closed with
  # exit 127 ("command not found"), not the diagnostic it is meant to prove.
  local fn
  for fn in $(declare -F | awk '{print $3}'); do
    # shellcheck disable=SC2163  # exporting the function named by $fn, not a variable literally called "fn"
    export -f "$fn" 2>/dev/null || true
  done

  selftest_control1
  selftest_control2
  selftest_control3
  selftest_control4
  selftest_control5
  echo
  if [ "$ST_FAILURES" -eq 0 ]; then
    echo "aggregate_changelog: selftest PASS -- all six controls' accepting and rejecting fixtures behaved as expected"
  else
    echo "aggregate_changelog: selftest FAIL -- $ST_FAILURES assertion(s) failed" >&2
    exit 1
  fi
}

case "${1:-}" in
  check) shift; check "$@" ;;
  check-pr) shift; check_pr "$@" ;;
  check-stale) shift; check_stale "$@" ;;
  release) shift; release "$@" ;;
  selftest) shift; selftest "$@" ;;
  *)
    echo "usage: $0 {check|check-pr|check-stale|release|selftest} [args]" >&2
    exit 2
    ;;
esac
