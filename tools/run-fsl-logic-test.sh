#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tier="${1:-pr}"
case "$tier" in
  pr|scheduled) ;;
  *)
    echo "usage: $0 [pr|scheduled]" >&2
    exit 2
    ;;
esac

report="$root/rust/target/fsl-logic/$tier.json"
mkdir -p "$(dirname "$report")"
FSL_LOGIC_TIER="$tier" FSL_LOGIC_REPORT="$report" \
  cargo test --manifest-path "$root/rust/Cargo.toml" -p fslc-rust \
    --test typed_agreement --locked \
    logic_test::fsl_logic_generated_agreement_is_complete_and_replayable \
    -- --exact --nocapture

if ! jq -e '.complete == true and .expected == .executed and (.cases | length) == .executed' \
  "$report" >/dev/null; then
  echo "fsl-logic: incomplete report $report" >&2
  exit 1
fi
executed="$(jq -r '.executed' "$report")"
printf 'fsl-logic: tier=%s cases=%s report=%s\n' "$tier" "$executed" "$report"
