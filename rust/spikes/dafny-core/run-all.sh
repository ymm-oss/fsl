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
# The one command: proof gates, JS, generated TS + tsc, exhaustive differential at the
# spec's sizes, unchanged Dafny and seeded random differential at 4x the sizes.
# TaskBoard declares its identity types `symmetric`, so it needs no --widen.
expect 0 "core.mjs seat_booking" node core.mjs ../../../specs/seat_booking.fsl --widen SeatId,UserId --seed 1
expect 0 "core.mjs cart_v1" node core.mjs ../../../specs/cart_v1.fsl --widen UserId,ItemId --seed 1
expect 0 "core.mjs fixtures/task_board (symmetric)" node core.mjs fixtures/task_board.fsl --seed 1
expect 0 "tsc: generated cores and the example type-check" node_modules/.bin/tsc -p .
expect 0 "example/seat-booking.ts runs on the generated core" node example/seat-booking.ts

# The generated wrapper refuses what the proof does not cover (non-integers,
# unknown keys, states breaking Wf or Inv) ...
expect 0 "generated wrapper refuses uncovered inputs" node controls/wrapper-inputs.ts
# ... and the control notices when one of those checks is missing.
sed 's|if (!fn("Inv")(c, st)) throw|if (false) throw|' out/SeatBooking.ts > out/SeatBooking.no-inv.ts
if cmp -s out/SeatBooking.ts out/SeatBooking.no-inv.ts; then echo "FAIL  no-inv mutant did not apply"; status=1; fi
expect 1 "wrapper control rejects a wrapper without the Inv check" env WRAPPER="$PWD/out/SeatBooking.no-inv.ts" node controls/wrapper-inputs.ts
sed 's/if (typeof v !== "number" || !Number.isSafeInteger(v)) throw/if (false) throw/' out/SeatBooking.ts > out/SeatBooking.no-int.ts
if cmp -s out/SeatBooking.ts out/SeatBooking.no-int.ts; then echo "FAIL  no-int mutant did not apply"; status=1; fi
expect 1 "wrapper control rejects a wrapper without the integer check" env WRAPPER="$PWD/out/SeatBooking.no-int.ts" node controls/wrapper-inputs.ts
rm -f out/SeatBooking.no-inv.ts out/SeatBooking.no-int.ts

# Invariant bodies are outside the step differential; controls/invariant-states.ts
# checks their translation on hand-classified TaskBoard states, and must reject a
# core compiled from generated Dafny whose invariants were altered.
expect 0 "generated invariants classify hand-checked TaskBoard states" node controls/invariant-states.ts
inv_mutant() { # inv_mutant <label> <sed expression>
  local d=out/neg/inv-$1
  rm -rf "$d" && mkdir -p "$d/out" "$d/proofs"
  sed "$2" out/TaskBoard.gen.dfy > "$d/out/TaskBoard.gen.dfy"
  if [ ! -s "$d/out/TaskBoard.gen.dfy" ] || cmp -s out/TaskBoard.gen.dfy "$d/out/TaskBoard.gen.dfy"; then
    echo "FAIL  invariant mutant $1 did not apply"; status=1; return
  fi
  if ! { cp proofs/TaskBoard.proof.dfy "$d/proofs/" &&
         dafny translate js --no-verify --include-runtime "$d/proofs/TaskBoard.proof.dfy" -o "$d/core" > "$d/translate.log" 2>&1 &&
         { cat "$d/core.js"; echo "module.exports = { M: TaskBoardProof, _dafny, BigNumber };"; } > "$d/TaskBoard.cjs" &&
         cp out/TaskBoard.ts "$d/TaskBoard.ts"; }; then
    echo "FAIL  invariant mutant $1 did not build"; status=1; return
  fi
  expect 1 "invariant control rejects a core whose $1" env WRAPPER="$PWD/$d/TaskBoard.ts" node controls/invariant-states.ts
}
inv_mutant "DoneIsFree is true" '/predicate Inv_DoneIsFree/{n;s/.*/    true/;}'
inv_mutant "DoneIsFree uses and for =>" '/predicate Inv_DoneIsFree/{n;s/(st.done\[t_2\] ==> /(st.done[t_2] \&\& /;}'
inv_mutant "DoneIsFree reverses =>" '/predicate Inv_DoneIsFree/{n;s/(st.done\[t_2\] ==> (st.owner\[t_2\] == None))/((st.owner[t_2] == None) ==> st.done[t_2])/;}'
inv_mutant "WipLimit is true" '/predicate Inv_WipLimit/{n;s/.*/    true/;}'

# The random mode catches a wrong core too, at the enlarged sizes.
expect 1 "random differential rejects SeatBooking --mutate drop-guard:book" env KERNEL="$PWD/out/big/SeatBooking.kernel.json" node check.mjs SeatBooking --random 20 --seed 1 --mutate drop-guard:book

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
# (e) a second include that does not start its line
{ echo 'include "../out/SeatBooking.gen.dfy"'; echo '/* */ include "../out/SeatBooking.gen.dfy"'; grep -v '^include ' proofs/SeatBooking.proof.dfy; } > "$neg/proofs/include.dfy"
for cheat in standalone extra include; do
  expect 1 "build rejects a proof file that is $cheat" env PROOF="$neg/proofs/$cheat.dfy" ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId
done
for cheat in redefine requires; do
  if cmp -s proofs/SeatBooking.proof.dfy "$neg/proofs/$cheat.dfy"; then echo "FAIL  refine mutant $cheat did not apply"; status=1; fi
  expect 1 "Dafny rejects a refining proof that tries to $cheat" dafny verify --verify-included-files "$neg/proofs/$cheat.dfy"
done
# The core compiled from the spec at its own sizes, checked against the spec at
# larger sizes: the generated Dafny must not change, and the Monitor must agree.
big=out/plus1 && mkdir -p "$big"  # exhaustive one size up (core.mjs uses out/big for 4x random)
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

# The proof loop, with a stand-in model (deterministic, no tokens): a reply that
# cheats is rejected by the unchanged gates; a correct one is accepted.
neg=out/neg/loop && rm -rf "$neg" && mkdir -p "$neg"
rm -rf .loop && mkdir .loop
sed 's/var st2 := book_Apply(c, st, s, u);/assume false; var st2 := book_Apply(c, st, s, u);/' proofs/SeatBooking.proof.dfy > "$neg/cheat.dfy"
if cmp -s proofs/SeatBooking.proof.dfy "$neg/cheat.dfy"; then echo "FAIL  loop cheat did not apply"; status=1; fi
expect 1 "proof loop rejects a model reply that assumes" env CLAUDE="node controls/fake-model.mjs" FAKE_REPLY="$neg/cheat.dfy" \
  node prove.mjs SeatBooking ../../../specs/seat_booking.fsl --widen SeatId,UserId --proof .loop/SeatBooking.proof.dfy --attempts 2
rm -f .loop/SeatBooking.proof.dfy
expect 0 "proof loop accepts a correct model reply" env CLAUDE="node controls/fake-model.mjs" FAKE_REPLY="$PWD/proofs/SeatBooking.proof.dfy" \
  node prove.mjs SeatBooking ../../../specs/seat_booking.fsl --widen SeatId,UserId --proof .loop/SeatBooking.proof.dfy --attempts 1
rm -rf .loop

# With a real model (opt-in: tokens and minutes): from no proof at all it must
# reach one the gates accept, and on a spec with a real bug it must fail.
if [ "${RUN_AI:-0}" = 1 ]; then
  rm -rf .ai && mkdir .ai
  expect 0 "AI proves SeatBooking from an empty proof" node prove.mjs SeatBooking ../../../specs/seat_booking.fsl --widen SeatId,UserId --proof .ai/SeatBooking.proof.dfy
  expect 1 "AI cannot prove cart_v1_buggy" node prove.mjs ShoppingCart ../../../specs/cart_v1_buggy.fsl --widen UserId,ItemId --proof .ai/ShoppingCart.proof.dfy
  rm -rf .ai
else
  echo "skip  AI proof loop with a real model (RUN_AI=1 runs it with the claude CLI)"
fi

# Rebuild the real cores: the cheats above went through build.sh.
expect 0 "rebuild SeatBooking" ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId
expect 0 "rebuild ShoppingCart" ./build.sh ShoppingCart ../../../specs/cart_v1.fsl UserId,ItemId

exit $status
