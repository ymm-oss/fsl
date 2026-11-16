// SPDX-License-Identifier: Apache-2.0
// Proof module for the generated SeatBookingSpec (the part an AI writes).

include "../out/SeatBooking.gen.dfy"

module SeatBookingProof refines SeatBookingSpec {
  predicate Match(o: Option<int>, u: int) { o.Some? && o.value == u }

  lemma CountUpd(c: Config, st: State, st2: State, s: int, w: Option<int>, u: int, i: nat)
    requires Wf(c, st) && Wf(c, st2) && 0 <= s < c.N_SeatId && st2.seats == st.seats[s := w]
    decreases c.N_SeatId - i
    ensures Count0(c, st2, u, i) as int ==
      Count0(c, st, u, i) as int
        - (if i <= s && Match(st.seats[s], u) then 1 else 0)
        + (if i <= s && Match(w, u) then 1 else 0)
  {
    if i < c.N_SeatId { CountUpd(c, st, st2, s, w, u, i + 1); }
  }

  lemma CountNone(c: Config, st: State, u: int, i: nat)
    requires Wf(c, st) && forall k :: 0 <= k < |st.seats| ==> st.seats[k] == None
    decreases c.N_SeatId - i
    ensures Count0(c, st, u, i) == 0
  {
    if i < c.N_SeatId { CountNone(c, st, u, i + 1); }
  }

  lemma InitEstablishes(c: Config) ensures Inv(c, Init(c)) {
    var st := Init(c);
    forall u | 0 <= u < c.N_UserId ensures Count0(c, st, u, 0) <= MAX_PER_USER { CountNone(c, st, u, 0); }
  }

  lemma book_Preserves(c: Config, st: State, s: int, u: int) {
    var st2 := book_Apply(c, st, s, u);
    assert Wf(c, st2);
    forall u2 | 0 <= u2 < c.N_UserId ensures Count0(c, st2, u2, 0) <= MAX_PER_USER {
      CountUpd(c, st, st2, s, Some(u), u2, 0);
    }
  }

  lemma cancel_Preserves(c: Config, st: State, s: int) {
    var st2 := cancel_Apply(c, st, s);
    assert Wf(c, st2);
    forall u2 | 0 <= u2 < c.N_UserId ensures Count0(c, st2, u2, 0) <= MAX_PER_USER {
      CountUpd(c, st, st2, s, None, u2, 0);
    }
  }
}
