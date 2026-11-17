// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): a TypeScript app using the proved core.
// Everything imported here is generated: out/SeatBooking.ts (by gen-ts.mjs) over
// out/SeatBooking.cjs (compiled from the proved Dafny). This file only calls it.
// The core is proved for any number of seats and users, so it runs at sizes far
// beyond the spec's own (3 seats, 2 users).
//
// usage (after ./core.mjs ../../../specs/seat_booking.fsl --widen SeatId,UserId):
//   node example/seat-booking.ts

import { init, step, validate, CoreInputError, type Action, type Sizes, type State } from "../out/SeatBooking.ts";

const sizes: Sizes = { SeatId: 500, UserId: 100 };
let state: State = init(sizes);

// A reducer: the app keeps the state (in memory, a DB row, a React store) and
// asks the core for the next one. null means the spec does not allow the action.
function dispatch(label: string, action: Action): void {
  const next = step(sizes, state, action);
  console.log(`${next ? "ok    " : "denied"}  ${label}`);
  if (next) state = next;
}

dispatch("user 7 books seat 10", { action: "book", params: { s: 10, u: 7 } });
dispatch("user 7 books seat 11", { action: "book", params: { s: 11, u: 7 } });
dispatch("user 7 books seat 12 (third seat, over MAX_PER_USER = 2)", { action: "book", params: { s: 12, u: 7 } });
dispatch("user 8 books seat 10 (already taken)", { action: "book", params: { s: 10, u: 8 } });
dispatch("user 8 books seat 500 (no such seat)", { action: "book", params: { s: 500, u: 8 } });
dispatch("cancel seat 11", { action: "cancel", params: { s: 11 } });
dispatch("user 7 books seat 12 (back under the cap)", { action: "book", params: { s: 12, u: 7 } });
dispatch("cancel seat 11 again (already free)", { action: "cancel", params: { s: 11 } });

const owned = Object.entries(state.seats).flatMap(([s, u]) => (u === null ? [] : [`seat ${s} -> user ${u}`]));
console.log(owned.join("\n"));

// The state is plain JSON, so it can be stored and read back. A stored state is
// checked again: one that breaks the invariant is not a state the proof covers.
const stored = JSON.parse(JSON.stringify(state)) as State;
validate(sizes, stored);
const tampered: State = { seats: { ...stored.seats, 13: 7 } }; // user 7 would hold 3 seats
try {
  step(sizes, tampered, { action: "cancel", params: { s: 13 } });
} catch (err) {
  if (!(err instanceof CoreInputError)) throw err;
  console.log(`rejected stored state: ${err.message}`);
}
