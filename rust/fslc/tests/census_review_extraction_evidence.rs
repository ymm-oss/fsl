// SPDX-License-Identifier: Apache-2.0

//! Runnable extraction evidence alongside the real-rule controls in sibling
//! test files. These are NOT substitutes for the census's actual rules.

#[path = "support/census_legacy.rs"]
#[allow(dead_code)]
mod legacy;
#[path = "support/product_source.rs"]
mod product_source;

#[test]
fn the_real_unix_cfg_shape_is_old_missed_and_new_retained() {
    let source = "#[cfg(any(test, unix))]\nfn product() { display_name(value); }\n\
                  #[cfg(test)]\nfn test_only() { display_name(value); }\n";
    assert_eq!(
        legacy::naming_product(source)
            .join("\n")
            .matches("display_name(")
            .count(),
        0
    );
    assert_eq!(
        product_source::product_source(source)
            .matches("display_name(")
            .count(),
        1
    );
}

#[test]
fn comment_padding_explains_the_old_positive_new_negative_boundary() {
    for name in ["display_name", "unmapped_public_spelling"] {
        let source = format!("fn product() {{ {name}/**/(value); }}");
        let old = legacy::naming_product(&source).join("\n");
        let current = product_source::product_source(&source);
        assert_eq!(old.matches(&format!("{name}(")).count(), 1);
        eprintln!(
            "{name}/**/(: frozen adjacent matches = {}, current adjacent matches = {}",
            old.matches(&format!("{name}(")).count(),
            current.matches(&format!("{name}(")).count()
        );
        assert!(current.contains(name), "the extractor keeps the identifier");
        syn::parse_file(&current).expect("the extracted call remains valid Rust");
        // Do not pin padding as desired behavior: the real naming matcher
        // tolerates it. The sibling census_naming_rule_controls tests, not
        // this extraction observation, must detect both calls.
    }
}

#[test]
fn doc_comment_blanking_explains_the_outcome_look_back_boundary() {
    let source = "#[must_use]\n/// Carries the verification outcome.\n\
                  pub struct DocumentedResult { pub outcome: bool }\n";
    let product = product_source::product_source(source);
    let lines = product.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "#[must_use]");
    assert!(lines[1].trim().is_empty(), "doc comment is blanked");
    assert_eq!(
        lines[2],
        "pub struct DocumentedResult { pub outcome: bool }"
    );
}
