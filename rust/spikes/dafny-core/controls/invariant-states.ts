// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908) control: the differential check compares steps, not invariant
// predicates, so the translation of `invariant` bodies is checked here instead.
// For TaskBoard each state below is classified by hand from the spec
// (fixtures/task_board.fsl) and must be accepted or refused by `validate`,
// which evaluates the generated Dafny Inv. The cases separate `=>` from `and`
// and from the reversed implication.
// WRAPPER overrides the module under test (run-all.sh points it at cores whose
// generated invariants were altered, which must make this script fail).
//
// usage: node controls/invariant-states.ts    exit 0 = every case behaved

const path = process.env.WRAPPER ?? new URL("../out/TaskBoard.ts", import.meta.url).pathname;
const { validate, CoreInputError } = await import(path);

const sizes = { TaskId: 3, WorkerId: 2 };
const st = (owner: (number | null)[], done: boolean[]) => ({
  owner: Object.fromEntries(owner.map((o, k) => [String(k), o])),
  done: Object.fromEntries(done.map((d, k) => [String(k), d])),
});
const cases: [string, boolean, ReturnType<typeof st>][] = [
  ["initial", true, st([null, null, null], [false, false, false])],
  ["claimed, not done (=> with a false premise)", true, st([0, 1, null], [false, false, false])],
  ["done and free (=> with a true premise and conclusion)", true, st([null, null, null], [true, false, false])],
  ["done but owned: breaks DoneIsFree", false, st([0, null, null], [true, false, false])],
  ["worker 0 holds two tasks: breaks WipLimit", false, st([0, 0, null], [false, false, false])],
];
let bad = 0;
for (const [label, ok, state] of cases) {
  let accepted = true;
  try {
    validate(sizes, state);
  } catch (err) {
    if (!(err instanceof CoreInputError)) throw err;
    accepted = false;
  }
  const good = accepted === ok;
  if (!good) bad++;
  console.log(`${good ? "ok  " : "FAIL"}  ${accepted ? "accepted" : "refused "}: ${label}`);
}
process.exit(bad ? 1 : 0);
