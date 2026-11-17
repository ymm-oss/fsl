// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): how a TypeScript app uses the proved core.
// The core is out/SeatBooking.cjs, compiled from Dafny by ../build.sh. It is
// proved for any number of seats and users, so this example uses sizes far
// beyond the spec's own (3 seats, 2 users).
//
// usage (after ./build.sh SeatBooking ../../../specs/seat_booking.fsl SeatId,UserId):
//   node example/seat-booking.ts

import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const BigNumber = require("bignumber.js");
const { M } = require("../out/SeatBooking.cjs");
const D = M.__default;

// A thin typed wrapper. The state is an opaque, immutable Dafny value; every
// transition goes through the total <action>_Step, which returns None when the
// action is not allowed, so the app cannot leave the proved state space.
type DafnyState = { readonly __brand: "SeatBooking.State" };
type SeatId = number;
type UserId = number;

class SeatBooking {
  private readonly config: unknown;
  private state: DafnyState;

  constructor(seatCount: number, userCount: number) {
    this.config = M.Config.create_Config(new BigNumber(seatCount), new BigNumber(userCount));
    this.state = D.Init(this.config);
  }

  /** Returns false when the seat is taken, the user is at the cap, or an id is out of range. */
  book(seat: SeatId, user: UserId): boolean {
    return this.apply(D.book__Step(this.config, this.state, new BigNumber(seat), new BigNumber(user)));
  }

  /** Returns false when the seat is already free or out of range. */
  cancel(seat: SeatId): boolean {
    return this.apply(D.cancel__Step(this.config, this.state, new BigNumber(seat)));
  }

  /** owner of each seat, or null when free */
  seats(): (UserId | null)[] {
    return Array.from(D.Get__seats(this.state) as Iterable<any>, (o) => (o.is_None ? null : o.dtor_value.toNumber()));
  }

  private apply(result: any): boolean {
    if (result.is_None) return false;
    this.state = result.dtor_value;
    return true;
  }
}

// ---- a short session --------------------------------------------------------
const show = (label: string, ok: boolean) => console.log(`${ok ? "ok    " : "denied"}  ${label}`);
const app = new SeatBooking(500, 100);

show("user 7 books seat 10", app.book(10, 7));
show("user 7 books seat 11", app.book(11, 7));
show("user 7 books seat 12 (third seat, over MAX_PER_USER = 2)", app.book(12, 7));
show("user 8 books seat 10 (already taken)", app.book(10, 8));
show("user 8 books seat 500 (no such seat)", app.book(500, 8));
show("cancel seat 11", app.cancel(11));
show("user 7 books seat 12 (back under the cap)", app.book(12, 7));
show("cancel seat 11 again (already free)", app.cancel(11));

const owned = app.seats().flatMap((u, s) => (u === null ? [] : [`seat ${s} -> user ${u}`]));
console.log(owned.join("\n"));
