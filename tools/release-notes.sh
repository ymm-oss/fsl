#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "release-notes.sh requires Bash 4 or newer" >&2; exit 1; }
# SPDX-License-Identifier: Apache-2.0
#
# Builds and checks the GitHub Release body for one version
# (docs/design/RELEASE.md, section 1, step 8). `.github/workflows/release.yml`
# runs `build` in the `assemble` job, which a `workflow_dispatch` dry run also
# reaches, and again in the tag-only `publish` job that feeds the file to
# `body_path`. A body that is empty or too long therefore fails the dry run,
# before a tag that cannot be reused exists.
#
# Usage:
#   release-notes.sh build --version X.Y.Z --repository-url URL --out FILE
#                          [--changelog FILE] [--summary-dir DIR]
#     Writes the release body to FILE and exits 0, or writes nothing and
#     exits 1 with one diagnostic:
#       - release-notes-version-invalid: X.Y.Z is not a SemVer version
#       - release-notes-section-missing: CHANGELOG.md has no `## [X.Y.Z]`
#         heading (or has more than one)
#       - release-notes-empty: the body has no non-whitespace character
#       - release-notes-too-long: the body is over RELEASE_BODY_LIMIT bytes
#     When DIR/vX.Y.Z.md exists (DIR defaults to docs/release-notes), the body
#     is that hand-written summary followed by an absolute link to the
#     `## [X.Y.Z]` section of the tagged CHANGELOG.md. Otherwise the body is
#     that section, without its heading, exactly as before this script.
#
#   release-notes.sh workspace-version [--cargo-toml FILE]
#     Prints `[workspace.package].version` from rust/Cargo.toml. A dispatch
#     run has no tag to read the version from, so it checks this one.
#
#   release-notes.sh selftest
#     Accepting and rejecting controls for `build` and `workspace-version`,
#     including both sides of the size boundary.
#
# The limit is counted in bytes. GitHub rejects a longer body with "body is
# too long (maximum is 125000 characters)"; that text is observed in
# third-party API error reports, not in GitHub's documentation. A UTF-8 byte
# count is never smaller than the code-point or UTF-16 length of the same
# text, so a body within the limit in bytes is within it under any of those
# counts.

set -euo pipefail

RELEASE_BODY_LIMIT=125000
DEFAULT_CHANGELOG=CHANGELOG.md
DEFAULT_SUMMARY_DIR=docs/release-notes
DEFAULT_CARGO_TOML=rust/Cargo.toml

die() {
  echo "$*" >&2
  exit 1
}

# The heading line `## [X.Y.Z] ...`, or nothing. The prefix match is the one
# the workflow's earlier inline extractor used, so the same section is found.
section_headings() {
  local version="$1" changelog="$2"
  awk -v heading="## [$version]" 'index($0, heading) == 1 { print }' "$changelog"
}

section_body() {
  local version="$1" changelog="$2"
  awk -v heading="## [$version]" '
    index($0, heading) == 1 { found = 1; next }
    found && /^## \[/ { exit }
    found { print }
  ' "$changelog"
}

# GitHub's anchor for an ATX heading: lowercase, drop every character that is
# not a letter, digit, space, hyphen or underscore, then turn each space into
# a hyphen. `## [4.12.0] - 2026-10-17` renders as `#4120---2026-10-17`
# (observed in GitHub's rendering of CHANGELOG.md at v4.12.0).
heading_anchor() {
  local text="${1#\#\# }"
  printf '%s' "$text" \
    | LC_ALL=C tr '[:upper:]' '[:lower:]' \
    | LC_ALL=C tr -cd 'a-z0-9 _-' \
    | tr ' ' '-'
}

build() {
  local version="" repository_url="" out="" changelog="$DEFAULT_CHANGELOG"
  local summary_dir="$DEFAULT_SUMMARY_DIR"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version) version="${2-}"; shift 2 ;;
      --repository-url) repository_url="${2-}"; shift 2 ;;
      --out) out="${2-}"; shift 2 ;;
      --changelog) changelog="${2-}"; shift 2 ;;
      --summary-dir) summary_dir="${2-}"; shift 2 ;;
      *) die "release-notes.sh build: unknown argument: $1" ;;
    esac
  done
  [ -n "$out" ] || die "release-notes.sh build: --out is required"
  [ -n "$repository_url" ] || die "release-notes.sh build: --repository-url is required"
  case "$repository_url" in
    https://*) ;;
    *) die "release-notes.sh build: --repository-url must be an absolute https URL: $repository_url" ;;
  esac
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] \
    || die "release-notes-version-invalid: '$version' is not a SemVer version"
  [ -f "$changelog" ] || die "release-notes-section-missing: $changelog does not exist"

  local headings count
  headings="$(section_headings "$version" "$changelog")"
  count=0
  [ -z "$headings" ] || count="$(printf '%s\n' "$headings" | wc -l | tr -d ' ')"
  [ "$count" = 1 ] \
    || die "release-notes-section-missing: $changelog has $count '## [$version]' headings, expected 1"

  local summary="$summary_dir/v$version.md" source tmp
  tmp="$(mktemp)"
  if [ -f "$summary" ]; then
    source="summary $summary"
    local anchor link
    anchor="$(heading_anchor "$headings")"
    link="${repository_url%/}/blob/v$version/CHANGELOG.md#$anchor"
    # Command substitution drops the summary's trailing newlines, so exactly
    # one blank line separates it from the link whatever the file ends with.
    local text
    text="$(cat "$summary")"
    printf '%s\n\nThe complete list of changes in %s is in [CHANGELOG.md](%s).\n' \
      "$text" "$version" "$link" > "$tmp"
    if ! grep -q '[^[:space:]]' "$summary"; then
      rm -f "$tmp"
      die "release-notes-empty: $summary has no non-whitespace content"
    fi
  else
    source="section ## [$version] of $changelog (no $summary)"
    section_body "$version" "$changelog" > "$tmp"
  fi

  if ! grep -q '[^[:space:]]' "$tmp"; then
    rm -f "$tmp"
    die "release-notes-empty: the body built from $source has no non-whitespace content; add $summary stating the release's reason"
  fi
  local bytes
  bytes="$(wc -c < "$tmp" | tr -d ' ')"
  if [ "$bytes" -gt "$RELEASE_BODY_LIMIT" ]; then
    rm -f "$tmp"
    die "release-notes-too-long: the body built from $source is $bytes bytes, over GitHub's $RELEASE_BODY_LIMIT limit; add or shorten $summary"
  fi
  mv "$tmp" "$out"
  echo "release-notes: $out from $source, $bytes bytes (limit $RELEASE_BODY_LIMIT)" >&2
}

workspace_version() {
  local cargo_toml="$DEFAULT_CARGO_TOML"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --cargo-toml) cargo_toml="${2-}"; shift 2 ;;
      *) die "release-notes.sh workspace-version: unknown argument: $1" ;;
    esac
  done
  local version
  version="$(awk '
    /^\[/ { in_section = ($0 == "[workspace.package]"); next }
    in_section && /^version[[:space:]]*=/ {
      sub(/^version[[:space:]]*=[[:space:]]*"/, "")
      sub(/".*$/, "")
      print
      exit
    }
  ' "$cargo_toml")"
  [ -n "$version" ] || die "release-notes.sh: no [workspace.package] version in $cargo_toml"
  printf '%s\n' "$version"
}

selftest() {
  local dir
  dir="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $dir now; it is local to this function.
  trap "rm -rf '$dir'" EXIT
  local failures=0 url=https://github.com/example/repo got

  # Prints why a control failed, with the file that shows it.
  fail() {
    echo "selftest FAIL ($1): $2" >&2
    [ -z "${3-}" ] || cat "$3" >&2
    failures=$((failures + 1))
  }

  expect_ok() {
    local name="$1"; shift
    if ! "$0" build --repository-url "$url" --out "$dir/out.md" "$@" 2> "$dir/err"; then
      fail "$name" "expected success, got:" "$dir/err"
      return 1
    fi
  }
  expect_reject() {
    local name="$1" diagnostic="$2"; shift 2
    rm -f "$dir/out.md"
    if "$0" build --repository-url "$url" --out "$dir/out.md" "$@" 2> "$dir/err"; then
      fail "$name" "expected $diagnostic, but build succeeded"
    elif ! grep -q "^$diagnostic:" "$dir/err"; then
      fail "$name" "expected $diagnostic, got:" "$dir/err"
    elif [ -e "$dir/out.md" ]; then
      fail "$name" "a rejected build left $dir/out.md behind"
    fi
  }
  changelog() {
    # $1: file, $2: body of the 9.9.9 section (printf %s, no added newline).
    { printf '# Changelog\n\n## [Unreleased]\n\n## [9.9.9] - 2026-01-02\n'
      printf '%s' "$2"
      printf '## [9.9.8] - 2026-01-01\n\n- Fixed (#1): older entry.\n'; } > "$1"
  }
  # GitHub's limit, not a tunable: the boundary fixtures below derive from it.
  [ "$RELEASE_BODY_LIMIT" = 125000 ] || fail "limit" "RELEASE_BODY_LIMIT is $RELEASE_BODY_LIMIT"
  local filler
  filler="$(head -c "$((RELEASE_BODY_LIMIT - 1))" /dev/zero | tr '\0' x)"
  mkdir -p "$dir/none" "$dir/notes"

  # Accepting: no summary, the body is the section without its heading.
  changelog "$dir/small.md" $'\n- Added (#2): a feature.\n\n'
  if expect_ok "section body" --version 9.9.9 --changelog "$dir/small.md" --summary-dir "$dir/none"; then
    got="$(cat "$dir/out.md")"
    [ "$got" = $'\n- Added (#2): a feature.' ] \
      || fail "section body" "unexpected body:" "$dir/out.md"
  fi

  # Boundary: exactly RELEASE_BODY_LIMIT bytes passes, one more fails.
  changelog "$dir/at-limit.md" "$filler"$'\n'
  if expect_ok "at limit" --version 9.9.9 --changelog "$dir/at-limit.md" --summary-dir "$dir/none"; then
    got="$(wc -c < "$dir/out.md")"
    [ "${got// /}" = "$RELEASE_BODY_LIMIT" ] \
      || fail "at limit" "body is $got bytes, not $RELEASE_BODY_LIMIT"
  fi
  changelog "$dir/over-limit.md" "${filler}x"$'\n'
  expect_reject "over limit" release-notes-too-long \
    --version 9.9.9 --changelog "$dir/over-limit.md" --summary-dir "$dir/none"

  # A summary replaces an oversized section, and links to it.
  printf '# 9.9.9\n\nHighlights.\n\n\n' > "$dir/notes/v9.9.9.md"
  if expect_ok "summary" --version 9.9.9 --changelog "$dir/over-limit.md" --summary-dir "$dir/notes"; then
    local expected
    expected=$'# 9.9.9\n\nHighlights.\n\nThe complete list of changes in 9.9.9 is in [CHANGELOG.md](https://github.com/example/repo/blob/v9.9.9/CHANGELOG.md#999---2026-01-02).'
    got="$(cat "$dir/out.md")"
    [ "$got" = "$expected" ] || fail "summary" "unexpected body:" "$dir/out.md"
  fi
  # The summary is checked against the same limit.
  { printf '%s' "$filler"; printf 'x\n'; } > "$dir/notes/v9.9.9.md"
  expect_reject "summary over limit" release-notes-too-long \
    --version 9.9.9 --changelog "$dir/small.md" --summary-dir "$dir/notes"
  printf ' \n\t\n' > "$dir/notes/v9.9.9.md"
  expect_reject "blank summary" release-notes-empty \
    --version 9.9.9 --changelog "$dir/small.md" --summary-dir "$dir/notes"

  # An empty section is rejected without a summary and accepted with one.
  changelog "$dir/empty.md" $'\n\n'
  expect_reject "empty section" release-notes-empty \
    --version 9.9.9 --changelog "$dir/empty.md" --summary-dir "$dir/none"
  printf 'Version-only release: the Z3 dependency moved.\n' > "$dir/notes/v9.9.9.md"
  expect_ok "empty section with summary" \
    --version 9.9.9 --changelog "$dir/empty.md" --summary-dir "$dir/notes" || true

  # The linked section must exist, with or without a summary.
  expect_reject "missing section" release-notes-section-missing \
    --version 9.9.7 --changelog "$dir/small.md" --summary-dir "$dir/none"
  printf 'Summary.\n' > "$dir/notes/v9.9.7.md"
  expect_reject "missing section with summary" release-notes-section-missing \
    --version 9.9.7 --changelog "$dir/small.md" --summary-dir "$dir/notes"
  # Two headings for one version leave the linked section ambiguous.
  { cat "$dir/small.md"; printf '## [9.9.9] - 2026-01-03\n\n- Added (#3): again.\n'; } > "$dir/twice.md"
  expect_reject "duplicate section" release-notes-section-missing \
    --version 9.9.9 --changelog "$dir/twice.md" --summary-dir "$dir/none"
  expect_reject "invalid version" release-notes-version-invalid \
    --version 'v9.9.9' --changelog "$dir/small.md" --summary-dir "$dir/none"

  # A prerelease heading slugs the way GitHub renders it.
  got="$(heading_anchor '## [5.0.0-rc.1] - 2026-11-07')"
  [ "$got" = "500-rc1---2026-11-07" ] || fail "anchor" "got $got"

  printf '[package]\nversion = "0.0.1"\n\n[workspace.package]\nedition = "2021"\nversion = "9.9.9"\n' \
    > "$dir/Cargo.toml"
  got="$("$0" workspace-version --cargo-toml "$dir/Cargo.toml")" || got="(failed)"
  [ "$got" = 9.9.9 ] || fail "workspace-version" "got $got"
  printf '[package]\nversion = "0.0.1"\n' > "$dir/NoWorkspace.toml"
  if "$0" workspace-version --cargo-toml "$dir/NoWorkspace.toml" > /dev/null 2>&1; then
    fail "workspace-version" "accepted a manifest without [workspace.package]"
  fi

  if [ "$failures" -ne 0 ]; then
    echo "release-notes.sh selftest: $failures failure(s)" >&2
    exit 1
  fi
  echo "release-notes.sh selftest: ok"
}

case "${1-}" in
  build) shift; build "$@" ;;
  workspace-version) shift; workspace_version "$@" ;;
  selftest) shift; selftest "$@" ;;
  *) die "usage: $0 build --version X.Y.Z --repository-url URL --out FILE [--changelog FILE] [--summary-dir DIR] | workspace-version [--cargo-toml FILE] | selftest" ;;
esac
