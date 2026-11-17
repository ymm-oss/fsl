// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/RepairLoop.gen.dfy"

module RepairLoopProof refines RepairLoopSpec {
  lemma InitEstablishes(c: Config)
  {
  }

  lemma check_fail_Preserves(c: Config, st: State)
  {
  }

  lemma check_pass_Preserves(c: Config, st: State)
  {
  }

  lemma induction_cti_Preserves(c: Config, st: State)
  {
  }

  lemma induction_proved_Preserves(c: Config, st: State)
  {
    assert st.status == Status.Verified;
    assert st.ever_verified;
  }

  lemma repair_Preserves(c: Config, st: State)
  {
    assert st.edits < 3;
    assert 0 <= st.edits + 1 <= 3;
  }

  lemma verify_ok_Preserves(c: Config, st: State)
  {
  }

  lemma verify_reachfail_Preserves(c: Config, st: State)
  {
  }

  lemma verify_violated_Preserves(c: Config, st: State)
  {
  }
}
