#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Spike (#1908) end to end: build and prove both specs, run the differential
# check, then require every negative control to be caught.
set -uo pipefail
cd "$(dirname "$0")"
status=0
expect() { # expect <0|nonzero> <label> <command...>
  local want=$1 label=$2; shift 2
  "$@" > "out/last.log" 2>&1
  local got=$?
  if { [ "$want" = 0 ] && [ $got -eq 0 ]; } || { [ "$want" != 0 ] && [ $got -ne 0 ]; }; then
    echo "ok    $label (exit $got)"
  else
    echo "FAIL  $label (exit $got, wanted $want)"; cat out/last.log; status=1
  fi
}

mkdir -p out/neg
expect 0 "build+prove SeatBooking" ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId
expect 0 "build+prove ShoppingCart" ./build.sh ShoppingCart ../../../specs/cart_v1.fsl UserId,ItemId
expect 0 "differential SeatBooking" node check.mjs SeatBooking
expect 0 "differential ShoppingCart" node check.mjs ShoppingCart

# Negative controls: the differential check must reject a wrong core.
for m in drop-guard:book drop-guard:cancel skip-effect:cancel; do
  expect 1 "differential rejects SeatBooking --mutate $m" node check.mjs SeatBooking --mutate "$m"
done
for m in drop-guard:checkout skip-effect:checkout skip-effect:add_to_cart; do
  expect 1 "differential rejects ShoppingCart --mutate $m" node check.mjs ShoppingCart --mutate "$m"
done

# A translator-shaped bug: the stock guard of checkout is lost in the generated Dafny.
neg=out/neg/cart
rm -rf "$neg" && mkdir -p "$neg/out" "$neg/proofs"
sed 's/st.cart\[u\].Some? && (var i := st.cart\[u\].value; (st.stock\[i\] > 0) && true)/st.cart[u].Some? \&\& true/' \
  out/ShoppingCart.gen.dfy > "$neg/out/ShoppingCart.gen.dfy"
if cmp -s out/ShoppingCart.gen.dfy "$neg/out/ShoppingCart.gen.dfy"; then echo "FAIL  stock-guard mutant did not apply"; status=1; fi
cp proofs/ShoppingCart.proof.dfy "$neg/proofs/"
expect 1 "proof rejects ShoppingCart without the stock guard" dafny verify --verify-included-files "$neg/proofs/ShoppingCart.proof.dfy"
dafny translate js --no-verify --include-runtime "$neg/proofs/ShoppingCart.proof.dfy" -o out/neg/cart-no-stock-guard > /dev/null 2>&1
{ cat out/neg/cart-no-stock-guard.js; echo "module.exports = { M: ShoppingCartProof, _dafny };"; } > out/neg/cart-no-stock-guard.cjs
expect 1 "differential rejects ShoppingCart without the stock guard" env CORE_CJS="$PWD/out/neg/cart-no-stock-guard.cjs" node check.mjs ShoppingCart

# The per-user cap guard weakened (< to <=) in the generated SeatBooking Dafny.
neg=out/neg/seat
rm -rf "$neg" && mkdir -p "$neg/out" "$neg/proofs"
sed 's/(Count0(c, st, u, 0) < MAX_PER_USER)/(Count0(c, st, u, 0) <= MAX_PER_USER)/' \
  out/SeatBooking.gen.dfy > "$neg/out/SeatBooking.gen.dfy"
if cmp -s out/SeatBooking.gen.dfy "$neg/out/SeatBooking.gen.dfy"; then echo "FAIL  cap mutant did not apply"; status=1; fi
cp proofs/SeatBooking.proof.dfy "$neg/proofs/"
expect 1 "proof rejects SeatBooking with the cap guard weakened" dafny verify --verify-included-files "$neg/proofs/SeatBooking.proof.dfy"

# The generated module's own obligations are verified too (StepPreserves left without its proof).
neg=out/neg/step
rm -rf "$neg" && mkdir -p "$neg/out" "$neg/proofs"
sed 's/{ book_Preserves(c, st, s, u); }/{ }/' out/SeatBooking.gen.dfy > "$neg/out/SeatBooking.gen.dfy"
if cmp -s out/SeatBooking.gen.dfy "$neg/out/SeatBooking.gen.dfy"; then echo "FAIL  step mutant did not apply"; status=1; fi
cp proofs/SeatBooking.proof.dfy "$neg/proofs/"
expect 1 "proof rejects a generated module whose StepPreserves is unproved" dafny verify --verify-included-files "$neg/proofs/SeatBooking.proof.dfy"

# The audit must reject a proof that assumes, axiomatizes, skips, or omits.
for cheat in assume axiom verify-false omit; do
  neg=out/neg/audit-$cheat
  rm -rf "$neg" && mkdir -p "$neg/out" "$neg/proofs"
  cp out/SeatBooking.gen.dfy "$neg/out/"
  case $cheat in
    assume) sed 's/var st2 := book_Apply(c, st, s, u);/assume false; var st2 := book_Apply(c, st, s, u);/' ;;
    axiom) sed 's/lemma book_Preserves(/lemma {:axiom} book_Preserves(/' ;;
    verify-false) sed 's/lemma book_Preserves(/lemma {:verify false} book_Preserves(/' ;;
    omit) python3 -c 'import sys;s=sys.stdin.read();a=s.index("  lemma cancel_Preserves");b=s.index("\n  }\n",a)+4;print(s[:a]+s[b:],end="")' ;;
  esac < proofs/SeatBooking.proof.dfy > "$neg/proofs/SeatBooking.proof.dfy"
  if cmp -s proofs/SeatBooking.proof.dfy "$neg/proofs/SeatBooking.proof.dfy"; then echo "FAIL  audit mutant $cheat did not apply"; status=1; fi
  expect 1 "audit rejects a proof that uses $cheat" bash -c "a=\$(dafny audit '$neg/proofs/SeatBooking.proof.dfy' 2>&1); grep -q 'completed with 0 findings' <<<\"\$a\" && ! grep -qi warning <<<\"\$a\""
done

# The proof file must refine the generated module and may not restate it.
neg=out/neg/refine
rm -rf "$neg" && mkdir -p "$neg/out" "$neg/proofs"
cp out/SeatBooking.gen.dfy "$neg/out/"
# (a) its own module named SeatBookingProof, with a weaker Inv, refining nothing
cat > "$neg/proofs/standalone.dfy" <<'DFY'
include "../out/SeatBooking.gen.dfy"
module SeatBookingProof {
  datatype Config = Config(N_SeatId: nat, N_UserId: nat)
  predicate Inv(c: Config, st: int) { true }
}
DFY
# (b) a second module beside the refining one
{ cat proofs/SeatBooking.proof.dfy; printf 'module Extra {\n}\n'; } > "$neg/proofs/extra.dfy"
# (c) redefine a predicate body; (d) add a precondition to a lemma
sed 's/  predicate Match(o: Option<int>, u: int)/  predicate Inv(c: Config, st: State) { true }\n  predicate Match(o: Option<int>, u: int)/' \
  proofs/SeatBooking.proof.dfy > "$neg/proofs/redefine.dfy"
sed 's/  lemma cancel_Preserves(c: Config, st: State, s: int) {/  lemma cancel_Preserves(c: Config, st: State, s: int) requires false {/' \
  proofs/SeatBooking.proof.dfy > "$neg/proofs/requires.dfy"
for cheat in standalone extra; do
  expect 1 "build rejects a proof file that is $cheat" env PROOF="$neg/proofs/$cheat.dfy" ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId
done
for cheat in redefine requires; do
  if cmp -s proofs/SeatBooking.proof.dfy "$neg/proofs/$cheat.dfy"; then echo "FAIL  refine mutant $cheat did not apply"; status=1; fi
  expect 1 "Dafny rejects a refining proof that tries to $cheat" dafny verify --verify-included-files "$neg/proofs/$cheat.dfy"
done
# The core compiled from the spec at its own sizes, checked against the spec at
# larger sizes: the generated Dafny must not change, and the Monitor must agree.
big=out/big && mkdir -p "$big"
sed -e 's/type SeatId = 0..2/type SeatId = 0..3/' ../../../specs/seat_booking.fsl > "$big/seat_booking.fsl"
sed -e 's/type UserId = 0..1/type UserId = 0..2/' ../../../specs/cart_v1.fsl > "$big/cart_v1.fsl"
for pair in SeatBooking:seat_booking:SeatId,UserId ShoppingCart:cart_v1:UserId,ItemId; do
  IFS=: read -r n f w <<<"$pair"
  if cmp -s "../../../specs/$f.fsl" "$big/$f.fsl"; then echo "FAIL  larger $f did not apply"; status=1; fi
  ../../target/release/fslc kernel "$big/$f.fsl" > "$big/$n.kernel.json"
  node gen.mjs "$big/$n.kernel.json" --widen "$w" > "$big/$n.gen.dfy"
  expect 0 "generated Dafny for larger $f is unchanged" diff <(tail -n +3 "out/$n.gen.dfy") <(tail -n +3 "$big/$n.gen.dfy")
  expect 0 "differential $n core against larger $f" env KERNEL="$big/$n.kernel.json" node check.mjs "$n"
  grep -E '"(reachable_states|steps_checked|divergences)"' out/last.log | tr -d ' \n'; echo
done

# Rebuild the real core: the cheats above went through build.sh.
expect 0 "rebuild SeatBooking" ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId

exit $status
