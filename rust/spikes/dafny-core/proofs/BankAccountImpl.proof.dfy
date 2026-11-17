// SPDX-License-Identifier: Apache-2.0
// Written by a model through prove.mjs; accepted only because build.sh's gates pass.

include "../out/BankAccountImpl.gen.dfy"

module BankAccountImplProof refines BankAccountImplSpec {
  lemma InitEstablishes(c: Config)
  {
  }

  lemma settle_Preserves(c: Config, st: State)
  {
  }

  lemma submit_deposit_Preserves(c: Config, st: State, a: int)
  {
  }

  lemma withdraw_cleared_Preserves(c: Config, st: State, a: int)
  {
  }
}
