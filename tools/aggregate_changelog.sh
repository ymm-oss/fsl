#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "aggregate_changelog.sh requires Bash 4 or newer" >&2; exit 1; }
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
# contract change to docs/DESIGN-changelog-fragments.md, the same way growing
# tools/check-product-gate-scope.sh's exempt-path list is (see
# docs/DESIGN-ci.md, "Agent-configuration exemption"): name the new word,
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
# commit (`docs/RELEASE.md` bundles steps 4-10 into one), and a branch whose
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
# additionally touched `docs/RELEASE.md`, and every release touches
# `editors/vscode/package.json` and `editors/vscode/package-lock.json`; none
# of those three is a product surface under is_product_surface_path, so none
# of them ever reaches this predicate. Listing them here anyway would be
# unreachable code that reads like coverage: an earlier version of this set
# did list the two `editors/vscode/` paths, and deleting both left `selftest`
# fully green because nothing could ever call them (review finding F3, #737,
# comment 2026-08-08, fourth round).
#
# A maintainer must add an exact path here whenever either side of that
# filter moves: a docs/RELEASE.md release-commit step that starts touching a
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
# Release exclusion (docs/DESIGN-changelog-fragments.md, control 1;
# corrected, review finding S2-1, #737, comment 2026-08-07, second round):
# the release commit itself (docs/RELEASE.md steps 4-7, committed together
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
# real release commit that also carries an unrelated product change (docs/
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
# docs/DESIGN-changelog-fragments.md, control 4, and the migration
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
  # no longer requires a deleted fragment either -- see docs/RELEASE.md,
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
  # *deletes* fragments (docs/RELEASE.md steps 4-7, one commit per step 10);
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
  # the real `release` subcommand, exactly as docs/RELEASE.md steps 4-10
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
  # the way docs/RELEASE.md steps 4-10 bundle a release into one commit.
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
