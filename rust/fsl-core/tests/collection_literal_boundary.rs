// SPDX-License-Identifier: Apache-2.0

use fsl_core::{FsResolver, build_model, parse_kernel_source};

#[test]
fn inferred_integer_literals_keep_the_existing_collection_context() {
    for (kind, ty) in [("Set", "Set<K>"), ("Seq", "Seq<K, 2>")] {
        for items in ["", "1"] {
            let literal = format!("{kind} {{ {items} }}");
            for predicate in [format!("s == {literal}"), format!("{literal} == s")] {
                let source = format!(
                    "spec Literal {{ type K = 0..2 state {{ s: {ty}, t: {ty}, c: Bool }} \
                     init {{ s = {kind} {{}} t = {kind} {{}} c = false }} \
                     action go() {{ t = if c then {literal} else s }} \
                     invariant P {{ {predicate} }} }}"
                );
                let kernel = parse_kernel_source(&source, &FsResolver::new("."))
                    .expect("existing literal syntax lowers");
                build_model(kernel).unwrap_or_else(|error| panic!("{source}: {error}"));
            }
        }
    }
}
