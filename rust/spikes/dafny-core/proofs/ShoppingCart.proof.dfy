// SPDX-License-Identifier: Apache-2.0
// Proof module for the generated ShoppingCartSpec (the part an AI writes).
include "../out/ShoppingCart.gen.dfy"

module ShoppingCartProof refines ShoppingCartSpec {
  lemma InitEstablishes(c: Config) {}
  lemma add_to_cart_Preserves(c: Config, st: State, u: int, i: int) {}
  lemma checkout_Preserves(c: Config, st: State, u: int) {}
  lemma remove_from_cart_Preserves(c: Config, st: State, u: int) {}
}
