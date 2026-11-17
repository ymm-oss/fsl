// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/RateLimiter.gen.dfy"

module RateLimiterProof refines RateLimiterSpec {
  lemma InitEstablishes(c: Config)
  {
  }

  lemma request_ok_Preserves(c: Config, st: State)
  {
  }

  lemma request_rejected_Preserves(c: Config, st: State)
  {
  }

  lemma tick_Preserves(c: Config, st: State)
  {
    var t := if (st.tokens + REFILL) <= BURST then (st.tokens + REFILL) else BURST;
    assert 0 <= t <= 3;
  }
}
