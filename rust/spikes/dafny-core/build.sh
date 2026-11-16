#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Spike (#1908): spec -> Kernel -> Dafny -> proof -> JS, for one spec.
# usage: ./build.sh <Name> <spec.fsl> <widen-list>   e.g. ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId
set -euo pipefail
cd "$(dirname "$0")"
name=$1 spec=$2 widen=${3:-}
fslc=${FSLC:-../../target/release/fslc}
mkdir -p out

"$fslc" kernel "$spec" > "out/$name.kernel.json"
node gen.mjs "out/$name.kernel.json" ${widen:+--widen "$widen"} > "out/$name.gen.dfy"

proof="${PROOF:-proofs/$name.proof.dfy}"
# The proof file must be one module that refines the generated one, including
# nothing else; otherwise it could export its own, weaker <Name>Proof.
code=$(grep -vE '^\s*(//.*)?$' "$proof")
if [ "$(grep -cE '^\s*include ' <<<"$code")" != 1 ] ||
   ! grep -qxE "include \"\.\./out/$name\.gen\.dfy\"" <<<"$code" ||
   [ "$(grep -cE '\bmodule\b' <<<"$code")" != 1 ] ||
   ! grep -qxE "module ${name}Proof refines ${name}Spec \{" <<<"$code"; then
  echo "structure: $proof must include only ../out/$name.gen.dfy and declare only 'module ${name}Proof refines ${name}Spec'" >&2
  exit 1
fi
# The proof module may not assume anything: no assume, no axioms, no bodiless lemmas.
audit=$(dafny audit "$proof" 2>&1 || true)
if ! grep -q 'completed with 0 findings' <<<"$audit" || grep -qi 'warning' <<<"$audit"; then
  echo "$audit" >&2
  echo "audit: $proof weakens the proof" >&2
  exit 1
fi

# Included files are not verified by default; the generated module has its own
# obligations (index ranges, termination, StepPreserves).
dafny verify --verify-included-files "$proof"
dafny translate js --include-runtime "$proof" -o "out/$name" > /dev/null
# Expose the compiled module to Node.
{
  cat "out/$name.js"
  echo "module.exports = { M: ${name}Proof, _dafny };"
} > "out/$name.cjs"
echo "built out/$name.cjs"
