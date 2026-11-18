#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "aggregate_changelog.sh requires Bash 4 or newer" >&2; exit 1; }
# SPDX-License-Identifier: Apache-2.0
#
# Aggregates checked-in changelog fragments under `changelog.d/` into
# `CHANGELOG.md`'s `[Unreleased]` section at release time, and enforces the
# six fail-closed controls docs/design/DESIGN-changelog-fragments.md requires of
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
# `changed` on the same footing as the other ten.
#
# `Removed` (1 occurrence) is left out, but not on a frequency argument --
# frequency alone does not separate the cases (review correction, #737,
# comment 2026-08-07, second round): seven of the eleven admitted words each
# occur exactly once in their own corpus (this mechanism's own post-Rust
# fragment history) and zero times among `### ` headings, the same posture
# `Removed`'s single `### ` occurrence has. What actually separates them is
# **recency**: `### Changed` last appears in `## [4.0.0]`, the current major
# version series (`## [4.2.0]` at measurement time); `### Removed`'s only
# occurrence is in `## [2.0.0]`, two major versions back. A word whose most
# recent use predates the current major is not "real, recurring usage" the
# way one still in current-era use is, and nothing in this mechanism's
# post-Rust history has needed `removed`. It can be added the same way, from
# real recent usage, if that changes. Growing the set further is still a
# contract change to docs/design/DESIGN-changelog-fragments.md, the same way growing
# tools/check-product-gate-scope.sh's exempt-path list is (see
# docs/design/DESIGN-ci.md, "Agent-configuration exemption"): name the new word,
# show it is measured from real, recent usage, and update
# changelog.d/README.md in the same change.
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
#   aggregate_changelog.sh check-section [--changelog FILE] [--version X.Y.Z]
#                                        [--summary-dir DIR]
#       Applies the fragment rules to an aggregated `## [X.Y.Z]` section of
#       CHANGELOG.md, where hand edits after `release` (and the pre-mechanism
#       `[Unreleased]` body `release` moves verbatim) are not covered by
#       `check`. Every top-level bullet must lead with a declared category
#       word and an id, `- <Category> (#N[, ...]):`; categories must follow
#       DECLARED_CATEGORY_ORDER; an (id, category) pair may appear once; and a
#       `breaking` / `breaking removal` label may appear only under `Changed`.
#       When the release summary DIR/vX.Y.Z.md exists (DIR defaults to
#       docs/release-notes beside the changelog), every id of every breaking
#       entry must also be referenced under its `## Breaking changes` heading
#       (issue #1806). Without --version it checks every released section from
#       CHECK_SECTION_FLOOR (5.0.0) on; older sections predate these rules and
#       are not checked (issue #1763).
#   aggregate_changelog.sh release --version X.Y.Z --date YYYY-MM-DD
#       Aggregates every fragment into a new `## [X.Y.Z] - YYYY-MM-DD`
#       section (preceded by whatever the existing `[Unreleased]` body still
#       holds, moved verbatim -- the migration pull request deliberately
#       left it there, see docs/design/DESIGN-changelog-fragments.md), verifies
#       conservation (control 5), rewrites CHANGELOG.md, and deletes the
#       consumed fragments -- all before returning, so the authority
#       handover from fragment to version section is a single atomic step
#       for the caller to commit.
#   aggregate_changelog.sh fold --version X.Y.Z [--changelog FILE]
#                               [--fragments-dir DIR]
#       Folds fragments that merged after the release commit into the
#       existing, newest `## [X.Y.Z]` section: each bullet rendered as
#       `release` renders it, placed where `release`'s order puts it, the
#       heading date unchanged, the fragments deleted, conservation verified
#       (verify_fold). With no fragments it changes nothing (issue #1806).
#   aggregate_changelog.sh selftest
#       Executes the accepting and rejecting fixtures for all six controls,
#       check-section (including its release-notes cross-check) and fold,
#       printing each rejecting fixture's exit status.

set -euo pipefail
export LC_ALL=C

# Absolute path to this script, resolved once, up front -- selftest's
# end-to-end control-1/control-4 fixtures `cd` into a throwaway git repo and
# re-invoke this tool, and `$0` alone would break there if the script was
# invoked with a relative path.
self_directory="$(dirname "$0")"
self_directory="$(cd "$self_directory" && pwd)"
self_basename="$(basename "$0")"
SELF="$self_directory/$self_basename"

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

# Basenames exempted from reject_unenumerable_fragments (below), alongside
# `README.md` (L3; review, #737, comment 2026-08-07, third round). Every one
# of these is a routine, benign byproduct of a contributor's local
# environment -- macOS Finder's `.DS_Store`, a directory-tracking
# placeholder, or an editor swap file -- never an attempt at a fragment:
# none end in `.md`, so none could ever match a declared category or be
# mistaken for lost content. Left unexempted, any of them sitting in
# `changelog.d/` fails local `check`/`check-pr`/`check-stale` with
# `changelog-fragment-path-invalid`, a routine false positive round one's
# reversal condition (a) treats as grounds for no-go. Git does not create or
# track these on its own, so a CI checkout is never affected either way;
# this exemption only changes local runs.
is_known_non_fragment_artifact() {
  case "$1" in
    .DS_Store|.gitkeep) return 0 ;;
    *.swp|*.swo) return 0 ;;
    *) return 1 ;;
  esac
}

# Rejects any regular file under $1 (at any depth) that `list_fragment_files`
# will not enumerate, the top-level `README.md` excepted. Two shapes share
# this one root cause and are both closed here in one place, not two
# shape-specific checks: `changelog.d/sub/2-x.added.md` (a subdirectory --
# `changelog.d/*.md` is a bash glob and `*` crosses `/`, so
# is_top_level_fragment_path's caller-side sibling check in control 1
# accepts it even though list_fragment_files' own glob, `"$dir"/*.md`, never
# enumerates anything below $dir itself) and `changelog.d/.9-hidden.added.md`
# (a top-level dotfile -- POSIX pathname expansion's bare `*` does not match
# a leading dot, so list_fragment_files silently skips it too, even though
# a `case` pattern match like is_top_level_fragment_path's is not pathname
# expansion and does match it). Closing only the subdirectory instance
# (review finding S2-3, #737, comment 2026-08-07) left the dotfile shape
# free to reproduce the identical silent loss end to end: check-pr passes,
# check and check-stale pass because they see an apparently-smaller (or
# empty) changelog.d/, and release aggregates every fragment it *can* see
# and reports success while the hidden one's entry never reaches
# CHANGELOG.md -- exactly the failure control 5 exists to prevent, entirely
# outside control 5's reach, because the fragment was never part of what
# control 5 compares against (review finding S3-1, #737, comment
# 2026-08-07, second round). Generalizing at the enumeration boundary
# itself -- "anything under $dir that list_fragment_files will not
# enumerate, README.md excepted" -- closes the whole class instead of one
# more instance of it.
#
# L3 exemption (review, #737, comment 2026-08-07, third round): a small,
# named set of routine, benign local-environment artifacts is exempted
# alongside README.md -- see is_known_non_fragment_artifact, below, for the
# full reasoning. None of them can ever be an attempted fragment (none end
# in `.md`), so exempting them costs this control nothing it exists to
# catch.
reject_unenumerable_fragments() {
  local dir="$1" f rel
  [ -d "$dir" ] || return 0
  # A newline-joined string, not an array: `"${arr[@]}"` on a zero-element
  # array is an unbound-variable error under `set -u` on bash 4.0-4.3, and
  # this dir legitimately has zero enumerable
  # fragments whenever every file under it is unenumerable (the exact case
  # this function exists to catch) or the dir holds only README.md (which
  # list_fragment_files itself always skips). `grep -qxF` below does exact
  # whole-line membership testing against this string instead of iterating
  # an array, so the empty case needs no special-casing at all.
  local enumerated found_paths
  enumerated="$(list_fragment_files "$dir")"
  found_paths="$(mktemp)" || fail "could not create fragment path inventory"
  find "$dir" -type f -print0 >"$found_paths" 2>/dev/null \
    || { rm -f "$found_paths"; fail "could not enumerate fragment paths under $dir"; }
  while IFS= read -r -d '' f; do
    # rel, not basename: a nested README.md ("sub/README.md") must not be
    # mistaken for the top-level exemption -- only $dir/README.md itself is
    # exempt, and it is already outside $dir/*.md in `find`'s own sense only
    # by virtue of failing the `enumerated` membership test below like any
    # other unenumerable path would, so the exemption is checked first, on
    # the full relative path, not the last path component.
    rel="${f#"$dir"/}"
    [ "$rel" = "README.md" ] && continue
    local basename_rel
    basename_rel="$(basename "$rel")"
    is_known_non_fragment_artifact "$basename_rel" && continue
    if ! printf '%s\n' "$enumerated" | grep -qxF "$rel"; then
      fail "changelog-fragment-path-invalid: $dir/$rel (not enumerable by list_fragment_files -- fragments must be direct, non-hidden children of $dir/ with a .md extension; only $dir/README.md is exempt)"
    fi
  done <"$found_paths"
  rm -f "$found_paths"
}

validate_fragment_names() {
  local dir="$1"
  reject_unenumerable_fragments "$dir"
  local -a files=()
  local f listed
  listed="$(list_fragment_files "$dir" | sort)" \
    || fail "could not list and sort fragments under $dir"
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$listed"
  # `"${files[@]}"` on a zero-element array is an unbound-variable error
  # under `set -u` on bash 4.0-4.3, so guard
  # the empty case -- an empty `changelog.d/` is trivially valid, not an
  # error, here.
  [ "${#files[@]}" -eq 0 ] && return 0
  for f in "${files[@]}"; do
    parse_fragment_name "$f" >/dev/null
  done
}

check_duplicates() {
  # Deliberately a plain indexed-array linear scan, not an associative
  # array: the simpler data structure is sufficient for the bounded input.
  # Fragment counts are small (dozens at most between releases), so the
  # O(n^2) scan is not a performance concern.
  local dir="$1"
  local -a files=()
  local f listed
  listed="$(list_fragment_files "$dir" | sort)" \
    || fail "could not list and sort fragments under $dir"
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$listed"
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
  local f listed
  listed="$(list_fragment_files "$dir")"
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$listed"
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

# Rejects a fragment body shape that would corrupt render_fragment's output
# even though the fragment itself is non-empty and conservation would still
# see its bytes reach the section -- content-scoped, so it complements
# control 5 (which only checks that content is conserved) and control 1
# (which only checks that content exists) rather than duplicating either.
# Two shapes are named because they were observed as concrete rendering
# hazards, not enumerated exhaustively: a stray carriage-return byte
# survives as a literal `\r` inside the Markdown file (this repository's
# fragments and CHANGELOG.md are LF-only, whether the `\r` arrived as a
# CRLF line ending or as a bare mid-line CR -- the byte, not its position,
# is what corrupts a diff/rendered view); a body whose first line already
# starts with a Markdown block marker renders wrong once the aggregator's
# own "- "/"  " markers are added in front of it -- `- ` or `* ` doubles the
# list marker (`- - like this`), and an ATX heading marker (`#` through
# `######`, followed by a space -- CommonMark's own definition, not a bare
# leading `#`) turns a bullet into a bulleted heading (`- ### Added`)
# instead of the heading the author's fragment content looks like it should
# have been. A bare `#NNN` issue reference with no following space (e.g. a
# body starting `#737: ...`) is not an ATX heading and must not be rejected
# on this basis (S4 correction; review, #737, comment 2026-08-07, second
# round: an earlier version of this check rejected any leading `#`
# regardless of a following space, which is broader than CommonMark's own
# heading grammar and would have misfired on exactly that shape).
validate_fragment_hygiene() {
  local file="$1" body first_line
  if LC_ALL=C grep -qU $'\r' "$file" 2>/dev/null; then
    fail "changelog-fragment-hygiene-invalid: $file (contains a carriage-return byte; fragments must use LF-only line endings, not CRLF or a bare CR)"
  fi
  body="$(trimmed_fragment_body "$file")"
  [ -z "$body" ] && return 0
  first_line="${body%%$'\n'*}"
  case "$first_line" in
    '- '*|'* '*|'+ '*)
      fail "changelog-fragment-hygiene-invalid: $file (body starts with a Markdown list marker, which would double up under the aggregator's own '- ')"
      ;;
  esac
  if [[ "$first_line" =~ ^#{1,6}[[:space:]] ]]; then
    fail "changelog-fragment-hygiene-invalid: $file (body starts with an ATX heading marker -- '#' through '######' followed by a space -- which would render as a bulleted heading)"
  fi
}

validate_fragment_hygiene_all() {
  local dir="$1"
  local -a files=()
  local f listed
  listed="$(list_fragment_files "$dir")"
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$listed"
  [ "${#files[@]}" -eq 0 ] && return 0
  for f in "${files[@]}"; do
    validate_fragment_hygiene "$dir/$f"
  done
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
  local f listed
  listed="$(sort_fragments "$dir")"
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$listed"
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
# aggregator that drops one fragment and duplicates another whose rendered
# block is a byte-for-byte prefix of the duplicate: the bullet count still
# matches and the dropped fragment's block is still found -- inside the
# duplicate. Per-position identity closes that: the dropped fragment's own
# position no longer holds its own block, so it fails.
verify_conservation() {
  local dir="$1" produced="$2"
  local -a sorted=()
  local f listed
  listed="$(sort_fragments "$dir")"
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$listed"
  local -a actual=()
  local seg bullet_file
  bullet_file="$(mktemp)" || fail "could not create rendered-bullet inventory"
  split_bullets "$produced" >"$bullet_file" \
    || { rm -f "$bullet_file"; fail "could not split rendered bullets"; }
  while IFS= read -r -d '' seg; do actual+=("$seg"); done <"$bullet_file"
  rm -f "$bullet_file"
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
# ("[X]: url"), which docs/design/RELEASE.md step 7 updates on every release.
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
    rust/*|src/fslc/*|specs/*|examples/*|docs/manual/LANGUAGE*|skills/fsl/references/*) return 0 ;;
    *) return 1 ;;
  esac
}

# The release-bump path set (H1; review, #737, comment 2026-08-07, third
# round): the fixed, small set of product-surface paths a genuine release
# commit legitimately touches without a fragment, and the *only* paths
# classify_product_diff's release exclusion (below) may waive -- not "every
# product-surface path in a diff classify_direct_edit labeled release-move",
# which is what the exclusion did through round two and which made
# "release-move" self-authorable: in the steady state this mechanism creates
# (`[Unreleased]`'s body permanently empty -- every entry lives under
# changelog.d/, and control 4 forbids editing the body directly), adding one
# line -- an empty `## [X.Y.Z] - YYYY-MM-DD` heading immediately after
# `## [Unreleased]` -- makes classify_direct_edit report "release-move" for
# free (there is no body to move, so the forgery costs nothing), and the old,
# diff-wide exclusion then waived every product-surface path in the same
# diff at no cost. Reproduced live before this fix: that one-line forgery,
# alongside an unrelated change to a new `rust/` file with no fragment,
# passed `check-pr`. Two non-adversarial diffs reached the same gap: a
# genuine release commit carrying an unrelated product change in the same
# commit (`docs/design/RELEASE.md` bundles steps 4-10 into one), and a branch whose
# `BASE_SHA` predates a release that has since landed on `main`.
#
# This set is deliberately not part of is_product_surface_path -- that
# predicate answers "does this path need a fragment at all", a broader
# question this control also relies on for ordinary (non-release) pull
# requests; this one answers "is this specific path part of what a release
# commit is allowed to bump for free", which only matters once
# classify_direct_edit has already validated the diff as a release move.
#
# Measured directly against this repository's release history, `git show
# --name-status --format= <sha>` on each of 56d5b1a (v4.2.0), 473239a
# (v4.1.0), 7b8607a (v4.0.0), e1dfdcb (v3.1.0): the touched path set is
# byte-identical across all four, every entry status `M` (no release commit
# adds a file here). Filtering that set through is_product_surface_path --
# the only paths this exemption can ever be consulted for, since
# classify_product_diff reaches it exclusively inside the
# `is_product_surface_path` branch -- leaves exactly the three paths below.
# `CHANGELOG.md` is excluded because it is governed separately, by
# classify_direct_edit/control 4, not by this control. `7b8607a`
# additionally touched `docs/design/RELEASE.md`, and every release touches
# `editors/vscode/package.json` and `editors/vscode/package-lock.json`; none
# of those three is a product surface under is_product_surface_path, so none
# of them ever reaches this predicate. Listing them here anyway would be
# unreachable code that reads like coverage: an earlier version of this set
# did list the two `editors/vscode/` paths, and deleting both left `selftest`
# fully green because nothing could ever call them (review finding F3, #737,
# comment 2026-08-08, fourth round).
#
# A maintainer must add an exact path here whenever either side of that
# filter moves: a docs/design/RELEASE.md release-commit step that starts touching a
# new product-surface path, or an is_product_surface_path that widens to
# cover a path releases already touch (`editors/vscode/*` is the live
# candidate). Otherwise every future release commit starts failing its own
# `changelog-fragment-missing`.
is_release_bump_path() {
  case "$1" in
    rust/Cargo.lock) return 0 ;;
    rust/Cargo.toml) return 0 ;;
    rust/fslc/tests/fixtures/domain_characterization/baseline.v1.json) return 0 ;;
    # #906's induction contract golden records each envelope's `versions`
    # block, so a release bump regenerates it for the same reason as the
    # characterization baseline above. Observed on the v4.4.1 candidate: the
    # complete product gate failed comparing 4.4.0 against 4.4.1 before this
    # path was named here.
    rust/fslc/tests/goldens/induction_cli_contract.json) return 0 ;;
    *) return 1 ;;
  esac
}

# A fragment path directly under $1 (top-level only -- see this function's
# neighbor, reject_unenumerable_fragments, for why `changelog.d/sub/x.added.md`
# must NOT match here even though the bash glob `changelog.d/*.md` alone
# would cross the `/`).
is_top_level_fragment_path() {
  case "$1" in
    changelog.d/*/*) return 1 ;;
    changelog.d/*.md) [ "$(basename "$1")" != "README.md" ] ;;
    *) return 1 ;;
  esac
}

# Reads "<status>\t<path>" lines (git diff --name-status shape) on stdin.
# Fails `changelog-fragment-missing: <paths>` if any changed path is a
# product surface and no fragment was added; the caller checks emptiness of
# each touched fragment separately (needs blob content, not just the diff).
# $1 is the direct-edit classification control 4 (classify_direct_edit,
# below) already computed for this same diff: "release-move" or "unchanged".
#
# Release exclusion (docs/design/DESIGN-changelog-fragments.md, control 1;
# corrected, review finding S2-1, #737, comment 2026-08-07, second round):
# the release commit itself (docs/design/RELEASE.md steps 4-7, committed together
# in step 10) bumps product-surface files (`rust/Cargo.toml`,
# `rust/Cargo.lock`, the domain characterization baseline) and *deletes* the
# fragments it aggregates -- it never *adds* one, so the rule as stated
# cannot be satisfied by the release process it must not block. An earlier
# version of this exclusion fired whenever the diff both deleted a top-level
# fragment and left CHANGELOG.md with git status `M`, on the theory that
# control 4 (classify_direct_edit) independently validates the edit is a
# genuine release move. It does not: classify_direct_edit's very first
# check returns "unchanged" the instant the `[Unreleased]` body's raw text
# is byte-identical, and that check inspects *only* the body -- an edit to
# CHANGELOG.md anywhere else (a stale link reference, a historical typo
# fix, a fabricated version section, or merely a trailing newline) leaves
# the body untouched and therefore "unchanged", while the old exclusion's
# own test was just git status `M`, which every one of those edits also
# satisfies. The two conditions never actually constrained each other; a
# non-release diff that deletes a fragment and touches CHANGELOG.md
# anywhere outside the body passed both, independently, every time.
# Reproduced live before this fix: four such diffs (a past-section typo, a
# link-reference-only edit, a bare trailing-newline append, and a
# fabricated version section) all passed `check-pr` (rejecting fixtures
# below).
#
# The fix removes the independent, weaker git-status test and makes this
# exclusion consume classify_direct_edit's own classification directly, so
# the two controls share one determination and cannot disagree: the
# exclusion now fires exactly when $1 is "release-move" -- classify_direct_edit's
# positive branch, reached only after it has confirmed the body was
# actually emptied and a matching new version heading was added, with
# nothing else in the file touched. A diff that deletes a fragment and
# edits CHANGELOG.md for some other reason still leaves the body
# byte-identical, classify_direct_edit still reports "unchanged", and this
# exclusion still does not fire, so `check-pr` still fails
# `changelog-fragment-missing`.
#
# The exclusion no longer additionally requires a deleted fragment (review
# finding S3-2, #737, comment 2026-08-07, second round): a release with
# zero fragments to aggregate is a legitimate release (a version-only
# bump), and classify_direct_edit's "release-move" classification is
# already the full, structurally-verified signal that a genuine release
# move occurred -- requiring a fragment deletion on top of it would reject
# exactly that legitimate zero-fragment case for no added safety, since
# nothing about the release process's validity depends on how many
# fragments happened to be waiting.
#
# Narrowed (H1; review, #737, comment 2026-08-07, third round): a
# "release-move" classification alone no longer waives every product-surface
# path in the diff -- only the ones in the fixed, measured set
# is_release_bump_path names (above). Through round two, "release-move" was
# self-authorable at zero cost: in the steady state this mechanism creates,
# `[Unreleased]`'s body is permanently empty, so one forged empty
# `## [X.Y.Z] - YYYY-MM-DD` heading right after `## [Unreleased]` makes
# classify_direct_edit report "release-move" for free (there is nothing to
# move, so nothing about the forgery costs anything), and the diff-wide
# exclusion then waived every product-surface path riding along in that same
# diff -- a genuinely unrelated `rust/` change with no fragment included.
# Two non-adversarial diffs reached the identical gap without any forgery: a
# real release commit that also carries an unrelated product change (docs/design/
# RELEASE.md bundles steps 4-10 into one commit), and a branch whose
# `BASE_SHA` predates a release that has since landed on `main`. Each
# product-surface path is now checked against is_release_bump_path
# individually: a path in that set is exempt exactly when $1 is
# "release-move" (unchanged from before); a path outside it still demands a
# fragment in the same diff, named individually in the same
# `changelog-fragment-missing` diagnostic. The forgery above now buys
# nothing -- the forged heading plus a change to, say,
# `rust/fsl-core/src/lib.rs` still fails, because `lib.rs` is not a release
# bump surface -- and both non-adversarial paths become correct rejections:
# a release commit carrying an unrelated product change now needs a
# fragment for that unrelated change, same as any other pull request would.
classify_product_diff() {
  local classification="${1:-unchanged}"
  local status path newpath
  local -a non_exempt=() added_fragments=() check_fragments=()
  while IFS=$'\t' read -r status path newpath; do
    [ -z "${status:-}" ] && continue
    # A rename or copy record has THREE tab-separated fields
    # (`R100<TAB>old<TAB>new`), not two, and `diff.renames` is on by default,
    # so this is the ordinary shape rather than an opt-in one. Read the third
    # field and classify the DESTINATION: with only two `read` variables the
    # trailing fields collapse into $path as `old<TAB>new`, and every
    # predicate below then tests the source. A file moved into a product
    # surface from outside it (`tools/x` -> `rust/x.rs`,
    # `docs/note.md` -> `specs/note.fsl`) escaped control 1 entirely that
    # way, with no forgery and no CHANGELOG.md edit -- whether control 1
    # fired depended on git's similarity heuristic rather than on what
    # changed, since the identical content change fails closed when git
    # happens to render it as A/D (review finding F1, #737, comment
    # 2026-08-08, fourth round). A rename *within* a product surface still
    # failed closed, but named the tab-joined pair in its diagnostic instead
    # of the destination. tools/check-product-gate-scope.sh, the sibling this
    # tool is modelled on, is immune for free: `git diff --name-only` prints
    # both sides of a rename on separate lines. This tool needs the status
    # column, so it handles the third field explicitly instead.
    case "$status" in
      R*|C*) [ -n "${newpath:-}" ] && path="$newpath" ;;
    esac
    if is_product_surface_path "$path"; then
      if [ "$classification" = "release-move" ] && is_release_bump_path "$path"; then
        : # Exempt: part of the fixed, measured release-bump path set,
          # and this diff is a validated release move. See this
          # function's own comment above, and is_release_bump_path's,
          # for why the exemption is scoped to that set rather than to
          # every product-surface path in the diff.
      else
        non_exempt+=("$path")
      fi
    fi
    if is_top_level_fragment_path "$path"; then
      case "$status" in
        A)
          added_fragments+=("$path")
          check_fragments+=("$path")
          ;;
        M)
          check_fragments+=("$path")
          ;;
      esac
    fi
  done
  if [ "${#non_exempt[@]}" -gt 0 ] && [ "${#added_fragments[@]}" -eq 0 ]; then
    fail "changelog-fragment-missing: ${non_exempt[*]}"
  fi
  # `"${arr[@]}"` on an empty array is an unbound-variable error under
  # `set -u` on bash 4.0-4.3, so guard the
  # common empty case explicitly instead of relying on a modern bash.
  if [ "${#check_fragments[@]}" -gt 0 ]; then
    printf '%s\n' "${check_fragments[@]}"
  fi
}

# ---- control 4b: direct-edit-forbidden, with the single release exclusion -

# Pure classifier over two whole-file CHANGELOG.md snapshots. See
# docs/design/DESIGN-changelog-fragments.md, control 4, and the migration
# correction: the only excluded diff shape left is the release move (the
# migration pull request does not touch the body at all). Prints
# "unchanged" on stdout if the `[Unreleased]` body's raw text and the
# heading immediately following it are both byte-identical between $base
# and $head (nothing about a release move happened -- this is the ordinary
# non-release pull request); prints "release-move" if the full release-move
# shape validates (body actually emptied, replaced by a new, correctly
# formatted version heading whose section starts with that same body, with
# nothing else in the file touched). Fails with
# `changelog-direct-edit-forbidden` for any other edit shape. This
# classification, not a pass/fail boolean, is the single source of truth
# both this control and control 1's release exclusion (classify_product_diff,
# above) consume (review finding S2-1, #737, comment 2026-08-07, second
# round) -- see that function's own comment for what went wrong when they
# were two independent tests instead.
#
# The "unchanged" test compares the heading immediately after `[Unreleased]`
# in addition to the body, not the body alone (S3-2 correction, same
# review): a genuine zero-fragment release, where the pre-existing body was
# already empty, produces byte-identical *body* text before and after (both
# are "nothing"), even though a real release move happened -- a new version
# heading appears where the old one used to be immediately following
# `[Unreleased]`. Comparing the body alone would misclassify that diff as
# "unchanged" and reopen exactly the S3-2 gap this correction closes.
# Comparing the heading too correctly reaches the release-move validation
# branch below for that diff, and correctly stays "unchanged" for the
# ordinary case where a pull request touches neither.
classify_direct_edit() {
  local base="$1" head="$2"

  grep -qxF '## [Unreleased]' "$base" || fail "changelog-direct-edit-forbidden: '## [Unreleased]' heading missing from the base revision"
  grep -qxF '## [Unreleased]' "$head" || fail "changelog-direct-edit-forbidden: '## [Unreleased]' heading was removed or renamed"

  local old_body new_body
  old_body="$(unreleased_raw_body "$base")"
  new_body="$(unreleased_raw_body "$head")"

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

  if [ "$old_body" = "$new_body" ] && [ "$head_first_heading" = "$base_first_heading" ]; then
    echo "unchanged"
    return 0
  fi

  local new_bullets
  new_bullets="$(printf '%s\n' "$new_body" | trim_blank_edges)"
  if [ -n "$new_bullets" ]; then
    fail "changelog-direct-edit-forbidden: the [Unreleased] body was edited directly, or still holds content that was not moved into a new version section"
  fi

  local old_bullets
  old_bullets="$(printf '%s\n' "$old_body" | trim_blank_edges)"

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

  echo "release-move"
}

# Thin pass/fail wrapper over classify_direct_edit's classification, for
# callers (most of selftest_control4's pure-predicate fixtures) that only
# care whether the edit is allowed, not what it was classified as.
check_direct_edit_files() {
  classify_direct_edit "$1" "$2" >/dev/null
}

# git-blob-backed wrapper: resolves BASE_SHA/HEAD_SHA to CHANGELOG.md
# snapshots at their merge base and HEAD_SHA, and returns
# classify_direct_edit's classification on stdout. check_pr calls this
# exactly once per invocation and feeds the result to both this control's
# own fail-closed behavior (a bad edit fails here, before control 1 ever
# runs) and control 1's release exclusion -- one computation, consumed
# twice, so the two controls cannot disagree (review finding S2-1, #737,
# comment 2026-08-07, second round). $1 is the repository-relative changelog
# path and must be supplied explicitly by the caller.
compute_direct_edit_classification() {
  local changelog="$1"
  : "${BASE_SHA:?BASE_SHA is required}"
  : "${HEAD_SHA:?HEAD_SHA is required}"
  # Diff against the merge base, not BASE_SHA's tip: this repository's own
  # convention for a pull-request/merge-group diff is three-dot
  # (`git diff "$BASE_SHA...$HEAD_SHA"`, used in `ci.yml` and by
  # classify_product_diff's caller below), which compares against
  # `git merge-base BASE_SHA HEAD_SHA`, not BASE_SHA itself. BASE_SHA is
  # `github.event.pull_request.base.sha` -- the base branch's *current* tip,
  # which moves every time something else lands on it. Reading CHANGELOG.md
  # straight from BASE_SHA therefore compares this branch's unchanged
  # [Unreleased] body against a DIFFERENT [Unreleased] body -- the one a
  # later release already moved under a version heading -- and every open
  # pull request that never touched CHANGELOG.md fails
  # `changelog-direct-edit-forbidden` the moment a release lands on main.
  local base_sha
  base_sha="$(git merge-base "$BASE_SHA" "$HEAD_SHA" 2>/dev/null)" \
    || fail "changelog-direct-edit-forbidden: cannot compute the merge base of $BASE_SHA and $HEAD_SHA"
  local tmp
  tmp="$(mktemp -d)"
  # `trap ... RETURN` is a single global registration, not scoped to this
  # function: left as-is, it would fire again on the *next* function's
  # return too, referencing a `$tmp` that has since gone out of scope and
  # aborting with "unbound variable" under `set -u`. Clearing it inside its
  # own body makes it fire exactly once.
  trap 'rm -rf "$tmp"; trap - RETURN' RETURN
  git show "$base_sha:$changelog" >"$tmp/base.md" 2>/dev/null || fail "changelog-direct-edit-forbidden: cannot read $changelog at $base_sha"
  git show "$HEAD_SHA:$changelog" >"$tmp/head.md" 2>/dev/null || fail "changelog-direct-edit-forbidden: cannot read $changelog at $HEAD_SHA"
  classify_direct_edit "$tmp/base.md" "$tmp/head.md"
}

# $1 is the direct-edit classification (see compute_direct_edit_classification
# above); check_pr computes it once and passes it here.
check_missing_or_empty_fragment() {
  local classification="${1:-}"
  [ -n "$classification" ] || fail "internal error: check_missing_or_empty_fragment requires a direct-edit classification"
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
  diff_output="$(git diff --name-status "$BASE_SHA...$HEAD_SHA" | classify_product_diff "$classification")"
  status=$?
  set -e
  [ "$status" -eq 0 ] || exit 1
  # classify_product_diff prints both newly added fragments and fragments
  # this diff *modifies*: emptiness must be checked on either, or a pull
  # request that edits an existing fragment down to whitespace (status `M`,
  # never `A`) passes silently -- the same failure control 1 exists to
  # catch, just reached through an edit instead of an omission.
  local -a to_check=()
  local f
  while IFS= read -r f; do
    [ -n "$f" ] && to_check+=("$f")
  done <<<"$diff_output"
  [ "${#to_check[@]}" -eq 0 ] && return 0
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"; trap - RETURN' RETURN
  for f in "${to_check[@]}"; do
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
  validate_fragment_hygiene_all "$dir"
  echo "aggregate_changelog: check PASS -- every fragment in '$dir' has a conforming name and no duplicate (id, category)"
}

check_pr() {
  local dir="${1:-changelog.d}"
  validate_fragment_names "$dir"
  check_duplicates "$dir"
  validate_fragment_hygiene_all "$dir"
  # Computed once, before either control that depends on it: control 4's own
  # fail-closed check (an invalid direct edit fails right here) and control
  # 1's release exclusion both consume this single classification, so they
  # cannot disagree about whether this diff is a genuine release move (S2-1).
  local classification status
  set +e
  classification="$(compute_direct_edit_classification "CHANGELOG.md")"
  status=$?
  set -e
  [ "$status" -eq 0 ] || exit 1
  check_missing_or_empty_fragment "$classification"
  echo "aggregate_changelog: check-pr PASS"
}

check_stale() {
  local dir="${1:-changelog.d}"
  # Same enumeration-boundary guard as validate_fragment_names: a fragment
  # `list_fragment_files` cannot see (a subdirectory, or a top-level
  # dotfile) must not be able to reach tag time invisibly either (S3-1).
  reject_unenumerable_fragments "$dir"
  local -a files=()
  local f listed
  listed="$(list_fragment_files "$dir")"
  while IFS= read -r f; do [ -n "$f" ] && files+=("$f"); done <<<"$listed"
  if [ "${#files[@]}" -gt 0 ]; then
    fail "stale-fragments-present: ${files[*]}"
  fi
  echo "aggregate_changelog: check-stale PASS -- '$dir' has no fragments left"
}

# ---- release-section check (issue #1763) ----------------------------------
#
# `check` sees fragments, never what they become: a released section can still
# be edited by hand (docs/design/RELEASE.md step 7; #1732 and #1733 did so for
# 5.0.0), and `release` moves the pre-mechanism `[Unreleased]` body in
# verbatim. This applies the fragment rules to the section itself.
#
# Ids are the `#N` tokens inside the lead's parentheses, folded like fragment
# ids (`#0691` is 691). Every id in a `refs` list (`refs #A`, `refs #A, #B`)
# is a cross-reference, not the entry's own id, so `Fixed (#1678, refs #1281)`
# and `Fixed (#1281)` do not collide. A breaking label is a label item that
# starts with the word `breaking`; `non-breaking` and `not breaking` are not
# breaking labels. The breaking rule is narrower than "not under Fixed" on purpose:
# a breaking change modifies existing behavior, which is `changed`'s
# definition in changelog.d/README.md, and readers look for breaking entries
# there. Within a category, id order is not checked: the 5.0.0 section keeps
# the pre-mechanism body's and #1733's late entries where they were placed.
#
# Sections before CHECK_SECTION_FLOOR are not checked: they were written before
# these rules (4.x sections use other lead shapes and repeat ids), and
# rewriting released history is not this check's purpose.
CHECK_SECTION_FLOOR=5.0.0

# Succeeds when SemVer $1 >= $2.
version_at_least() {
  local a="$1" b="$2" i
  local -a x y
  IFS=. read -r -a x <<<"$a"
  IFS=. read -r -a y <<<"$b"
  for i in 0 1 2; do
    if ((10#${x[$i]} > 10#${y[$i]})); then return 0; fi
    if ((10#${x[$i]} < 10#${y[$i]})); then return 1; fi
  done
  return 0
}

# Prints the version of every `## [X.Y.Z]` heading in $1, in file order.
released_section_versions() {
  awk '
    /^## \[[0-9]+\.[0-9]+\.[0-9]+\]/ {
      v = substr($0, 5); print substr(v, 1, index(v, "]") - 1)
    }
  ' "$1"
}

# Prints one diagnostic line per violation in section $2 of $1; exits 1 if
# there was any, 2 if the section does not exist. When $3 names a file, it
# also writes one line per bullet whose lead parses, in section order:
# "<bullet number>\t<category>\t<first id>\t<breaking 0|1>\t<line>\t<id,id,...>"
# (ids exclude `refs` lists). fold and the release-notes cross-check read
# these leads instead of parsing the section a second way.
check_section_body() {
  local changelog="$1" version="$2" leads_out="${3:-}"
  awk -v want="$version" -v cats="${DECLARED_CATEGORY_ORDER[*]}" -v leads_out="$leads_out" '
    function report(code, msg) {
      printf "%s: [%s] %s:%d: %s\n", code, want, FILENAME, NR, msg
      bad++
    }
    BEGIN {
      n = split(cats, c, " ")
      for (i = 1; i <= n; i++) rank[c[i]] = i
      heading = "## [" want "]"
    }
    /^## \[/ {
      if (in_section) { in_section = 0; done = 1 }
      if (!done && substr($0, 1, length(heading)) == heading) { in_section = 1; found = 1 }
      next
    }
    !in_section { next }
    /^[ \t]*$/ { next }
    /^- / {
      bullets++
      line = $0
      if (line !~ /^- [A-Z][a-z]+ \(/) {
        report("changelog-section-lead-invalid", "bullet does not start with `- <Category> (`: " substr(line, 1, 60))
        next
      }
      op = index(line, " (")
      word = substr(line, 3, op - 3)
      rest = substr(line, op + 2)
      cl = index(rest, "):")
      paren = substr(rest, 1, cl - 1)
      if (cl == 0 || index(paren, "(") || index(paren, ")")) {
        report("changelog-section-lead-invalid", "bullet lead has no `(...)`: label: " substr(line, 1, 60))
        next
      }
      cat = tolower(word)
      if (!(cat in rank)) {
        report("changelog-section-category-undeclared", "`" word "` is not a declared category")
        next
      }
      if (rank[cat] < last_rank) {
        report("changelog-section-order-invalid", "`" word "` after `" last_word "` breaks the declared category order")
      }
      if (rank[cat] > last_rank) { last_rank = rank[cat]; last_word = word }
      # The label is a comma-separated list of items. An item is a breaking
      # label only when it starts with the word `breaking` (`breaking`,
      # `breaking removal`), so `non-breaking` and `not breaking` are not.
      # An item starting with `refs` opens a reference list: its ids, and
      # those of the bare `#N` items that follow it (`refs #A, #B`), are
      # cross-references, not ids of the entry itself.
      labels = paren
      gsub(/;/, ",", labels)
      m = split(labels, item, ",")
      ids = 0; breaking = 0; in_refs = 0; first = 0; idlist = ""
      for (j = 1; j <= m; j++) {
        s = item[j]
        sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s)
        ls = tolower(s)
        if (ls ~ /^refs([ \t]|$)/) { in_refs = 1; continue }
        if (in_refs && s ~ /^#[0-9]+$/) continue
        in_refs = 0
        if (ls ~ /^breaking([ \t]|$)/) breaking = 1
        t = split(s, tok, /[ \t]+/)
        for (i = 1; i <= t; i++) {
          if (tok[i] !~ /^#[0-9]+$/) continue
          id = substr(tok[i], 2) + 0
          ids++
          if (ids == 1) first = id
          idlist = idlist (ids > 1 ? "," : "") id
          key = id SUBSEP cat
          if (key in seen) {
            report("changelog-section-duplicate-id", "#" id " appears twice under `" word "` (first at line " seen[key] ")")
          } else {
            seen[key] = NR
          }
        }
      }
      if (breaking && cat != "changed") {
        report("changelog-section-breaking-outside-changed", "`" word " (" paren ")` carries a breaking label outside `Changed`")
      }
      if (ids == 0) {
        report("changelog-section-id-missing", "`" word " (" paren ")` names no issue or pull-request id")
      } else if (leads_out != "") {
        printf "%d\t%s\t%d\t%d\t%d\t%s\n", bullets, cat, first, breaking, NR, idlist > leads_out
      }
      next
    }
    /^(  |\t)/ { if (bullets) next }
    { report("changelog-section-line-invalid", "not a bullet, a continuation line or a blank line: " substr($0, 1, 60)) }
    END {
      if (!found) { printf "changelog-section-missing: no `## [%s]` section in %s\n", want, FILENAME; exit 2 }
      exit (bad > 0)
    }
  ' "$changelog"
}

# Release-notes cross-check (issue #1806). A release with a hand-written
# summary publishes that summary, not the section, as its GitHub Release body
# (tools/release-notes.sh build), so a breaking entry the summary leaves out
# is invisible to a reader of the release. Every id of every breaking entry
# in $1 (check_section_body's leads) must appear as a `#N` token inside the
# summary's `## Breaking changes` section, up to the next `## ` heading.
# `refs` ids are not the entry's own and are not required. Without a summary
# the release body is the section itself, so there is nothing to compare.
# Prints one diagnostic per unlisted id and returns 1 if there was any;
# otherwise prints the number of breaking ids checked.
check_release_notes_breaking() {
  local leads="$1" summary="$2" version="$3" changelog="$4"
  local required listed
  required="$(awk -F '\t' '$4 == 1 { n = split($6, a, ","); for (i = 1; i <= n; i++) print a[i] "\t" $5 }' "$leads")"
  if [ -z "$required" ]; then
    echo 0
    return 0
  fi
  listed="$(awk '
    /^## / { in_breaking = ($0 ~ /^## Breaking changes[ \t]*$/); next }
    in_breaking {
      line = $0
      while (match(line, /#[0-9]+/)) {
        print substr(line, RSTART + 1, RLENGTH - 1) + 0
        line = substr(line, RSTART + RLENGTH)
      }
    }
  ' "$summary" | sort -u)"
  local id line missing=0 count=0
  while IFS=$'\t' read -r id line; do
    count=$((count + 1))
    if ! printf '%s\n' "$listed" | grep -qxF "$id"; then
      echo "changelog-section-breaking-unlisted: [$version] $changelog:$line: breaking entry #$id is not referenced under \`## Breaking changes\` in $summary" >&2
      missing=$((missing + 1))
    fi
  done <<<"$required"
  [ "$missing" -eq 0 ] || return 1
  echo "$count"
}

check_section() {
  local changelog="CHANGELOG.md" version="" summary_dir=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --changelog) [ "$#" -ge 2 ] || fail "check-section: --changelog needs a value"; changelog="$2"; shift 2 ;;
      --version) [ "$#" -ge 2 ] || fail "check-section: --version needs a value"; version="$2"; shift 2 ;;
      --summary-dir) [ "$#" -ge 2 ] || fail "check-section: --summary-dir needs a value"; summary_dir="$2"; shift 2 ;;
      *) echo "usage: $0 check-section [--changelog FILE] [--version X.Y.Z] [--summary-dir DIR]" >&2; exit 2 ;;
    esac
  done
  [ -f "$changelog" ] || fail "check-section: '$changelog' not found"
  # Beside the changelog, as release-notes.sh's DEFAULT_SUMMARY_DIR is beside
  # the default CHANGELOG.md, so a fixture changelog never reads the
  # repository's own summaries.
  [ -n "$summary_dir" ] || summary_dir="$(dirname "$changelog")/docs/release-notes"
  local -a versions=()
  local v listed
  if [ -n "$version" ]; then
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "check-section: --version must be SemVer X.Y.Z, got '$version'"
    versions=("$version")
  else
    listed="$(released_section_versions "$changelog")" \
      || fail "check-section: could not list the sections of '$changelog'"
    while IFS= read -r v; do
      if [ -n "$v" ] && version_at_least "$v" "$CHECK_SECTION_FLOOR"; then
        versions+=("$v")
      fi
    done <<<"$listed"
    # Fail closed: a heading shape this does not recognize must not turn the
    # check into a vacuous pass.
    [ "${#versions[@]}" -gt 0 ] \
      || fail "changelog-section-missing: no released section at or above $CHECK_SECTION_FLOOR in '$changelog'"
  fi
  local status failed=0 tmp summary count notes=""
  tmp="$(mktemp -d)" || fail "check-section: could not create a scratch directory"
  for v in "${versions[@]}"; do
    : >"$tmp/leads"
    set +e
    check_section_body "$changelog" "$v" "$tmp/leads" >&2
    status=$?
    set -e
    [ "$status" -eq 0 ] || failed=1
    summary="$summary_dir/v$v.md"
    if [ ! -f "$summary" ]; then
      notes="$notes; $v: no $summary, the release body is the section"
    elif [ "$status" -ne 2 ]; then
      set +e
      count="$(check_release_notes_breaking "$tmp/leads" "$summary" "$v" "$changelog")"
      status=$?
      set -e
      if [ "$status" -eq 0 ]; then
        notes="$notes; $v: $count breaking id(s) listed in $summary"
      else
        failed=1
      fi
    fi
  done
  rm -rf "$tmp"
  [ "$failed" -eq 0 ] \
    || fail "aggregate_changelog: check-section FAIL -- '$changelog' section(s) ${versions[*]} break the fragment rules or the release-notes cross-check (see above)"
  echo "aggregate_changelog: check-section PASS -- '$changelog' section(s) ${versions[*]}: declared category leads in order, one (id, category) each, breaking labels only under Changed$notes"
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
  validate_fragment_hygiene_all "$fragdir"

  local -a sorted=()
  local f listed
  listed="$(sort_fragments "$fragdir")"
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$listed"
  # An empty `changelog.d/` is a legitimate release, not an error (review
  # finding S3-2, #737, comment 2026-08-07, second round): a version bump
  # with no product-facing content since the previous release is a real
  # release shape, and this command must agree with check-pr's release
  # exclusion (classify_product_diff, tools/aggregate_changelog.sh), which
  # no longer requires a deleted fragment either -- see docs/design/RELEASE.md,
  # step 7. `no-fragments-to-aggregate` previously hard-failed here, which
  # made step 7 impossible to run for that release shape at all. Guarded
  # with `-gt 0` before the loop below (rather than only inside it) because
  # `"${sorted[@]}"` on a zero-element array is an unbound-variable error
  # under `set -u` on bash 4.0-4.3.
  if [ "${#sorted[@]}" -gt 0 ]; then
    for f in "${sorted[@]}"; do
      fragment_is_empty "$fragdir/$f" && fail "changelog-fragment-empty: $fragdir/$f"
    done
  fi

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

  if [ "${#sorted[@]}" -gt 0 ]; then
    for f in "${sorted[@]}"; do
      rm -f "$fragdir/$f"
    done
  fi

  echo "aggregate_changelog: released $version, aggregated ${#sorted[@]} fragment(s), '$fragdir' is now empty"
}

# ---- post-release-commit fold (issue #1806) --------------------------------
#
# A fragment that merges after the release commit and before the tag is not
# in the `## [X.Y.Z]` section `release` wrote, and check-stale refuses the tag
# while it sits in changelog.d/. 5.0.0 folded such fragments by hand five
# times (#1733, #1790, #1793, #1796, #1802): each bullet byte-identical to
# what `release` renders, placed where `release`'s order puts it, the heading
# date unchanged. `fold` does exactly that.
#
# Placement is a stable merge of the sorted fragments into the section's
# existing bullets: a fragment goes immediately before the first bullet whose
# (declared category rank, first id) is greater than the fragment's
# (rank, id), or after the section's last bullet when there is none. The
# section's own order within a category is not re-sorted: 5.0.0 keeps
# pre-mechanism entries out of id order (see check_section_body), and a
# fold must not move them.
#
# Refusals, each before anything is written:
#   fold-section-missing      no `## [X.Y.Z] - YYYY-MM-DD` heading, or more than one
#   fold-section-not-latest   X.Y.Z is not the section right after `[Unreleased]`
#   fold-unreleased-not-empty `[Unreleased]` has content `release` would have
#                             moved into X.Y.Z; fold cannot place it
#   fold-version-tagged       tag vX.Y.Z exists in the changelog's repository
#   fold-section-invalid      the section already breaks check-section's rules
#   fold-duplicate-entry      a fragment's (id, category) is already an entry
#                             of the section
#   fold-conservation-failed  verify_fold rejected the result
# With no fragments it changes nothing and exits 0, so a second run is a
# no-op. The release notes are prose and are not edited here; check-section's
# release-notes cross-check fails if a folded breaking entry is not listed.

# Prints every line of $1 outside the body of the `## [$2]` section (its
# heading included), so two changelogs that differ only inside that body
# print the same text.
outside_section_body() {
  awk -v heading="## [$2]" '
    /^## \[/ { in_section = (!done && substr($0, 1, length(heading)) == heading); if (in_section) done = 1; print; next }
    !in_section { print }
  ' "$1"
}

# The raw body of the `## [$2]` section of $1 (between its heading and the
# next `## [` heading), with a trailing "." so command substitution keeps
# its trailing blank lines.
section_body_raw() {
  awk -v heading="## [$2]" '
    /^## \[/ { if (in_section) exit; if (substr($0, 1, length(heading)) == heading) in_section = 1; next }
    in_section { print }
  ' "$1"
  printf '.'
}

# Counts the leading and trailing blank lines of stdin as "<lead> <trail>".
blank_edges() {
  awk '
    { lines[NR] = $0 }
    END {
      s = 1; while (s <= NR && lines[s] ~ /^[ \t]*$/) s++
      e = NR; while (e >= s && lines[e] ~ /^[ \t]*$/) e--
      print (s - 1), (NR - e)
    }
  '
}

# Conservation for fold, the counterpart of verify_conservation: $2 (the
# written changelog) must equal $1 (the changelog before the fold) outside
# the body of `## [$3]`, keep that body's leading and trailing blank lines
# when it had bullets, and hold exactly the old bullets plus one rendered
# bullet per fragment of $4 -- the old bullets byte for byte and in their old
# order, the new ones byte for byte and in control 3's order. Every bullet is
# consumed from the front of one of the two queues, so a dropped, duplicated,
# reordered or edited bullet fails. Each folded bullet must also follow
# exactly as many old bullets as the placement rule gives, recomputed here
# from the old section's leads rather than taken from fold's awk. The result
# must still pass check_section_body.
verify_fold() {
  local before="$1" after="$2" version="$3" fragdir="$4"
  local outside_before outside_after
  outside_before="$(outside_section_body "$before" "$version")"
  outside_after="$(outside_section_body "$after" "$version")"
  [ "$outside_before" = "$outside_after" ] \
    || fail "fold-conservation-failed: '$after' differs from '$before' outside the body of ## [$version]"
  local old_raw new_raw old_body new_body old_edges new_edges
  old_raw="$(section_body_raw "$before" "$version")"; old_raw="${old_raw%.}"
  new_raw="$(section_body_raw "$after" "$version")"; new_raw="${new_raw%.}"
  old_body="$(printf '%s' "$old_raw" | trim_blank_edges)"
  new_body="$(printf '%s' "$new_raw" | trim_blank_edges)"
  old_edges="$(printf '%s' "$old_raw" | blank_edges)"
  new_edges="$(printf '%s' "$new_raw" | blank_edges)"
  if [ -n "$old_body" ] && [ "$old_edges" != "$new_edges" ]; then
    fail "fold-conservation-failed: the blank lines around the bullets of ## [$version] changed"
  fi
  local -a old=() new=() frag=()
  local seg f listed scratch
  scratch="$(mktemp)" || fail "could not create a bullet inventory"
  if [ -n "$old_body" ]; then
    split_bullets "$old_body" >"$scratch"
    while IFS= read -r -d '' seg; do old+=("$seg"); done <"$scratch"
  fi
  split_bullets "$new_body" >"$scratch"
  while IFS= read -r -d '' seg; do new+=("$seg"); done <"$scratch"
  rm -f "$scratch"
  # Old bullet keys (category rank, first id), in section order.
  local -a old_rank=() old_id=() after_old=()
  local leads_file lead_cat lead_first
  leads_file="$(mktemp)" || fail "could not create a lead inventory"
  check_section_body "$before" "$version" "$leads_file" >/dev/null 2>&1 || true
  while IFS=$'\t' read -r _ lead_cat lead_first _; do
    old_rank+=("$(category_index "$lead_cat")")
    old_id+=("$lead_first")
  done <"$leads_file"
  rm -f "$leads_file"
  [ "${#old_rank[@]}" -eq "${#old[@]}" ] \
    || fail "fold-conservation-failed: ## [$version] of '$before' has ${#old[@]} bullet(s) but ${#old_rank[@]} parsed lead(s)"
  local meta frag_rank frag_id p
  listed="$(sort_fragments "$fragdir")"
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    frag+=("$(render_fragment "$fragdir/$f")")
    meta="$(parse_fragment_name "$f")"
    frag_id="${meta%%$'\t'*}"
    frag_rank="$(category_index "${meta##*$'\t'}")"
    p=0
    while [ "$p" -lt "${#old_rank[@]}" ] \
      && ! { [ "${old_rank[$p]}" -gt "$frag_rank" ] || { [ "${old_rank[$p]}" -eq "$frag_rank" ] && [ "${old_id[$p]}" -gt "$frag_id" ]; }; }; do
      p=$((p + 1))
    done
    after_old+=("$p")
  done <<<"$listed"
  if [ "${#new[@]}" -ne $((${#old[@]} + ${#frag[@]})) ]; then
    fail "fold-conservation-failed: ## [$version] has ${#new[@]} bullet(s), expected ${#old[@]} existing + ${#frag[@]} folded"
  fi
  local i=0 j=0 k
  for k in "${!new[@]}"; do
    if [ "$j" -lt "${#frag[@]}" ] && [ "${new[$k]}" = "${frag[$j]}" ]; then
      [ "$i" -eq "${after_old[$j]}" ] \
        || fail "fold-conservation-failed: bullet $((k + 1)) of ## [$version] follows $i existing bullet(s); release's order puts it after ${after_old[$j]}"
      j=$((j + 1))
    elif [ "$i" -lt "${#old[@]}" ] && [ "${new[$k]}" = "${old[$i]}" ]; then
      i=$((i + 1))
    else
      fail "fold-conservation-failed: bullet $((k + 1)) of ## [$version] is neither the next existing bullet nor the next folded fragment"
    fi
  done
  check_section_body "$after" "$version" >&2 \
    || fail "fold-conservation-failed: the folded ## [$version] breaks the check-section rules (see above)"
}

fold() {
  local version="" changelog="CHANGELOG.md" fragdir="changelog.d"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version) [ "$#" -ge 2 ] || fail "fold: --version needs a value"; version="$2"; shift 2 ;;
      --changelog) [ "$#" -ge 2 ] || fail "fold: --changelog needs a value"; changelog="$2"; shift 2 ;;
      --fragments-dir) [ "$#" -ge 2 ] || fail "fold: --fragments-dir needs a value"; fragdir="$2"; shift 2 ;;
      *) echo "usage: $0 fold --version X.Y.Z [--changelog FILE] [--fragments-dir DIR]" >&2; exit 2 ;;
    esac
  done
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "fold: --version must be SemVer X.Y.Z, got '$version'"
  [ -f "$changelog" ] || fail "fold: '$changelog' not found"

  validate_fragment_names "$fragdir"
  check_duplicates "$fragdir"
  validate_fragment_hygiene_all "$fragdir"
  local -a sorted=()
  local f listed
  listed="$(sort_fragments "$fragdir")"
  while IFS= read -r f; do [ -n "$f" ] && sorted+=("$f"); done <<<"$listed"
  if [ "${#sorted[@]}" -gt 0 ]; then
    for f in "${sorted[@]}"; do
      fragment_is_empty "$fragdir/$f" && fail "changelog-fragment-empty: $fragdir/$f"
    done
  fi

  local headings first_heading
  headings="$(awk -v heading="## [$version]" 'substr($0, 1, length(heading)) == heading' "$changelog")"
  [[ "$headings" =~ ^\#\#\ \[[0-9]+\.[0-9]+\.[0-9]+\]\ -\ [0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] \
    || fail "fold-section-missing: '$changelog' needs exactly one '## [$version] - YYYY-MM-DD' heading"
  grep -qxF '## [Unreleased]' "$changelog" \
    || fail "fold-section-not-latest: '$changelog' has no '## [Unreleased]' heading"
  first_heading="$(after_unreleased "$changelog")"
  first_heading="${first_heading%%$'\n'*}"
  [ "$first_heading" = "$headings" ] \
    || fail "fold-section-not-latest: '$headings' is not the section right after '## [Unreleased]' ('$first_heading' is)"
  local unreleased
  unreleased="$(unreleased_raw_body "$changelog" | trim_blank_edges)"
  [ -z "$unreleased" ] \
    || fail "fold-unreleased-not-empty: '## [Unreleased]' in '$changelog' has content that release would have moved into $version"
  # After the tag, changelog.d/ holds the next release's fragments. Only a
  # tag ref present locally is seen: an unfetched tag, or a changelog outside
  # a Git work tree, is not.
  if git -C "$(dirname "$changelog")" rev-parse -q --verify "refs/tags/v$version^{commit}" >/dev/null 2>&1; then
    fail "fold-version-tagged: v$version is already tagged; its fragments belong to the next release"
  fi

  # A global and an EXIT trap: every refusal below exits through fail().
  FOLD_TMP="$(mktemp -d)" || fail "fold: could not create a scratch directory"
  trap 'rm -rf "$FOLD_TMP"' EXIT
  local tmp="$FOLD_TMP"
  : >"$tmp/leads"
  check_section_body "$changelog" "$version" "$tmp/leads" >&2 \
    || fail "fold-section-invalid: ## [$version] in '$changelog' breaks the check-section rules (see above)"

  if [ "${#sorted[@]}" -eq 0 ]; then
    echo "aggregate_changelog: fold $version -- '$fragdir' has no fragments, '$changelog' unchanged"
    return 0
  fi

  local meta id cat idx line i=0
  : >"$tmp/manifest"
  for f in "${sorted[@]}"; do
    meta="$(parse_fragment_name "$f")"
    id="${meta%%$'\t'*}"
    cat="${meta##*$'\t'}"
    line="$(awk -F '\t' -v cat="$cat" -v id="$id" '
      $2 == cat { n = split($6, a, ","); for (i = 1; i <= n; i++) if (a[i] + 0 == id + 0) { print $5; exit } }
    ' "$tmp/leads")"
    [ -z "$line" ] \
      || fail "fold-duplicate-entry: $fragdir/$f: (#$id, $cat) is already an entry of ## [$version] ($changelog:$line)"
    idx="$(category_index "$cat")"
    i=$((i + 1))
    render_fragment "$fragdir/$f" >"$tmp/bullet.$i"
    printf '%s\t%s\t%s\n' "$idx" "$id" "$tmp/bullet.$i" >>"$tmp/manifest"
  done

  awk -v heading="## [$version]" -v cats="${DECLARED_CATEGORY_ORDER[*]}" \
      -v leads="$tmp/leads" -v manifest="$tmp/manifest" '
    BEGIN {
      n = split(cats, c, " ")
      for (i = 1; i <= n; i++) rank[c[i]] = i - 1
      while ((getline l < leads) > 0) { split(l, f, "\t"); brank[f[1]] = rank[f[2]]; bid[f[1]] = f[3] + 0 }
      while ((getline l < manifest) > 0) { split(l, f, "\t"); nf++; frank[nf] = f[1] + 0; fid[nf] = f[2] + 0; fpath[nf] = f[3] }
      next_f = 1
    }
    # Prints the queued fragments that sort before (r, id); r = n + 1 drains the queue.
    function emit_before(r, id,    l) {
      while (next_f <= nf && (frank[next_f] < r || (frank[next_f] == r && fid[next_f] < id))) {
        while ((getline l < fpath[next_f]) > 0) print l
        close(fpath[next_f])
        next_f++
      }
    }
    function flush_blanks() { for (; blanks > 0; blanks--) print "" }
    function end_section() {
      if (content) { emit_before(n + 1, 0); flush_blanks() }
      else { blanks = 0; print ""; emit_before(n + 1, 0); print "" }
      in_section = 0
    }
    /^## \[/ {
      if (in_section) end_section()
      else if (!done && substr($0, 1, length(heading)) == heading) { in_section = 1; done = 1 }
      print
      next
    }
    !in_section { print; next }
    /^[ \t]*$/ { blanks++; next }
    /^- / { bullet++; flush_blanks(); emit_before(brank[bullet], bid[bullet]); print; content = 1; next }
    { flush_blanks(); print; content = 1 }
    END { if (in_section) end_section() }
  ' "$changelog" >"$changelog.new" || { rm -f "$changelog.new"; fail "fold: could not write '$changelog.new'"; }

  ( verify_fold "$changelog" "$changelog.new" "$version" "$fragdir" ) \
    || { rm -f "$changelog.new"; exit 1; }
  mv "$changelog.new" "$changelog"
  for f in "${sorted[@]}"; do
    rm -f "$fragdir/$f"
  done
  echo "aggregate_changelog: folded ${#sorted[@]} fragment(s) into $version (heading '$headings' unchanged), '$fragdir' is now empty"
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

  # Rejecting: a conforming-looking fragment name hidden in a subdirectory
  # (review finding S2-3, #737, comment 2026-08-07). It satisfies control 1's
  # shallow diff-path check and parse_fragment_name itself would happily
  # accept its basename, but list_fragment_files never enumerates it, so
  # without reject_unenumerable_fragments it is invisible to controls 2, 3,
  # and 6, to check-stale, and to release's aggregation -- the entry is
  # silently lost with no diagnostic naming the cause.
  mkdir -p "$tmp/sub"
  st_setup_frag "$tmp/sub" "2-x.added.md" "Added (#2): x, hidden in a subdirectory."
  st_expect_fail "control6 rejecting: fragment in a subdirectory (changelog-fragment-path-invalid)" \
    reject_unenumerable_fragments "$tmp"
  st_expect_fail "control6 rejecting: validate_fragment_names also rejects a subdirectory fragment" \
    validate_fragment_names "$tmp"
  rm -rf "$tmp/sub"

  # Rejecting (review finding S3-1, #737, comment 2026-08-07, second round):
  # a *top-level* dotfile reproduces the identical silent loss through a
  # completely different mechanism -- not a path crossing `/`, but POSIX
  # pathname expansion's own exclusion of a leading dot from a bare `*`
  # glob, which list_fragment_files' `"$dir"/*.md` relies on and
  # is_top_level_fragment_path's `case` pattern match does not share (`case`
  # patterns are not pathname expansion, so its `*` matches a leading dot
  # fine). Closing only the subdirectory instance above left this sibling
  # shape free to pass check-pr, check, and check-stale, and be silently
  # skipped by release's aggregation -- generalizing
  # reject_unenumerable_fragments to "anything list_fragment_files will not
  # enumerate" (not just "anything in a subdirectory") closes this instance
  # with the same one check, not a third shape-specific one.
  st_setup_frag "$tmp" ".9-hidden.added.md" "Added (#9): a hidden dotfile fragment."
  st_expect_fail "control6 rejecting: top-level dotfile fragment (changelog-fragment-path-invalid)" \
    reject_unenumerable_fragments "$tmp"
  st_expect_fail "control6 rejecting: validate_fragment_names also rejects a dotfile fragment" \
    validate_fragment_names "$tmp"
  st_expect_fail "control6 rejecting: check-stale also rejects a dotfile fragment left at tag time" \
    check_stale "$tmp"
  rm -f "$tmp"/.9-hidden.added.md

  # Accepting: the top-level README.md itself is still exempt, not swept up
  # by the generalization above.
  printf '# README\n' >"$tmp/README.md"
  st_expect_pass "control6 accepting: README.md alone is exempt from enumerability, not rejected" \
    reject_unenumerable_fragments "$tmp"
  rm -f "$tmp/README.md"

  # Accepting (L3; review, #737, comment 2026-08-07, third round): routine,
  # benign local-environment artifacts must not fail check/check-pr/check-stale
  # -- none of them ends in `.md`, so none is an attempted fragment.
  printf 'binary junk\n' >"$tmp/.DS_Store"
  printf '' >"$tmp/.gitkeep"
  printf 'swap file contents\n' >"$tmp/.2-x.added.md.swp"
  st_expect_pass "control6 accepting (L3): .DS_Store, .gitkeep, and a vim .swp file are exempt, not rejected" \
    reject_unenumerable_fragments "$tmp"
  rm -f "$tmp/.DS_Store" "$tmp/.gitkeep" "$tmp/.2-x.added.md.swp"

  # Rejecting: the L3 exemption is a small, closed, exact-match set -- it
  # must not widen into anything that could actually hide a fragment. A file
  # ending in `.md.bak` or a misspelled `.ds_store` is neither `README.md`
  # nor in is_known_non_fragment_artifact's set, and must still be rejected.
  printf 'Added (#9): hidden by a near-miss artifact name.\n' >"$tmp/9-x.added.md.bak"
  st_expect_fail "control6 rejecting (L3): a near-miss artifact name is not exempted (changelog-fragment-path-invalid)" \
    reject_unenumerable_fragments "$tmp"
  rm -f "$tmp/9-x.added.md.bak"

  # M2 (required; review, #737, comment 2026-08-07, third round): a mutant
  # that loosens reject_unenumerable_fragments' README.md exemption test
  # from `rel` (the full relative path under $dir) to `basename` left
  # `selftest` fully green, even though the code comment right above the
  # check explicitly argues for the `rel`/`basename` distinction: only
  # `$dir/README.md` itself is exempt, not a same-named file nested in a
  # subdirectory. The code already rejects `changelog.d/sub/README.md`;
  # nothing in the suite proved it.
  mkdir -p "$tmp/sub"
  printf '# README\n' >"$tmp/sub/README.md"
  st_expect_fail "control6 rejecting (M2): a nested README.md is not the top-level exemption (changelog-fragment-path-invalid)" \
    reject_unenumerable_fragments "$tmp"
  rm -rf "$tmp/sub"

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
  local lexicographic_numeric_display
  lexicographic_numeric_display="$(printf '%s' "$lexicographic_numeric" | tr '\n' ' ')" \
    || fail "could not format numeric-order selftest output"
  echo "selftest: rejecting fixture 'control3 rejecting: lexicographic sham vs numeric golden' produced: $lexicographic_numeric_display"
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
  local lexicographic_category_display
  lexicographic_category_display="$(printf '%s' "$lexicographic_category" | tr '\n' ' ')" \
    || fail "could not format category-order selftest output"
  echo "selftest: rejecting fixture 'control3 rejecting: lexicographic-category sham vs category golden' produced: $lexicographic_category_display"
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
  local dropped first_render third_render
  first_render="$(render_fragment "$tmp/1-a.added.md")"
  third_render="$(render_fragment "$tmp/3-c.added.md")"
  dropped="${first_render}"$'\n'"${third_render}"
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
  local duplicated duplicated_render
  duplicated_render="$(render_fragment "$tmp/2-b.added.md")"
  duplicated="${duplicated_render}"$'\n'"${duplicated_render}"
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

  # Release exclusion (review finding S2-1, #737, comment 2026-08-07, second
  # round): the release commit bumps product-surface files and only
  # *deletes* fragments (docs/design/RELEASE.md steps 4-7, one commit per step 10);
  # it never adds one. classify_product_diff no longer decides this for
  # itself from the diff's file statuses -- it takes control 4's own
  # classification (classify_direct_edit, via compute_direct_edit_classification)
  # as an explicit argument, and grants the exclusion only when that
  # classification is exactly "release-move". These fixtures exercise the
  # classifier's own logic directly with a literal classification argument;
  # the end-to-end fixtures below (selftest_release_exclusion) additionally
  # prove that check-pr, driven by the real BASE_SHA/HEAD_SHA git-blob
  # wrapper, actually computes that classification correctly and threads it
  # through -- an isolated unit fixture like this one cannot, by itself,
  # prove the two controls are wired together rather than merely both
  # accepting the same literal string by coincidence.
  # Names every reachable is_release_bump_path entry, not just the two
  # version-string files: the domain characterization baseline is the one
  # entry in that set that is a generated product artifact rather than a
  # version bump, and deleting it from the set used to leave `selftest` fully
  # green -- a gutted mutant of this round's own new constant (review finding
  # F2, #737, comment 2026-08-08, fourth round). Every entry in the set is
  # exercised here and in the zero-fragment fixture below, so dropping any of
  # them now fails closed with `changelog-fragment-missing`.
  st_expect_pass "control1 accepting: release shape, given a genuine release-move classification (every release-bump path, F2)" \
    bash -c 'printf "M\trust/Cargo.toml\nM\trust/Cargo.lock\nM\trust/fslc/tests/fixtures/domain_characterization/baseline.v1.json\nD\tchangelog.d/2-x.added.md\nM\tCHANGELOG.md\n" | classify_product_diff release-move'
  # The regression case for the original S2-1 bug: the exact same file-status
  # shape (fragment deleted, CHANGELOG.md modified) that the earlier,
  # over-wide exclusion granted on git status alone, now given the
  # classification an *unvalidated* edit actually produces ("unchanged" --
  # classify_direct_edit's own early return whenever the [Unreleased] body
  # is byte-identical, which every one of the four falsified variants in
  # selftest_release_exclusion below produces). Must still fail.
  st_expect_fail "control1 rejecting: release-shaped diff, but classification says the body was never touched (changelog-fragment-missing)" \
    bash -c 'printf "M\trust/Cargo.toml\nM\trust/Cargo.lock\nD\tchangelog.d/2-x.added.md\nM\tCHANGELOG.md\n" | classify_product_diff unchanged'
  # Zero-fragment release (review finding S3-2, #737, comment 2026-08-07,
  # second round): the exclusion no longer requires a deleted fragment at
  # all, only a validated release-move classification -- a version-only
  # bump with nothing accumulated in changelog.d/ is still a legitimate
  # release.
  st_expect_pass "control1 accepting: release-move classification alone is sufficient, with zero fragments deleted (S3-2)" \
    bash -c 'printf "M\trust/Cargo.toml\nM\trust/Cargo.lock\nM\trust/fslc/tests/fixtures/domain_characterization/baseline.v1.json\nM\tCHANGELOG.md\n" | classify_product_diff release-move'
  # Narrowed exclusion (H1; review, #737, comment 2026-08-07, third round):
  # a "release-move" classification only waives the measured release-bump
  # path set (is_release_bump_path), not every product-surface path riding
  # along in the same diff. A path outside that set -- here,
  # `rust/fsl-core/src/lib.rs`, an ordinary source file -- still demands a
  # fragment even though the diff is a genuine, validated release move, and
  # the diagnostic names only that path, not the exempt ones alongside it.
  st_expect_fail "control1 rejecting: release-move classification does not exempt a product path outside the release-bump set (H1, changelog-fragment-missing)" \
    bash -c 'printf "M\trust/Cargo.toml\nM\trust/Cargo.lock\nM\trust/fsl-core/src/lib.rs\nM\tCHANGELOG.md\n" | classify_product_diff release-move'
  # The pre-H1-fix version of this exclusion made "release-move" itself
  # self-authorable at zero cost, because the whole diff was waived once
  # that one classification held. Confirm the diagnostic names exactly the
  # non-exempt path and none of the exempt ones (this checks message
  # content, so it must *pass*, not fail, when the diagnostic is correct).
  # shellcheck disable=SC2016  # single-quoted on purpose: $out must expand
  # inside the child `bash -c`, not this outer script, when it is parsed.
  st_expect_pass "control1: the fragment-missing diagnostic for a mixed release-move diff names only the non-exempt path (H1)" \
    bash -c 'out="$(printf "M\trust/Cargo.toml\nM\trust/Cargo.lock\nM\trust/fsl-core/src/lib.rs\nM\tCHANGELOG.md\n" | classify_product_diff release-move 2>&1)"; [ "$out" = "changelog-fragment-missing: rust/fsl-core/src/lib.rs" ]'
  # The exclusion must still be exactly as narrow as a validated release
  # move: neither an unvalidated fragment deletion nor an unvalidated
  # CHANGELOG.md touch, alone, satisfies it -- both must still fail closed
  # with the default ("unchanged") classification.
  st_expect_fail "control1 rejecting: fragment deleted but classification is not release-move (changelog-fragment-missing)" \
    bash -c 'printf "M\trust/Cargo.toml\nD\tchangelog.d/2-x.added.md\n" | classify_product_diff'
  st_expect_fail "control1 rejecting: CHANGELOG.md touched but classification is not release-move (changelog-fragment-missing)" \
    bash -c 'printf "M\trust/Cargo.toml\nM\tCHANGELOG.md\n" | classify_product_diff'
  # A fragment hidden in a subdirectory must not count as coverage either
  # (review finding S2-3): is_top_level_fragment_path excludes it, so this
  # still fails exactly like the no-fragment case above.
  st_expect_fail "control1 rejecting: only a nested fragment was added (changelog-fragment-missing)" \
    bash -c 'printf "M\trust/fsl-core/src/lib.rs\nA\tchangelog.d/sub/2-x.added.md\n" | classify_product_diff'
  # Rename/copy records carry three tab-separated fields, and the
  # DESTINATION is what determines whether a fragment is owed (review
  # finding F1, #737, comment 2026-08-08, fourth round). A file moved into a
  # product surface from outside it must fail exactly like a plain addition;
  # before the fix, $path held `old<TAB>new` and every predicate tested the
  # source, so this passed.
  st_expect_fail "control1 rejecting: a rename INTO a product surface still owes a fragment (F1, changelog-fragment-missing)" \
    bash -c 'printf "R100\ttools/mover.txt\trust/moved.rs\n" | classify_product_diff'
  st_expect_fail "control1 rejecting: a rename-with-edit into specs/ still owes a fragment (F1, changelog-fragment-missing)" \
    bash -c 'printf "R084\tdocs/note.md\tspecs/note.fsl\n" | classify_product_diff'
  st_expect_fail "control1 rejecting: a copy INTO a product surface still owes a fragment (F1, changelog-fragment-missing)" \
    bash -c 'printf "C075\ttools/mover.txt\trust/copied.rs\n" | classify_product_diff'
  # The diagnostic must name the destination alone, not the tab-joined pair
  # the two-variable read produced. Checks message content, so it must pass.
  # shellcheck disable=SC2016  # single-quoted on purpose: $out must expand
  # inside the child `bash -c`, not this outer script, when it is parsed.
  st_expect_pass "control1: the fragment-missing diagnostic for a rename names the destination, not the source (F1)" \
    bash -c 'out="$(printf "R100\trust/keep.rs\trust/renamed.rs\n" | classify_product_diff 2>&1)"; [ "$out" = "changelog-fragment-missing: rust/renamed.rs" ]'
  # The accepting half of the same pair: the identical rename WITH a
  # conforming fragment in the same diff passes, so the rejecting fixtures
  # above are calibrated against a real difference in the diff, not against
  # rename records being rejected unconditionally.
  st_expect_pass "control1 accepting: a rename INTO a product surface, with a fragment in the same diff (F1)" \
    bash -c 'printf "R100\ttools/mover.txt\trust/moved.rs\nA\tchangelog.d/3-x.added.md\n" | classify_product_diff'
  # A rename that leaves a product surface owes nothing: the destination is
  # not a product surface, even though the source was. This is the direction
  # the pre-fix code got accidentally "right" for the wrong reason, so pin
  # it explicitly now that the field it reads has changed.
  st_expect_pass "control1 accepting: a rename OUT of a product surface owes no fragment (F1)" \
    bash -c 'printf "R100\trust/moved.rs\ttools/mover.txt\n" | classify_product_diff'

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

  # Body hygiene (review finding S4-4): non-empty, but a shape that would
  # corrupt the rendered bullet.
  printf 'Added (#1): normal content.\n' >"$tmp/hygienic.md"
  st_expect_pass "control1 accepting: hygienic fragment body" validate_fragment_hygiene "$tmp/hygienic.md"
  printf 'Added (#1): a line.\r\nAnother line.\r\n' >"$tmp/crlf.md"
  st_expect_fail "control1 rejecting: CRLF line ending (changelog-fragment-hygiene-invalid)" \
    validate_fragment_hygiene "$tmp/crlf.md"
  # A bare mid-line CR, not a CRLF pair, must be caught by the same check
  # (S4 correction; review, #737, comment 2026-08-07, second round): the
  # rejection was always this broad (`grep -qU $'\r'` matches any CR byte),
  # only the diagnostic wording claimed otherwise ("CRLF line ending"). This
  # fixture proves the detection matches the now-corrected wording.
  printf 'Added (#1): weird\rmid-line CR, not a CRLF pair.\n' >"$tmp/bare-cr.md"
  st_expect_fail "control1 rejecting: bare mid-line CR, not CRLF (changelog-fragment-hygiene-invalid)" \
    validate_fragment_hygiene "$tmp/bare-cr.md"
  printf -- '- Added (#1): looks like a bullet already.\n' >"$tmp/leading-dash.md"
  st_expect_fail "control1 rejecting: body starts with a list marker (changelog-fragment-hygiene-invalid)" \
    validate_fragment_hygiene "$tmp/leading-dash.md"
  printf '### Added\n' >"$tmp/leading-heading.md"
  st_expect_fail "control1 rejecting: body starts with an ATX heading marker (changelog-fragment-hygiene-invalid)" \
    validate_fragment_hygiene "$tmp/leading-heading.md"
  # Narrowing fixture (S4 correction, same review comment): a bare `#NNN`
  # issue reference with no following space is not an ATX heading
  # (CommonMark requires '#' through '######' followed by a space) and must
  # be accepted, not rejected. An earlier version of this check rejected
  # any leading '#' regardless of a following space, which would have
  # misfired on exactly this ordinary fragment-body shape.
  printf '#737: a bare issue reference, not a heading.\n' >"$tmp/hash-no-space.md"
  st_expect_pass "control1 accepting: leading '#NNN' with no space is not an ATX heading" \
    validate_fragment_hygiene "$tmp/hash-no-space.md"
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

  # M2 (required; review, #737, comment 2026-08-07, third round): a mutant
  # that deletes classify_direct_edit's version-heading regex
  # (`[[ ... =~ ^\#\#\ \[[0-9]+\.[0-9]+\.[0-9]+\]\ -\ [0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]`)
  # left `selftest` fully green -- that regex is the *only* gate on the
  # "release-move" shape, precisely the shape H1 forges. The code already
  # rejects a non-version heading (`is not a version heading`); nothing in
  # the suite exercised it. Body emptied (a real release-move attempt), but
  # the new heading is not `## [X.Y.Z] - YYYY-MM-DD`.
  printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [Next release, soon]\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/vNext...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >"$head"
  st_expect_fail "control4b rejecting (M2): new heading is not a version heading (changelog-direct-edit-forbidden)" check_direct_edit_files "$base" "$head"

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

  # End-to-end (review finding S2-1, #737, comment 2026-08-07): a full
  # release pull request -- product-surface changes (rust/Cargo.toml,
  # rust/Cargo.lock) and fragment deletions in the same diff, produced by
  # the real `release` subcommand, exactly as docs/design/RELEASE.md steps 4-10
  # commit it. Before this fix, this failed
  # `changelog-fragment-missing: rust/Cargo.lock rust/Cargo.toml`, and the
  # documented workaround of adding a dummy fragment only moved the failure
  # to `check-stale` at tag time -- both reproduced live against this
  # script, not asserted, while diagnosing the finding.
  local relrepo="$tmp/release-repo"
  mkdir -p "$relrepo/rust" "$relrepo/changelog.d"
  (
    cd "$relrepo"
    git init -q
    git config user.email "selftest@example.invalid"
    git config user.name "selftest"
    printf '[workspace.package]\nversion = "1.0.0"\n' >rust/Cargo.toml
    printf 'lockfile v1\n' >rust/Cargo.lock
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n' >CHANGELOG.md
    printf 'Added (#2): a real fragment.\n' >changelog.d/2-x.added.md
    git add -A
    git commit -q -m base
  )
  local rel_base_sha; rel_base_sha="$(cd "$relrepo" && git rev-parse HEAD)"
  (
    cd "$relrepo"
    printf '[workspace.package]\nversion = "1.1.0"\n' >rust/Cargo.toml
    printf 'lockfile v1.1\n' >rust/Cargo.lock
    "$SELF" release --version 1.1.0 --date 2026-08-07
    git add -A
    git commit -q -m "chore(release): v1.1.0"
  )
  local rel_head_sha; rel_head_sha="$(cd "$relrepo" && git rev-parse HEAD)"
  st_expect_pass "control1 end-to-end accepting: check-pr on a real release pull request (product bump + fragment consumption)" \
    bash -c "cd '$relrepo' && BASE_SHA='$rel_base_sha' HEAD_SHA='$rel_head_sha' '$SELF' check-pr"

  # End-to-end (review finding S2-2, #737, comment 2026-08-07): a feature
  # branch that never touches CHANGELOG.md must still pass check-pr after a
  # release has landed on the branch it forked from and moved BASE_SHA's own
  # CHANGELOG.md to a different [Unreleased] body. Before this fix,
  # check_direct_edit read CHANGELOG.md straight from BASE_SHA (the base
  # branch's current tip) instead of the merge base, so this failed
  # `changelog-direct-edit-forbidden` for an edit the branch never made --
  # reproduced live before the fix, in a repo built exactly this way.
  local mbrepo="$tmp/mergebase-repo"
  mkdir -p "$mbrepo/rust" "$mbrepo/changelog.d"
  (
    cd "$mbrepo"
    git init -q -b main
    git config user.email "selftest@example.invalid"
    git config user.name "selftest"
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n' >CHANGELOG.md
    printf 'fn main() {}\n' >rust/lib.rs
    printf 'Added (#4): a prior fragment.\n' >changelog.d/4-x.added.md
    git add -A
    git commit -q -m base
  )
  (
    cd "$mbrepo"
    git checkout -q -b feature
    printf 'fn main() { /* feature work, no CHANGELOG.md touch */ }\n' >rust/lib.rs
    printf 'Added (#5): a feature.\n' >changelog.d/5-x.added.md
    git add -A
    git commit -q -m "feature work"
  )
  local mb_feature_sha; mb_feature_sha="$(cd "$mbrepo" && git rev-parse HEAD)"
  (
    cd "$mbrepo"
    git checkout -q main
    "$SELF" release --version 1.1.0 --date 2026-08-07
    git add -A
    git commit -q -m "chore(release): v1.1.0"
  )
  local mb_new_main_sha; mb_new_main_sha="$(cd "$mbrepo" && git rev-parse HEAD)"
  st_expect_pass "control4b end-to-end accepting: base advanced by a release, feature branch never touched CHANGELOG.md" \
    bash -c "cd '$mbrepo' && BASE_SHA='$mb_new_main_sha' HEAD_SHA='$mb_feature_sha' '$SELF' check-pr"

  # End-to-end (review finding S4-3, #737, comment 2026-08-07): editing an
  # already-tracked fragment down to whitespace (status `M`, never `A`) must
  # still fail. classify_product_diff previously only reported *added*
  # fragments to the emptiness check.
  (
    cd "$mbrepo"
    git checkout -q feature
    printf '   \n' >changelog.d/5-x.added.md
    git add -A
    git commit -q -m "empty out an existing fragment"
  )
  local mb_emptied_sha; mb_emptied_sha="$(cd "$mbrepo" && git rev-parse HEAD)"
  st_expect_fail "control1 end-to-end rejecting: an existing fragment edited down to whitespace (changelog-fragment-empty)" \
    bash -c "cd '$mbrepo' && BASE_SHA='$mb_feature_sha' HEAD_SHA='$mb_emptied_sha' '$SELF' check-pr"

  # End-to-end (review finding F1, #737, comment 2026-08-08, fourth round):
  # a real `git mv` into a product surface, driven through the real check-pr,
  # against real git rename detection -- the unit fixtures above feed
  # classify_product_diff a hand-written `R100` line and so cannot prove git
  # actually emits that shape here, nor that check-pr's own `git diff
  # --name-status` invocation reaches this code with three fields. Both the
  # rejecting case (no fragment) and its accepting control (the identical
  # rename plus a fragment) run against the same base, so a green accepting
  # case cannot come from the rename simply being ignored again.
  local mvrepo="$tmp/rename-repo"
  mkdir -p "$mvrepo/rust" "$mvrepo/tools" "$mvrepo/changelog.d"
  (
    cd "$mvrepo"
    git init -q -b main
    git config user.email "selftest@example.invalid"
    git config user.name "selftest"
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): one.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n' >CHANGELOG.md
    printf 'fn main() {}\n' >rust/lib.rs
    printf 'a helper that is about to move into the product surface\nline two\nline three\n' >tools/mover.txt
    git add -A
    git commit -q -m base
  )
  local mv_base_sha; mv_base_sha="$(cd "$mvrepo" && git rev-parse HEAD)"
  (
    cd "$mvrepo"
    git checkout -q -b move-no-fragment
    git mv tools/mover.txt rust/moved.rs
    git commit -q -m "move a file into rust/ with no fragment"
  )
  local mv_head_sha; mv_head_sha="$(cd "$mvrepo" && git rev-parse HEAD)"
  st_expect_fail "control1 end-to-end rejecting (F1): a real git mv into rust/ with no fragment (changelog-fragment-missing)" \
    bash -c "cd '$mvrepo' && BASE_SHA='$mv_base_sha' HEAD_SHA='$mv_head_sha' '$SELF' check-pr"
  (
    cd "$mvrepo"
    git checkout -q -b move-with-fragment "$mv_base_sha"
    git mv tools/mover.txt rust/moved.rs
    printf 'Changed (#6): moved the helper into the native workspace.\n' >changelog.d/6-move.changed.md
    git add -A
    git commit -q -m "move a file into rust/ with a fragment"
  )
  local mv_ok_sha; mv_ok_sha="$(cd "$mvrepo" && git rev-parse HEAD)"
  st_expect_pass "control1 end-to-end accepting (F1): the same git mv, with a fragment in the same pull request" \
    bash -c "cd '$mvrepo' && BASE_SHA='$mv_base_sha' HEAD_SHA='$mv_ok_sha' '$SELF' check-pr"

  rm -rf "$tmp"
}

# End-to-end coverage for the S2-1 release-exclusion fix and the S3-2
# zero-fragment release decision (review, #737, comment 2026-08-07, second
# round). selftest_control1's unit fixtures exercise classify_product_diff
# directly with a literal classification argument; they cannot, by
# themselves, prove check-pr's real BASE_SHA/HEAD_SHA git-blob wrapper
# actually computes that classification from CHANGELOG.md's content and
# threads the *same* value into both controls, rather than the two
# happening to agree on a hand-picked string. These fixtures close that gap
# by driving the real `check-pr` subcommand end to end in a throwaway git
# repository, the same way selftest_control4's fixtures do.
selftest_release_exclusion() {
  local tmp; tmp="$(mktemp -d)"

  # Shared base shape for the four rejecting variants below: a product
  # change with no new fragment, a fragment *deletion* (the exact shape the
  # original, over-wide exclusion granted on git status alone), and a
  # CHANGELOG.md edit that never touches the [Unreleased] body or the
  # heading immediately following it -- so classify_direct_edit reports
  # "unchanged", not "release-move", for every one of them.
  build_release_exclusion_base() {
    local repo="$1"
    mkdir -p "$repo/rust" "$repo/changelog.d"
    (
      cd "$repo"
      git init -q
      git config user.email "selftest@example.invalid"
      git config user.name "selftest"
      printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): an existing pending entry, untouched by any variant below.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
      printf 'fn main() {}\n' >rust/lib.rs
      printf 'Added (#2): a fragment about to be deleted, not aggregated.\n' >changelog.d/2-x.added.md
      git add -A
      git commit -q -m base
    )
  }

  # Variant A: a typo fix inside a *past* version section's body -- not the
  # [Unreleased] body, not the heading immediately following it.
  local repo_a="$tmp/variant-a"
  build_release_exclusion_base "$repo_a"
  local a_base; a_base="$(cd "$repo_a" && git rev-parse HEAD)"
  (
    cd "$repo_a"
    printf 'fn main() { /* unrelated product change, no fragment */ }\n' >rust/lib.rs
    rm -f changelog.d/2-x.added.md
    sed -i.bak 's/^- Old\.$/- Olde. (typo fix, unrelated to any release)./' CHANGELOG.md
    rm -f CHANGELOG.md.bak
    git add -A
    git commit -q -m "variant A: typo fix in a past version section"
  )
  local a_head; a_head="$(cd "$repo_a" && git rev-parse HEAD)"
  st_expect_fail "S2-1 rejecting, variant A: typo fix in a past version section (changelog-fragment-missing)" \
    bash -c "cd '$repo_a' && BASE_SHA='$a_base' HEAD_SHA='$a_head' '$SELF' check-pr"

  # Variant B: update the trailing "[Unreleased]:" link reference only.
  local repo_b="$tmp/variant-b"
  build_release_exclusion_base "$repo_b"
  local b_base; b_base="$(cd "$repo_b" && git rev-parse HEAD)"
  (
    cd "$repo_b"
    printf 'fn main() { /* unrelated product change, no fragment */ }\n' >rust/lib.rs
    rm -f changelog.d/2-x.added.md
    sed -i.bak 's#^\[Unreleased\]: https://example/compare/v1.0.0\.\.\.HEAD$#[Unreleased]: https://example/compare/v1.0.0...main#' CHANGELOG.md
    rm -f CHANGELOG.md.bak
    git add -A
    git commit -q -m "variant B: update the [Unreleased]: link reference only"
  )
  local b_head; b_head="$(cd "$repo_b" && git rev-parse HEAD)"
  st_expect_fail "S2-1 rejecting, variant B: [Unreleased]: link-reference-only edit (changelog-fragment-missing)" \
    bash -c "cd '$repo_b' && BASE_SHA='$b_base' HEAD_SHA='$b_head' '$SELF' check-pr"

  # Variant C (the sharpest): append a single trailing newline. The
  # smallest possible CHANGELOG.md edit that still satisfies a git-status-only
  # exclusion test.
  local repo_c="$tmp/variant-c"
  build_release_exclusion_base "$repo_c"
  local c_base; c_base="$(cd "$repo_c" && git rev-parse HEAD)"
  (
    cd "$repo_c"
    printf 'fn main() { /* unrelated product change, no fragment */ }\n' >rust/lib.rs
    rm -f changelog.d/2-x.added.md
    printf '\n' >>CHANGELOG.md
    git add -A
    git commit -q -m "variant C: append a single trailing newline to CHANGELOG.md"
  )
  local c_head; c_head="$(cd "$repo_c" && git rev-parse HEAD)"
  st_expect_fail "S2-1 rejecting, variant C (sharpest): a single trailing newline appended to CHANGELOG.md (changelog-fragment-missing)" \
    bash -c "cd '$repo_c' && BASE_SHA='$c_base' HEAD_SHA='$c_head' '$SELF' check-pr"

  # Variant D: insert a fabricated version section immediately after
  # [Unreleased], without actually emptying [Unreleased]'s own body into it
  # -- an attempt to *look* like a release move without being one.
  # classify_direct_edit rejects this outright (the body still holds
  # content that was never moved), so check-pr fails via control 4's own
  # check, before control 1 ever runs -- still an overall check-pr failure,
  # which is what this fixture proves.
  local repo_d="$tmp/variant-d"
  build_release_exclusion_base "$repo_d"
  local d_base; d_base="$(cd "$repo_d" && git rev-parse HEAD)"
  (
    cd "$repo_d"
    printf 'fn main() { /* unrelated product change, no fragment */ }\n' >rust/lib.rs
    rm -f changelog.d/2-x.added.md
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): an existing pending entry, untouched by any variant below.\n\n## [9.9.9] - 2026-01-01\n\n- Fake.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
    git add -A
    git commit -q -m "variant D: insert a fabricated version section"
  )
  local d_head; d_head="$(cd "$repo_d" && git rev-parse HEAD)"
  st_expect_fail "S2-1 rejecting, variant D: a fabricated version section, body never actually moved (changelog-direct-edit-forbidden)" \
    bash -c "cd '$repo_d' && BASE_SHA='$d_base' HEAD_SHA='$d_head' '$SELF' check-pr"

  # H1 (blocking; review, #737, comment 2026-08-07, third round): the
  # exclusion S2-1/S3-2 built above turned out to be too wide in the other
  # direction. In the steady state this mechanism creates -- `[Unreleased]`'s
  # body permanently empty, every entry living under changelog.d/, and
  # control 4 forbidding a direct body edit -- adding *one line*, an empty
  # `## [X.Y.Z] - YYYY-MM-DD` heading immediately after `## [Unreleased]`,
  # makes classify_direct_edit report "release-move" for free: there is no
  # body to move, so the forgery costs nothing. Before this fix, that one
  # line then exempted *every* product-surface path riding along in the same
  # diff. Reproduced live, exit 0 (PASS -- the hole), before this fix; both
  # now fail with exit 1 naming the actual non-exempt path, after it.
  build_h1_steady_state_base() {
    local repo="$1"
    mkdir -p "$repo/rust" "$repo/changelog.d"
    (
      cd "$repo"
      git init -q
      git config user.email "selftest@example.invalid"
      git config user.name "selftest"
      printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
      printf 'fn main() {}\n' >rust/lib.rs
      git add -A
      git commit -q -m base
    )
  }

  # H1 variant a: forged heading plus a change to an existing tracked file.
  local repo_h1a="$tmp/h1-forged-heading"
  build_h1_steady_state_base "$repo_h1a"
  local h1a_base; h1a_base="$(cd "$repo_h1a" && git rev-parse HEAD)"
  (
    cd "$repo_h1a"
    printf 'fn main() { /* unrelated product change, no fragment, riding on a forged release heading */ }\n' >rust/lib.rs
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [9.9.9] - 2026-08-07\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
    git add -A
    git commit -q -m "H1 forgery: empty version heading forged after [Unreleased], unrelated product change, no fragment"
  )
  local h1a_head; h1a_head="$(cd "$repo_h1a" && git rev-parse HEAD)"
  st_expect_fail "H1 rejecting: a forged empty version heading no longer exempts an unrelated product-surface change (changelog-fragment-missing)" \
    bash -c "cd '$repo_h1a' && BASE_SHA='$h1a_base' HEAD_SHA='$h1a_head' '$SELF' check-pr"

  # H1 variant b: forged heading plus a brand-new rust/ file -- the shape an
  # ordinary, non-adversarial feature commit takes once one stray heading is
  # present anywhere in the same diff.
  local repo_h1b="$tmp/h1-forged-heading-new-file"
  build_h1_steady_state_base "$repo_h1b"
  local h1b_base; h1b_base="$(cd "$repo_h1b" && git rev-parse HEAD)"
  (
    cd "$repo_h1b"
    printf 'pub fn new_thing() {}\n' >rust/new_file.rs
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [9.9.9] - 2026-08-07\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
    git add -A
    git commit -q -m "H1 forgery: forged heading plus a brand-new rust/ file, no fragment"
  )
  local h1b_head; h1b_head="$(cd "$repo_h1b" && git rev-parse HEAD)"
  st_expect_fail "H1 rejecting: a forged empty version heading does not exempt a brand-new rust/ file either (changelog-fragment-missing)" \
    bash -c "cd '$repo_h1b' && BASE_SHA='$h1b_base' HEAD_SHA='$h1b_head' '$SELF' check-pr"

  # H1 variant c: not a forgery at all -- a genuinely valid, hand-verified
  # full release move (body moved verbatim into the new section, exactly the
  # shape classify_direct_edit's own validation requires) that also carries
  # an unrelated product-surface change with no fragment in the same commit,
  # the way docs/design/RELEASE.md steps 4-10 bundle a release into one commit.
  local repo_h1c="$tmp/h1-handforged-release-plus-unrelated"
  mkdir -p "$repo_h1c/rust" "$repo_h1c/changelog.d"
  (
    cd "$repo_h1c"
    git init -q
    git config user.email "selftest@example.invalid"
    git config user.name "selftest"
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n- Added (#1): a pending entry.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v1.0.0...HEAD\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
    printf 'fn main() {}\n' >rust/lib.rs
    git add -A
    git commit -q -m base
  )
  local h1c_base; h1c_base="$(cd "$repo_h1c" && git rev-parse HEAD)"
  (
    cd "$repo_h1c"
    printf 'fn main() { /* unrelated change riding along in a genuine release move */ }\n' >rust/lib.rs
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [2.0.0] - 2026-08-07\n\n- Added (#1): a pending entry.\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n\n[Unreleased]: https://example/compare/v2.0.0...HEAD\n[2.0.0]: https://example/compare/v1.0.0...v2.0.0\n[1.0.0]: https://example/compare/v0.9.0...v1.0.0\n' >CHANGELOG.md
    git add -A
    git commit -q -m "H1: genuine release move, plus an unrelated rust/lib.rs change, no fragment"
  )
  local h1c_head; h1c_head="$(cd "$repo_h1c" && git rev-parse HEAD)"
  st_expect_fail "H1 rejecting: a validated release move still demands a fragment for a product change outside the release-bump set (changelog-fragment-missing)" \
    bash -c "cd '$repo_h1c' && BASE_SHA='$h1c_base' HEAD_SHA='$h1c_head' '$SELF' check-pr"

  unset -f build_h1_steady_state_base

  # Controls E and F from the review's table, for completeness alongside
  # the four variants above: a product change that does not touch
  # CHANGELOG.md at all, and one that edits the [Unreleased] body directly,
  # both already correctly rejected before this fix and still rejected
  # after it (already covered by selftest_control1's basic fixtures and
  # selftest_control4's direct-edit fixtures respectively; not repeated
  # here as full end-to-end cases).

  # S3-2: a version-only release with zero fragments in changelog.d/ must
  # still merge. Both paths this finding named must agree: `release` itself
  # must not hard-fail with `no-fragments-to-aggregate`, and the resulting
  # diff must pass `check-pr` without a fragment, because a validated
  # release-move classification alone is now sufficient (no deleted
  # fragment required).
  local relrepo="$tmp/zero-fragment-release"
  mkdir -p "$relrepo/rust" "$relrepo/changelog.d"
  (
    cd "$relrepo"
    git init -q
    git config user.email "selftest@example.invalid"
    git config user.name "selftest"
    printf '[workspace.package]\nversion = "1.0.0"\n' >rust/Cargo.toml
    printf 'lockfile v1\n' >rust/Cargo.lock
    printf '# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- Old.\n' >CHANGELOG.md
    git add -A
    git commit -q -m base
  )
  local rel_base; rel_base="$(cd "$relrepo" && git rev-parse HEAD)"
  (
    cd "$relrepo"
    printf '[workspace.package]\nversion = "1.1.0"\n' >rust/Cargo.toml
    printf 'lockfile v1.1\n' >rust/Cargo.lock
    "$SELF" release --version 1.1.0 --date 2026-08-07
    git add -A
    git commit -q -m "chore(release): v1.1.0 (version-only, zero fragments)"
  )
  local rel_head; rel_head="$(cd "$relrepo" && git rev-parse HEAD)"
  st_expect_pass "S3-2 accepting: a version-only release with zero fragments merges" \
    bash -c "cd '$relrepo' && BASE_SHA='$rel_base' HEAD_SHA='$rel_head' '$SELF' check-pr"

  unset -f build_release_exclusion_base
  rm -rf "$tmp"
}

# check-section (issue #1763): each rejecting fixture breaks exactly one rule
# of an otherwise accepted section, so its exit is attributable to that rule.
selftest_section() {
  local tmp; tmp="$(mktemp -d)"
  local head='# Changelog'$'\n\n''## [Unreleased]'$'\n\n''## [5.1.0] - 2026-12-01'$'\n'
  local tail_old=$'\n''## [4.12.0] - 2026-10-17'$'\n\n''- Fixed (#5, breaking): a 4.x lead.'$'\n''- Fixed (#5): the same id again.'$'\n''* not a bullet the rules accept'$'\n'
  # $1 = file name, $2.. = section body lines (after the heading).
  st_write_section() {
    local name="$1"; shift
    { printf '%s\n' "$head"; printf '%s\n' "$@"; printf '%s' "$tail_old"; } >"$tmp/$name"
  }
  local -a good=(
    "- Added (#7): a capability."
    "- Changed (#8, breaking removal): a form is refused."
    "  Continuation line."
    ""
    "  - a nested list item inside the bullet."
    "- Changed (#9, #10, breaking): a CLI change."
    "- Changed (Part of #11): a staged change."
    "- Fixed (#8): the same id in another category."
    "- Fixed (#12, refs #8): a cross-reference is not an id of the entry."
    "- Fixed (#13, representative decision 2026-10-26): a dated label."
    "- Documented (#7): a write-up."
  )
  st_write_section good.md "" "${good[@]}" ""
  st_expect_pass "check-section accepting: well-formed 5.1.0 section; 4.12.0 below the floor is skipped" \
    check_section --changelog "$tmp/good.md"
  st_expect_pass "check-section accepting: --version 5.1.0" \
    check_section --changelog "$tmp/good.md" --version 5.1.0
  # The 4.12.0 tail breaks three rules, so the floor is what keeps it out.
  st_expect_fail "check-section rejecting: an explicit --version below the floor is checked" \
    check_section --changelog "$tmp/good.md" --version 4.12.0
  # A breaking label is an item that starts with `breaking`: a negated one is not.
  st_write_section nonbrk.md "" "${good[@]:0:10}" "- Fixed (#18, non-breaking): a negated label under Fixed." "${good[@]:10}" ""
  st_expect_pass "check-section accepting: \`non-breaking\` is not a breaking label" \
    check_section --changelog "$tmp/nonbrk.md"
  st_write_section notbrk.md "" "${good[@]:0:10}" "- Fixed (#19, not breaking): a negated label under Fixed." "${good[@]:10}" ""
  st_expect_pass "check-section accepting: \`not breaking\` is not a breaking label" \
    check_section --changelog "$tmp/notbrk.md"
  # Every id of a `refs` list is a reference: #12 here would otherwise repeat Fixed #12.
  st_write_section refslist.md "" "${good[@]:0:10}" "- Fixed (#20, refs #8, #12): a refs list of two ids." "${good[@]:10}" ""
  st_expect_pass "check-section accepting: the second id of \`refs #A, #B\` is a reference" \
    check_section --changelog "$tmp/refslist.md"
  st_write_section refslistonly.md "" "${good[@]:0:10}" "- Fixed (refs #8, #21): only a refs list." "${good[@]:10}" ""
  st_expect_fail "check-section rejecting: a lead whose ids are all in a refs list" \
    check_section --changelog "$tmp/refslistonly.md"
  st_write_section refsbrk.md "" "${good[@]:0:10}" "- Fixed (#22, refs #8, breaking): a label after a refs list." "${good[@]:10}" ""
  st_expect_fail "check-section rejecting: a breaking label after a refs list still counts" \
    check_section --changelog "$tmp/refsbrk.md"

  st_write_section dup.md "" "${good[@]}" "- Documented (#7): a second bullet for the same (id, category)." ""
  st_expect_fail "check-section rejecting: duplicate (id, category)" check_section --changelog "$tmp/dup.md"
  st_write_section dupmulti.md "" "${good[@]:0:6}" "- Changed (#10): #10 is already in a two-id lead." "${good[@]:6}" ""
  st_expect_fail "check-section rejecting: duplicate id inside a multi-id lead" check_section --changelog "$tmp/dupmulti.md"
  st_write_section dupzero.md "" "${good[@]}" "- Documented (#0007): zero-padded #7." ""
  st_expect_fail "check-section rejecting: zero-padded alias (#0007 == #7)" check_section --changelog "$tmp/dupzero.md"
  st_write_section brkfixed.md "" "${good[@]:0:7}" "- Fixed (#14, breaking removal): refused under Fixed." "${good[@]:7}" ""
  st_expect_fail "check-section rejecting: breaking removal under Fixed" check_section --changelog "$tmp/brkfixed.md"
  st_write_section brkadded.md "" "- Added (#14, breaking): breaking under Added." "${good[@]}" ""
  st_expect_fail "check-section rejecting: breaking under Added (only Changed carries it)" check_section --changelog "$tmp/brkadded.md"
  st_write_section undeclared.md "" "${good[@]:0:7}" "- Removed (#15): not a declared category." "${good[@]:7}" ""
  st_expect_fail "check-section rejecting: undeclared category lead" check_section --changelog "$tmp/undeclared.md"
  st_write_section noid.md "" "${good[@]:0:7}" "- Fixed (no id): a lead without an id." "${good[@]:7}" ""
  st_expect_fail "check-section rejecting: lead without an id" check_section --changelog "$tmp/noid.md"
  st_write_section refsonly.md "" "${good[@]:0:7}" "- Fixed (refs #8): only a cross-reference." "${good[@]:7}" ""
  st_expect_fail "check-section rejecting: lead with only a refs id" check_section --changelog "$tmp/refsonly.md"
  st_write_section nolead.md "" "${good[@]:0:7}" "- Fixed #16: no parenthesised label." "${good[@]:7}" ""
  st_expect_fail "check-section rejecting: lead without a parenthesised label" check_section --changelog "$tmp/nolead.md"
  st_write_section order.md "" "- Fixed (#17): Fixed before Added." "${good[@]}" ""
  st_expect_fail "check-section rejecting: categories out of declared order" check_section --changelog "$tmp/order.md"
  st_write_section stray.md "" "${good[@]:0:7}" "A stray paragraph." "${good[@]:7}" ""
  st_expect_fail "check-section rejecting: a line that is not a bullet or a continuation" check_section --changelog "$tmp/stray.md"
  st_expect_fail "check-section rejecting: --version names no section" \
    check_section --changelog "$tmp/good.md" --version 5.2.0
  printf '%s\n' '# Changelog' '' '## [Unreleased]' "$tail_old" >"$tmp/old-only.md"
  st_expect_fail "check-section rejecting: no released section at or above the floor" \
    check_section --changelog "$tmp/old-only.md"

  # Release-notes cross-check (issue #1806). The changelogs live in rn/, so
  # rn/docs/release-notes/v5.1.0.md is their default summary and the fixtures
  # above keep having none. The breaking ids of `good` are #8, #9 and #10.
  mkdir -p "$tmp/rn/docs/release-notes" "$tmp/other"
  local notes="$tmp/rn/docs/release-notes/v5.1.0.md"
  st_write_section rn/good.md "" "${good[@]}" ""
  printf '%s\n' '# 5.1.0' '' '## Breaking changes' '' '- A form is refused (#8).' '' \
    '### CLI' '' '- A CLI change (#9, PR #10).' '' '## Other fixes' '' '- #12.' >"$notes"
  st_expect_pass "check-section accepting: every breaking id is under the summary's ## Breaking changes, ### subsections included" \
    check_section --changelog "$tmp/rn/good.md"
  st_write_section rn/refs.md "" "${good[@]:0:7}" "- Changed (#30, refs #31, breaking): a breaking entry with a reference." "${good[@]:7}" ""
  printf '%s\n' '## Breaking changes' '' '- #8, #9, #10 and #30.' >"$notes"
  st_expect_pass "check-section accepting: a breaking entry's refs id (#31) need not be listed" \
    check_section --changelog "$tmp/rn/refs.md"
  printf '%s\n' '## Headline' '' '- #10 leads the release.' '' '## Breaking changes' '' '- #8 and #9.' >"$notes"
  st_expect_fail "check-section rejecting: a breaking id (#10) referenced only outside ## Breaking changes" \
    check_section --changelog "$tmp/rn/good.md"
  st_expect_fail "check-section rejecting: --version runs the cross-check too" \
    check_section --changelog "$tmp/rn/good.md" --version 5.1.0
  printf '%s\n' '## Breaking changes' '' '- #8, #9 and #100.' >"$notes"
  st_expect_fail "check-section rejecting: #100 is not a reference to #10" \
    check_section --changelog "$tmp/rn/good.md"
  printf '%s\n' '# 5.1.0' '' 'Breaking changes: #8, #9, #10.' >"$notes"
  st_expect_fail "check-section rejecting: a summary with no ## Breaking changes heading" \
    check_section --changelog "$tmp/rn/good.md"
  printf '%s\n' '## Breaking changes' '' '- #8, #9, #10.' >"$tmp/other/v5.1.0.md"
  st_expect_pass "check-section accepting: --summary-dir names the summary directory" \
    check_section --changelog "$tmp/rn/good.md" --summary-dir "$tmp/other"
  rm -f "$notes"
  st_expect_pass "check-section accepting: without a summary the release body is the section, nothing to cross-check" \
    check_section --changelog "$tmp/rn/good.md"

  unset -f st_write_section
  rm -rf "$tmp"
}

# fold (issue #1806): the accepting fixture places a fragment at the start of
# a category, inside a category kept out of id order, after a bullet that only
# refs its id, into a category the section does not have, and at the end of
# the section, and compares the whole file with the expected one.
selftest_fold() {
  local tmp; tmp="$(mktemp -d)"
  local -a head=('# Changelog' '' 'Intro.' '' '## [Unreleased]' '' '## [5.1.0] - 2026-12-01' '')
  local -a tail=('' '## [5.0.0] - 2026-11-07' '' '- Added (#1): older.' '' '[Unreleased]: https://example.invalid/compare/v5.1.0...HEAD')
  local -a section=(
    "- Added (#7): a capability."
    "- Changed (#20): a change."
    "- Changed (#5): an entry kept out of id order."
    "- Changed (#30, breaking): a CLI change."
    "  Continuation line."
    ""
    "  A second paragraph."
    "- Fixed (#8): a fix."
    "- Fixed (#12, refs #40): a cross-reference."
    "- Documented (#7): a write-up."
  )
  # $1 = fixture dir; the rest = section lines.
  st_fold_fixture() {
    local d="$1"; shift
    rm -rf "$d"; mkdir -p "$d/changelog.d"
    printf 'Fragment contract.\n' >"$d/changelog.d/README.md"
    printf '%s\n' "${head[@]}" "$@" "${tail[@]}" >"$d/CHANGELOG.md"
  }
  st_fold_main_fragments() {
    local d="$1"
    st_setup_frag "$d/changelog.d" "3-a.added.md" "Added (#3): before #7."
    st_setup_frag "$d/changelog.d" "25-b.changed.md" "Changed (#25): after #5, before #30."
    st_setup_frag "$d/changelog.d" "40-c.fixed.md" "Fixed (#40): a multi-line fragment" "whose second line is indented."
    st_setup_frag "$d/changelog.d" "2-d.required.md" "Required (#2): a category the section did not have."
    st_setup_frag "$d/changelog.d" "9-e.decided.md" "Decided (#9): the last category, at the end of the section."
  }
  # shellcheck disable=SC2329  # invoked through st_expect_pass/st_expect_fail
  st_fold() {  # $1 = fixture dir, $2.. = extra fold arguments
    local d="$1"; shift
    "$SELF" fold --changelog "$d/CHANGELOG.md" --fragments-dir "$d/changelog.d" "$@"
  }
  # Succeeds when fold fails with diagnostic $1 ($2 = fixture dir, $3.. = fold arguments).
  # shellcheck disable=SC2329  # invoked through st_expect_pass
  st_fold_reports() {
    local code="$1" out; shift
    if out="$(st_fold "$@" 2>&1)"; then return 1; fi
    printf '%s\n' "$out" | grep -q "^$code"
  }
  # $1 = label, $2 = expected diagnostic, $3 = fixture dir, $4.. = fold
  # arguments. A refusal names its own diagnostic, since another refusal can
  # catch the same input later, and leaves the changelog and every fragment as
  # they were.
  st_fold_refuses() {
    local label="$1" code="$2" d="$3"; shift 3
    rm -rf "$d.snapshot"; cp -R "$d" "$d.snapshot"
    st_expect_fail "fold rejecting: $label" st_fold "$d" "$@"
    st_expect_pass "fold rejecting: $label -- reported as $code" st_fold_reports "$code" "$d" "$@"
    st_expect_pass "fold rejecting: $label -- CHANGELOG.md and changelog.d/ untouched" diff -r "$d.snapshot" "$d"
  }

  local d="$tmp/main"
  st_fold_fixture "$d" "${section[@]}"
  st_fold_main_fragments "$d"
  printf '%s\n' "${head[@]}" \
    "- Added (#3): before #7." \
    "- Added (#7): a capability." \
    "- Changed (#20): a change." \
    "- Changed (#5): an entry kept out of id order." \
    "- Changed (#25): after #5, before #30." \
    "- Changed (#30, breaking): a CLI change." \
    "  Continuation line." \
    "" \
    "  A second paragraph." \
    "- Fixed (#8): a fix." \
    "- Fixed (#12, refs #40): a cross-reference." \
    "- Fixed (#40): a multi-line fragment" \
    "  whose second line is indented." \
    "- Required (#2): a category the section did not have." \
    "- Documented (#7): a write-up." \
    "- Decided (#9): the last category, at the end of the section." \
    "${tail[@]}" >"$tmp/main.expected"
  st_expect_pass "fold accepting: five fragments folded into 5.1.0" st_fold "$d" --version 5.1.0
  st_expect_pass "fold accepting: the result is byte-identical to the expected changelog (date unchanged)" \
    cmp "$tmp/main.expected" "$d/CHANGELOG.md"
  local listing
  listing="$(ls -A "$d/changelog.d")"
  st_expect_pass "fold accepting: changelog.d/ keeps only README.md" test "$listing" = "README.md"
  st_expect_pass "fold accepting: a second run with no fragments exits 0" st_fold "$d" --version 5.1.0
  st_expect_pass "fold accepting: ... and leaves the changelog unchanged" cmp "$tmp/main.expected" "$d/CHANGELOG.md"

  # A version-only release wrote a section with no bullets.
  d="$tmp/empty"
  rm -rf "$d"; mkdir -p "$d/changelog.d"
  printf '%s\n' '# Changelog' '' '## [Unreleased]' '' '## [5.1.0] - 2026-12-01' '' '' '' '## [5.0.0] - 2026-11-07' '' '- Added (#1): older.' >"$d/CHANGELOG.md"
  st_setup_frag "$d/changelog.d" "4-x.fixed.md" "Fixed (#4): the only entry."
  printf '%s\n' '# Changelog' '' '## [Unreleased]' '' '## [5.1.0] - 2026-12-01' '' '- Fixed (#4): the only entry.' '' '## [5.0.0] - 2026-11-07' '' '- Added (#1): older.' >"$tmp/empty.expected"
  st_expect_pass "fold accepting: into a section with no bullets" st_fold "$d" --version 5.1.0
  st_expect_pass "fold accepting: ... laid out as release lays out a section" cmp "$tmp/empty.expected" "$d/CHANGELOG.md"

  d="$tmp/refuse"
  st_fold_fixture "$d" "${section[@]}"
  st_fold_main_fragments "$d"
  st_fold_refuses "--version names no section" fold-section-missing "$d" --version 5.2.0
  st_fold_refuses "--version is not the newest section" fold-section-not-latest "$d" --version 5.0.0
  st_fold_refuses "--version is not SemVer" "fold: --version must be SemVer" "$d" --version v5.1.0

  d="$tmp/unreleased"
  st_fold_fixture "$d" "${section[@]}"
  st_fold_main_fragments "$d"
  sed -i 's/^## \[Unreleased\]$/&\n\n- Fixed (#99): a body release would have moved./' "$d/CHANGELOG.md"
  st_fold_refuses "[Unreleased] has content" fold-unreleased-not-empty "$d" --version 5.1.0

  d="$tmp/dup"
  st_fold_fixture "$d" "${section[@]}"
  st_setup_frag "$d/changelog.d" "30-x.changed.md" "Changed (#30): #30 is already a Changed entry."
  st_fold_refuses "(id, category) already in the section" fold-duplicate-entry "$d" --version 5.1.0
  st_fold_fixture "$d" "${section[@]}"
  st_setup_frag "$d/changelog.d" "0008-y.fixed.md" "Fixed (#8): zero-padded #8."
  st_fold_refuses "zero-padded alias of a section entry (#0008 == #8)" fold-duplicate-entry "$d" --version 5.1.0
  st_fold_fixture "$d" "${section[@]}"
  st_setup_frag "$d/changelog.d" "8-z.changed.md" "Changed (#8): Fixed #8 is another category."
  st_expect_pass "fold accepting: the same id in another category is not a duplicate" st_fold "$d" --version 5.1.0

  d="$tmp/badfrag"
  st_fold_fixture "$d" "${section[@]}"
  st_setup_frag "$d/changelog.d" "6-w.fixed.md" "Fixed #6 has no parenthesised label."
  st_fold_refuses "a fragment whose bullet breaks the section rules" fold-conservation-failed "$d" --version 5.1.0
  st_fold_fixture "$d" "${section[@]}"
  printf '\n\n' >"$d/changelog.d/6-w.fixed.md"
  st_fold_refuses "an empty fragment" changelog-fragment-empty "$d" --version 5.1.0
  st_fold_fixture "$d" "${section[@]}"
  st_setup_frag "$d/changelog.d" "6-w.removed.md" "Removed (#6): undeclared."
  st_fold_refuses "an undeclared category" changelog-fragment-name-invalid "$d" --version 5.1.0

  d="$tmp/invalid"
  st_fold_fixture "$d" "- Fixed (#17): Fixed before Added." "${section[@]}"
  st_setup_frag "$d/changelog.d" "3-a.added.md" "Added (#3): would go first."
  st_fold_refuses "the section already breaks the rules" fold-section-invalid "$d" --version 5.1.0

  d="$tmp/tagged"
  st_fold_fixture "$d" "${section[@]}"
  st_fold_main_fragments "$d"
  ( cd "$d" && git init -q && git config user.email "selftest@example.invalid" && git config user.name "selftest" \
      && git add -A && git commit -q -m base && git tag v5.1.0 )
  st_fold_refuses "the version is already tagged" fold-version-tagged "$d" --version 5.1.0

  # verify_fold against shams of the accepted result.
  d="$tmp/verify"
  st_fold_fixture "$d" "${section[@]}"
  st_fold_main_fragments "$d"
  local before="$d/CHANGELOG.md"
  st_expect_pass "fold verify accepting: the expected result" verify_fold "$before" "$tmp/main.expected" 5.1.0 "$d/changelog.d"
  grep -v '^- Required (#2)' "$tmp/main.expected" >"$tmp/sham-drop"
  st_expect_fail "fold verify rejecting: a dropped fragment" verify_fold "$before" "$tmp/sham-drop" 5.1.0 "$d/changelog.d"
  sed 's/^- Fixed (#8): a fix\.$/- Fixed (#8): a reworded fix./' "$tmp/main.expected" >"$tmp/sham-edit"
  st_expect_fail "fold verify rejecting: an existing bullet edited" verify_fold "$before" "$tmp/sham-edit" 5.1.0 "$d/changelog.d"
  sed 's/^## \[5\.1\.0\] - 2026-12-01$/## [5.1.0] - 2026-12-02/' "$tmp/main.expected" >"$tmp/sham-date"
  st_expect_fail "fold verify rejecting: the heading date moved" verify_fold "$before" "$tmp/sham-date" 5.1.0 "$d/changelog.d"
  sed 's/^- Added (#1): older\.$/- Added (#1): older, edited./' "$tmp/main.expected" >"$tmp/sham-outside"
  st_expect_fail "fold verify rejecting: another section edited" verify_fold "$before" "$tmp/sham-outside" 5.1.0 "$d/changelog.d"
  sed 's/^- Changed (#20): a change\.$/- Changed (#20): a change.\n- Changed (#20): a change./' "$tmp/main.expected" >"$tmp/sham-dup"
  sed -i '/^- Changed (#5): /d' "$tmp/sham-dup"
  st_expect_fail "fold verify rejecting: an existing bullet duplicated in place of another" verify_fold "$before" "$tmp/sham-dup" 5.1.0 "$d/changelog.d"
  awk '/^- Added \(#3\)/ { held = $0; next } { print } /^- Added \(#7\)/ { print held }' "$tmp/main.expected" >"$tmp/sham-order"
  st_expect_fail "fold verify rejecting: an existing bullet and a folded one swapped" verify_fold "$before" "$tmp/sham-order" 5.1.0 "$d/changelog.d"
  sed 's/^## \[5\.1\.0\] - 2026-12-01$/&\n/' "$tmp/main.expected" >"$tmp/sham-blank"
  st_expect_fail "fold verify rejecting: a blank line added around the bullets" verify_fold "$before" "$tmp/sham-blank" 5.1.0 "$d/changelog.d"

  unset -f st_fold_fixture st_fold_main_fragments st_fold st_fold_reports st_fold_refuses
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
  # S2-1 (release exclusion over-widening) and S3-2 (zero-fragment release):
  # end-to-end fixtures driven through the real check-pr/release subcommands,
  # not just the pure classify_product_diff predicate (review, #737, comment
  # 2026-08-07, second round).
  selftest_release_exclusion
  selftest_control5
  # selftest_control6 was defined but never invoked here from this
  # subcommand's introduction onward -- found auditing this orchestrator
  # while adding S2-3's nested-fragment fixture to it, since that fixture
  # would otherwise silently never run either. Control 6 itself (`parse_fragment_name`)
  # was still exercised indirectly by every other control's fixtures that
  # set up fragment names, but its own dedicated accepting/rejecting
  # fixtures were dead code from a `selftest` caller's perspective.
  selftest_control6
  selftest_section
  selftest_fold
  echo
  if [ "$ST_FAILURES" -eq 0 ]; then
    echo "aggregate_changelog: selftest PASS -- all six controls', check-section's (with the release-notes cross-check) and fold's accepting and rejecting fixtures behaved as expected"
  else
    echo "aggregate_changelog: selftest FAIL -- $ST_FAILURES assertion(s) failed" >&2
    exit 1
  fi
}

case "${1:-}" in
  check) shift; check "$@" ;;
  check-pr) shift; check_pr "$@" ;;
  check-stale) shift; check_stale "$@" ;;
  check-section) shift; check_section "$@" ;;
  release) shift; release "$@" ;;
  fold) shift; fold "$@" ;;
  selftest) shift; selftest "$@" ;;
  *)
    echo "usage: $0 {check|check-pr|check-stale|check-section|release|fold|selftest} [args]" >&2
    echo "       check-pr reads BASE_SHA and HEAD_SHA from the environment" >&2
    exit 2
    ;;
esac
