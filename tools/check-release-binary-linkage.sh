#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

check_binary() {
  local binary="$1" ldd_output linked

  ldd_output="$(ldd "$binary")"
  if linked="$(grep libz3 <<<"$ldd_output")"; then
    printf 'release linkage check failed: %s dynamically links libz3:\n%s\n' "$binary" "$linked" >&2
    return 1
  fi
}

check_glibc() {
  local binary="$1" required

  required="$(readelf --version-info "$binary" | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1)"
  if ! test -n "$required"; then
    printf 'release ABI check failed: %s has no GLIBC requirement\n' "$binary" >&2
    return 1
  fi
  local maximum
  maximum="$(printf '%s\n' GLIBC_2.39 "$required" | sort -V | tail -1)" \
    || return 1
  if ! test "$maximum" = GLIBC_2.39; then
    printf 'release ABI check failed: %s requires %s; maximum supported GLIBC version is GLIBC_2.39\n' \
      "$binary" "$required" >&2
    return 1
  fi
}

run_check() {
  local binary
  for binary in "$@"; do
    check_glibc "$binary"
    check_binary "$binary"
  done
}

run_self_test() {
  local mode="$1" fixture unfixed output
  fixture="$(mktemp -d)"
  trap 'rm -rf "$fixture"' RETURN
  mkdir -p "$fixture/bin"
  : >"$fixture/fslc"
  : >"$fixture/fslc-lsp"
  : >"$fixture/fslc-with-libz3"
  : >"$fixture/fslc-with-new-glibc"

  # This PATH stub stands in for Linux ldd so the shell control can run on
  # any host; it does not claim to inspect a real release artifact.
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    "case \"\$1\" in" \
    '  *with-libz3) printf "\\tlibz3.so.4 => /fixture/libz3.so.4 (0x00000000)\\n" ;;' \
    '  *) printf "\\tlibc.so.6 => /fixture/libc.so.6 (0x00000000)\\n" ;;' \
    'esac' \
    >"$fixture/bin/ldd"
  chmod +x "$fixture/bin/ldd"

  # This PATH stub controls only the GLIBC tokens the guard parses. It does
  # not claim that these fixtures reproduce real Linux readelf output.
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    "case \"\$2\" in" \
    '  *with-new-glibc) printf "Version needs section:\\n  Name: GLIBC_2.40\\n" ;;' \
    '  *) printf "Version needs section:\\n  Name: GLIBC_2.39\\n" ;;' \
    'esac' \
    >"$fixture/bin/readelf"
  chmod +x "$fixture/bin/readelf"

  # Calibration: this is the pre-fix loop. `!` exempts the failing pipeline
  # from errexit, so a bad non-final binary does not abort; the final clean
  # iteration then supplies the loop's successful status.
  unfixed="$fixture/unfixed-guard.sh"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -e' \
    'for binary in "$@"; do' \
    "  ! ldd \"\$binary\" | grep -q libz3" \
    'done' \
    >"$unfixed"
  chmod +x "$unfixed"
  if PATH="$fixture/bin:$PATH" bash -e "$unfixed" "$fixture/fslc-with-libz3" "$fixture/fslc-lsp"; then
    echo "release linkage self-test: unfixed guard accepts a libz3-linked non-final binary"
  else
    echo "release linkage self-test: unfixed guard did not reproduce the expected hole" >&2
    return 1
  fi

  if [ "$mode" = accept ]; then
    PATH="$fixture/bin:$PATH" "$0" "$fixture/fslc" "$fixture/fslc-lsp"
    echo "release linkage self-test: clean ldd and compliant readelf fixtures accepted"
    return 0
  fi

  if [ "$mode" = reject ]; then
    PATH="$fixture/bin:$PATH" "$0" "$fixture/fslc-with-libz3" "$fixture/fslc-lsp"
    return 0
  fi

  if output="$(PATH="$fixture/bin:$PATH" "$0" "$fixture/fslc" 2>&1)"; then
    echo "release linkage self-test: compliant readelf fixture accepted (GLIBC_2.39)"
  else
    printf 'release linkage self-test: compliant readelf fixture was rejected:\n%s\n' "$output" >&2
    return 1
  fi

  if ! PATH="$fixture/bin:$PATH" "$0" "$fixture/fslc" "$fixture/fslc-lsp"; then
    echo "release linkage self-test: clean ldd fixture was rejected" >&2
    return 1
  fi
  echo "release linkage self-test: clean ldd fixture accepted"

  if output="$(PATH="$fixture/bin:$PATH" "$0" "$fixture/fslc-with-libz3" "$fixture/fslc-lsp" 2>&1)"; then
    echo "release linkage self-test: libz3 ldd fixture unexpectedly passed" >&2
    return 1
  fi
  if ! grep -Fq "$fixture/fslc-with-libz3 dynamically links libz3" <<<"$output"; then
    printf 'release linkage self-test: rejecting diagnostic was incomplete:\n%s\n' "$output" >&2
    return 1
  fi
  echo "release linkage self-test: libz3 ldd fixture rejected"

  if output="$(PATH="$fixture/bin:$PATH" "$0" "$fixture/fslc-with-new-glibc" 2>&1)"; then
    echo "release linkage self-test: too-new readelf fixture unexpectedly passed" >&2
    return 1
  fi
  if ! grep -Fq 'requires GLIBC_2.40; maximum supported GLIBC version is GLIBC_2.39' <<<"$output"; then
    printf 'release linkage self-test: GLIBC rejection did not show required and maximum versions:\n%s\n' "$output" >&2
    return 1
  fi
  echo "release linkage self-test: too-new readelf fixture rejected (GLIBC_2.40 > GLIBC_2.39)"
}

if [ "${1:-}" = "--self-test" ]; then
  shift
  if [ "$#" -gt 1 ] || { [ "$#" -eq 1 ] && [ "$1" != accept ] && [ "$1" != reject ]; }; then
    echo "usage: $0 --self-test [accept|reject]" >&2
    exit 2
  fi
  run_self_test "${1:-all}"
elif [ "$#" -eq 0 ]; then
  echo "usage: $0 BINARY [BINARY ...]" >&2
  exit 2
else
  run_check "$@"
fi
