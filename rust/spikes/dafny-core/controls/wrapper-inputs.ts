// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908) control: the generated wrapper must refuse every input the proof
// does not cover, with CoreInputError, and accept the ones it does.
// WRAPPER overrides the module under test (run-all.sh points it at a copy with
// a check removed, which must make this script fail).
//
// usage: node controls/wrapper-inputs.ts    exit 0 = every case behaved

const path = process.env.WRAPPER ?? new URL("../out/SeatBooking.ts", import.meta.url).pathname;
const { init, step, validate, CoreInputError } = await import(path);

const sizes = { SeatId: 4, UserId: 2 };
const s0 = init(sizes);
const two = step(sizes, step(sizes, s0, { action: "book", params: { s: 0, u: 1 } }), { action: "book", params: { s: 1, u: 1 } });
let bad = 0;
function refuses(label: string, f: () => unknown): void {
  try {
    f();
    console.log(`FAIL  accepted: ${label}`);
    bad++;
  } catch (err) {
    if (err instanceof CoreInputError) console.log(`ok    refused: ${label}`);
    else {
      console.log(`FAIL  ${label}: ${String(err)} (not a CoreInputError)`);
      bad++;
    }
  }
}
function accepts(label: string, f: () => unknown): void {
  try {
    f();
    console.log(`ok    accepted: ${label}`);
  } catch (err) {
    console.log(`FAIL  refused: ${label}: ${String(err)}`);
    bad++;
  }
}

accepts("a state reached by step", () => validate(sizes, two));
accepts("an out-of-range seat (the spec has no such instance; step returns null)", () => {
  if (step(sizes, s0, { action: "book", params: { s: 4, u: 0 } }) !== null) throw new Error("not null");
});
// Inside the type bounds but breaking PerUserCap: StepPreserves does not cover it.
refuses("a state that breaks PerUserCap", () => step(sizes, { seats: { ...two.seats, 2: 1 } }, { action: "cancel", params: { s: 3 } }));
refuses("a state outside the type bounds", () => validate(sizes, { seats: { ...s0.seats, 0: 2 } }));
refuses("a state with a missing key", () => validate(sizes, { seats: { 0: null, 1: null, 2: null } }));
refuses("a state with an extra field", () => validate(sizes, { ...s0, extra: 1 }));
for (const v of [1.5, Number.NaN, "1", 2 ** 53 + 1, null]) {
  refuses(`seat id ${typeof v === "string" ? JSON.stringify(v) : String(v)}`, () => step(sizes, s0, { action: "book", params: { s: v, u: 0 } }));
  refuses(`size ${typeof v === "string" ? JSON.stringify(v) : String(v)}`, () => init({ SeatId: v, UserId: 2 }));
}
refuses("a negative size", () => init({ SeatId: -1, UserId: 2 }));
refuses("an unknown size", () => init({ SeatId: 1, UserId: 1, Other: 1 }));
refuses("an unknown action", () => step(sizes, s0, { action: "steal", params: {} }));
refuses("an unknown parameter", () => step(sizes, s0, { action: "cancel", params: { s: 0, x: 1 } }));
refuses("a missing parameter", () => step(sizes, s0, { action: "book", params: { s: 0 } }));

process.exit(bad ? 1 : 0);
