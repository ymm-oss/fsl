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
  "$@" < /dev/null > "out/last.log" 2>&1
  local got=$?
  if { [ "$want" = 0 ] && [ $got -eq 0 ]; } || { [ "$want" != 0 ] && [ $got -ne 0 ]; }; then
    echo "ok    $label (exit $got)"
  else
    echo "FAIL  $label (exit $got, wanted $want)"; cat out/last.log; status=1
  fi
}

mkdir -p out/neg
: > out/core-results.jsonl
# core <label> <core.mjs args...>: expect success and keep the JSON report.
core() {
  local label=$1; shift
  expect 0 "core.mjs $label" node core.mjs "$@"
  grep -E '^\{' out/last.log >> out/core-results.jsonl || true
}
# The one command: proof gates, JS, generated TS + tsc, exhaustive differential at the
# spec's sizes, unchanged Dafny and seeded random differential at 4x the sizes.
# TaskBoard declares its identity types `symmetric`, so it needs no --widen.
core seat_booking ../../../specs/seat_booking.fsl --widen SeatId,UserId --seed 1
core cart_v1 ../../../specs/cart_v1.fsl --widen UserId,ItemId --seed 1
core "fixtures/task_board (symmetric)" fixtures/task_board.fsl --seed 1
core "fixtures/batching (/ and %)" fixtures/batching.fsl --seed 1 --depth 8
# leadsTo is not translated: without --liveness-by-fslc the spec is refused.
mkdir -p out/neg/liveness
../../target/release/fslc kernel ../../../specs/mutex_queue.fsl > out/neg/liveness/MutexQueue.kernel.json
expect 2 "gen.mjs refuses mutex_queue's leadsTo without --liveness-by-fslc" node gen.mjs out/neg/liveness/MutexQueue.kernel.json --widen ProcId
grep -q '^unsupported: properties.leads_to' out/last.log || { echo "FAIL  leadsTo refusal: unexpected message"; status=1; }
# A reachable partial read (head/pop on an empty Seq) does not verify in Dafny.
mkdir -p out/neg/partial
../../target/release/fslc kernel fixtures/negative/unguarded_head.fsl > out/neg/partial/UnguardedHead.kernel.json
node gen.mjs out/neg/partial/UnguardedHead.kernel.json > out/neg/partial/UnguardedHead.gen.dfy
expect 1 "Dafny rejects a generated module with an unguarded head()/pop()" dafny verify --allow-warnings out/neg/partial/UnguardedHead.gen.dfy
grep -qE 'Error: (lower bound|index) out of range' out/last.log || { echo "FAIL  unguarded head(): not rejected for the range"; status=1; }
# Widening scales a type only; a bound constant the spec also reads is refused.
expect 2 "core.mjs refuses widening a type whose bound constant is also read" node core.mjs fixtures/negative/const_bound.fsl --widen T --seed 1
grep -q 'is also read by the spec' out/last.log || { echo "FAIL  const-bound refusal: unexpected message"; status=1; }
# Enums, structs, field updates, if, sum, min; proofs written by the model through
# prove.mjs. Specs whose counters are unbounded are searched to depth 8.
while read -r f args; do
  # shellcheck disable=SC2086
  core "$f $args" "../../../specs/$f.fsl" --seed 1 $args
done <<'SPECS'
repair_loop
rate_limiter --depth 8
bank --depth 8
bank_impl --depth 8
auth_lockout --widen UserId
cart_impl --widen UserId,ItemId
seat_booking_impl --widen SeatId,UserId
inventory_reservation --widen ResId,ItemId
payment --widen PayId
audit_log --depth 8
job_pipeline --widen JobId
order_workflow --widen OrderId
mutex_queue --widen ProcId --liveness-by-fslc
cart_fixed --name CartFixed --widen UserId,ItemId
SPECS
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
sed 's/if (t \&\& t.key === sizesKey(sizes)) return t.st;/if (t) return t.st;/' out/SeatBooking.ts > out/SeatBooking.no-key.ts
if cmp -s out/SeatBooking.ts out/SeatBooking.no-key.ts; then echo "FAIL  no-key mutant did not apply"; status=1; fi
expect 1 "wrapper control rejects a wrapper that trusts a state for other sizes" env WRAPPER="$PWD/out/SeatBooking.no-key.ts" node controls/wrapper-inputs.ts
sed 's/    Object.freeze(v);//' out/SeatBooking.ts > out/SeatBooking.no-freeze.ts
if cmp -s out/SeatBooking.ts out/SeatBooking.no-freeze.ts; then echo "FAIL  no-freeze mutant did not apply"; status=1; fi
expect 1 "wrapper control rejects a wrapper that does not freeze trusted states" env WRAPPER="$PWD/out/SeatBooking.no-freeze.ts" node controls/wrapper-inputs.ts
rm -f out/SeatBooking.no-inv.ts out/SeatBooking.no-int.ts out/SeatBooking.no-key.ts out/SeatBooking.no-freeze.ts

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
  expect 1 "invcheck rejects a core whose $1" env WRAPPER="$PWD/$d/TaskBoard.ts" node invcheck.mjs TaskBoard --samples 100
  if ! grep -qE '"disagreements": [1-9]' out/last.log; then echo "FAIL  invcheck $1: no disagreement reported"; status=1; fi
}
inv_mutant "DoneIsFree is true" '/predicate Inv_DoneIsFree/{n;s/.*/    true/;}'
inv_mutant "DoneIsFree uses and for =>" '/predicate Inv_DoneIsFree/{n;s/(st.done\[t_2\] ==> /(st.done[t_2] \&\& /;}'
inv_mutant "DoneIsFree reverses =>" '/predicate Inv_DoneIsFree/{n;s/(st.done\[t_2\] ==> (st.owner\[t_2\] == None))/((st.owner[t_2] == None) ==> st.done[t_2])/;}'
inv_mutant "WipLimit is true" '/predicate Inv_WipLimit/{n;s/.*/    true/;}'

# Translator-shaped mutants of the newer constructs (if, enum, min, sum): the
# generated Dafny is altered, compiled without verification, and the exhaustive
# differential must reject the core.
gen_mutant() { # gen_mutant <Name> <label> <sed expression> [check.mjs args...]
  local n=$1 label=$2 expr=$3; shift 3
  local d="out/neg/gen-$n-${label// /-}"
  rm -rf "$d" && mkdir -p "$d/out" "$d/proofs"
  sed "$expr" "out/$n.gen.dfy" > "$d/out/$n.gen.dfy"
  if [ ! -s "$d/out/$n.gen.dfy" ] || cmp -s "out/$n.gen.dfy" "$d/out/$n.gen.dfy"; then
    echo "FAIL  generator mutant $n $label did not apply"; status=1; return
  fi
  if ! { cp "proofs/$n.proof.dfy" "$d/proofs/" &&
         dafny translate js --no-verify --include-runtime "$d/proofs/$n.proof.dfy" -o "$d/core" > "$d/translate.log" 2>&1 &&
         { cat "$d/core.js"; echo "module.exports = { M: ${n}Proof, _dafny, BigNumber };"; } > "$d/$n.cjs"; }; then
    echo "FAIL  generator mutant $n $label did not build"; status=1; return
  fi
  expect 1 "differential rejects $n with $label" env CORE_CJS="$PWD/$d/$n.cjs" node check.mjs "$n" "$@"
  # Rejected by a reported divergence, not by a crash.
  if ! grep -qE '"divergences": [1-9]' out/last.log; then echo "FAIL  $n $label: no divergence reported"; status=1; fi
}
gen_mutant AuthLockout "the if condition changed" '/function login_fail_Apply/,/^  }/s/attempts == 2/attempts == 1/g'
gen_mutant Payment "capture setting the wrong enum member" '/function capture_Apply/,/^  }/s/PSt\.Captured/PSt.Voided/'
gen_mutant RateLimiter "min turned into max" 's/(if (st.tokens + REFILL) <= BURST then/(if (st.tokens + REFILL) >= BURST then/' --depth 8
gen_mutant Payment "the ledger sum adding refunds" '/function Sum0/,/^  }/s/amount - st.payments\[i\].refunded/amount + st.payments[i].refunded/'
gen_mutant Batching "truncating division" 's|debt := ((0 - st.pending) / BATCH)|debt := (-(st.pending / BATCH))|' --depth 8
gen_mutant MutexQueue "pop taking the last waiter" 's|waiters := st.waiters\[1..\])|waiters := st.waiters[..\|st.waiters\| - 1])|'
gen_mutant JobPipeline "let reading the last queued job" 's|var j := st.queue\[0\];|var j := st.queue[\|st.queue\| - 1];|'
gen_mutant OrderWorkflow "ship not adding to the set" 's|shipped := (st.shipped + {o}))|shipped := st.shipped)|'

# The random mode catches a wrong core too, at the enlarged sizes.
expect 1 "random differential rejects SeatBooking --mutate drop-guard:book" env KERNEL="$PWD/out/big/SeatBooking.kernel.json" node check.mjs SeatBooking --random 20 --seed 1 --mutate drop-guard:book

# Negative controls: the differential check must reject a wrong core.
for m in drop-guard:book drop-guard:cancel skip-effect:cancel; do
  expect 1 "differential rejects SeatBooking --mutate $m" node check.mjs SeatBooking --mutate "$m"
done
for m in drop-guard:checkout skip-effect:checkout skip-effect:add_to_cart; do
  expect 1 "differential rejects ShoppingCart --mutate $m" node check.mjs ShoppingCart --mutate "$m"
done

# fslc's testgen suite, run against the core, must fail a wrong core too: a lost
# effect (a baked expected state differs) and a refused allowed step.
expect 1 "testgen suite rejects ShoppingCart --mutate skip-effect:checkout" node testgen.mjs ShoppingCart --mutate skip-effect:checkout
if ! grep -qE '"failed": [1-9]' out/last.log; then echo "FAIL  testgen skip-effect: no failed scenario reported"; status=1; fi
expect 1 "testgen suite rejects SeatBooking --mutate refuse:cancel" node testgen.mjs SeatBooking --mutate refuse:cancel
if ! grep -q "which the spec allows" out/last.log; then echo "FAIL  testgen refuse: the refused step was not reported"; status=1; fi

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

# A spec with a real bug: cart_v1's proof does not carry over to cart_v1_buggy.
expect 1 "core.mjs refuses cart_v1_buggy (checkout can drive stock below 0)" node core.mjs ../../../specs/cart_v1_buggy.fsl --widen UserId,ItemId --seed 1
# cart_buggy is cart_fixed without the stock guard: under cart_fixed's name, its proof must not verify.
expect 1 "core.mjs refuses cart_buggy with cart_fixed's proof" node core.mjs ../../../specs/cart_buggy.fsl --name CartFixed --widen UserId,ItemId --seed 1
grep -qE 'error|Error' out/last.log || { echo "FAIL  cart_buggy was not refused by Dafny"; status=1; }

# Rebuild the real cores: the cheats above went through build.sh.
expect 0 "rebuild SeatBooking" ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId
expect 0 "rebuild ShoppingCart" ./build.sh ShoppingCart ../../../specs/cart_v1.fsl UserId,ItemId
expect 0 "rebuild CartFixed" ./build.sh CartFixed ../../../specs/cart_fixed.fsl UserId,ItemId

exit $status
