// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/AuthLockout.gen.dfy"

module AuthLockoutProof refines AuthLockoutSpec {

  lemma InitEstablishes(c: Config)
  {
    var st := Init(c);
    assert |st.accounts| == c.N_UserId;
    assert forall k :: 0 <= k < |st.accounts| ==> st.accounts[k] == Account(0, false);
    assert Wf(c, st);
    assert Inv_LockedMeansMaxAttempts(c, st);
    assert Inv_MaxAttemptsMeansLocked(c, st);
    assert Inv_NoSessionWhileLocked(c, st);
  }

  lemma admin_unlock_Preserves(c: Config, st: State, u: int)
  {
    var st2 := admin_unlock_Apply(c, st, u);
    assert st2.accounts == st.accounts[u := Account(0, false)];
    assert st2.session == st.session;
    assert st2.ever_locked == st.ever_locked;
    assert forall k :: 0 <= k < |st2.accounts| && k != u ==> st2.accounts[k] == st.accounts[k];
    assert Wf(c, st2);
    assert Inv_LockedMeansMaxAttempts(c, st2);
    assert Inv_MaxAttemptsMeansLocked(c, st2);
    assert Inv_NoSessionWhileLocked(c, st2);
    assert !(st2.accounts[u].locked);
  }

  lemma login_fail_Preserves(c: Config, st: State, u: int)
  {
    var st2 := login_fail_Apply(c, st, u);
    assert !st.accounts[u].locked;
    assert st.accounts[u].attempts != 3;
    assert 0 <= st.accounts[u].attempts <= 2;
    assert !st.session[u];
    assert st2.session == st.session;
    if st.accounts[u].attempts == 2 {
      assert st2.accounts == st.accounts[u := Account(3, true)];
    } else {
      assert st2.accounts == st.accounts[u := Account(st.accounts[u].attempts + 1, false)];
    }
    assert |st2.accounts| == |st.accounts|;
    assert |st2.ever_locked| == |st.ever_locked|;
    assert forall k :: 0 <= k < |st2.accounts| && k != u ==> st2.accounts[k] == st.accounts[k];
    assert Wf(c, st2);
    assert Inv_LockedMeansMaxAttempts(c, st2);
    assert Inv_MaxAttemptsMeansLocked(c, st2);
    assert Inv_NoSessionWhileLocked(c, st2);
  }

  lemma login_ok_Preserves(c: Config, st: State, u: int)
  {
    var st2 := login_ok_Apply(c, st, u);
    assert !st.accounts[u].locked;
    assert st2.accounts == st.accounts[u := Account(0, false)];
    assert st2.session == st.session[u := true];
    assert st2.ever_locked == st.ever_locked;
    assert forall k :: 0 <= k < |st2.accounts| && k != u ==> st2.accounts[k] == st.accounts[k];
    assert forall k :: 0 <= k < |st2.session| && k != u ==> st2.session[k] == st.session[k];
    assert Wf(c, st2);
    assert Inv_LockedMeansMaxAttempts(c, st2);
    assert Inv_MaxAttemptsMeansLocked(c, st2);
    assert Inv_NoSessionWhileLocked(c, st2);
  }

  lemma logout_Preserves(c: Config, st: State, u: int)
  {
    var st2 := logout_Apply(c, st, u);
    assert st2.accounts == st.accounts;
    assert st2.ever_locked == st.ever_locked;
    assert st2.session == st.session[u := false];
    assert forall k :: 0 <= k < |st2.session| && k != u ==> st2.session[k] == st.session[k];
    assert Wf(c, st2);
    assert Inv_LockedMeansMaxAttempts(c, st2);
    assert Inv_MaxAttemptsMeansLocked(c, st2);
    assert Inv_NoSessionWhileLocked(c, st2);
  }
}
