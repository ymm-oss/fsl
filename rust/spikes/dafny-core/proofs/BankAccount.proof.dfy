// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/BankAccount.gen.dfy"

module BankAccountProof refines BankAccountSpec {
  lemma InitEstablishes(c: Config)
  {
    assert Wf(c, Init(c));
    assert Init(c).balance == 0;
  }

  lemma deposit_Preserves(c: Config, st: State, a: int)
  {
    var st2 := deposit_Apply(c, st, a);
    assert st2.balance == st.balance + a;
    assert Wf(c, st2);
    assert st2.balance >= 0;
  }

  lemma withdraw_Preserves(c: Config, st: State, a: int)
  {
    var st2 := withdraw_Apply(c, st, a);
    assert st2.balance == st.balance - a;
    assert Wf(c, st2);
    assert st2.balance >= 0;
  }
}
