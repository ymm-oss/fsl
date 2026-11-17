// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/Batching.gen.dfy"

module BatchingProof refines BatchingSpec {

  lemma InitEstablishes(c: Config)
  {
  }

  lemma add_Preserves(c: Config, st: State, n: int)
  {
    var r := add_Apply(c, st, n);
    assert r.pending == st.pending + n;
    assert 0 <= r.pending <= 9;
  }

  lemma NegDivNonPositive(p: int)
    requires 0 < p <= 9
    ensures (0 - p) / 4 <= 0
  {
    if p == 1 { assert (0 - 1) / 4 == -1; }
    else if p == 2 { assert (0 - 2) / 4 == -1; }
    else if p == 3 { assert (0 - 3) / 4 == -1; }
    else if p == 4 { assert (0 - 4) / 4 == -1; }
    else if p == 5 { assert (0 - 5) / 4 == -2; }
    else if p == 6 { assert (0 - 6) / 4 == -2; }
    else if p == 7 { assert (0 - 7) / 4 == -2; }
    else if p == 8 { assert (0 - 8) / 4 == -2; }
    else { assert p == 9; assert (0 - 9) / 4 == -3; }
  }

  lemma flush_Preserves(c: Config, st: State)
  {
    var r := flush_Apply(c, st);
    assert 0 <= st.pending / BATCH;
    assert 0 <= st.pending % BATCH < BATCH;
    NegDivNonPositive(st.pending);
    assert r.batches == st.batches + st.pending / BATCH;
    assert r.rest == st.pending % BATCH;
    assert r.debt == (0 - st.pending) / BATCH;
    assert r.pending == 0;
    assert r.share == st.share;
  }

  lemma split_Preserves(c: Config, st: State, k: int)
  {
    var r := split_Apply(c, st, k);
    if k == 1 {
      assert st.pending % 1 == 0;
    } else if k == 2 {
      assert 0 <= st.pending % 2 < 2;
    } else {
      assert k == 3;
      assert 0 <= st.pending % 3 < 3;
    }
    assert r.share == st.pending % k;
    assert 0 <= r.share < 3;
  }
}
