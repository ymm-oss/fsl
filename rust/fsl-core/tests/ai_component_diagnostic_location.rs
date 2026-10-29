// SPDX-License-Identifier: Apache-2.0

use fsl_core::{validate_ai_component, validate_ai_component_with_location};

#[test]
fn located_validation_preserves_every_legacy_error_field() {
    for (body, anchor, message) in [
        (
            "ai_component C { authority { may_execute Missing; } }",
            "may_execute",
            "unknown tool 'Missing' in authority block",
        ),
        (
            "ai_component C { check hard { rule bogus; } }",
            "check hard",
            "unknown ai hard-contract rule 'bogus'",
        ),
        (
            "ai_component C { }",
            "ai_component",
            "ai_component 'C' declares no tools",
        ),
        (
            "ai_component C { fallback { when low require human; when low require human; } }",
            "when low require human; }",
            "duplicate fallback reason 'low'",
        ),
    ] {
        let source = format!(
            "// preceding comment\n    {body}\nstatistical_property Q {{\n target C\n require min_samples >= 0\n}}\n"
        );
        let component = &fsl_syntax::parse_ai_project(&source, "P")
            .unwrap()
            .components[0];
        let legacy = validate_ai_component(component).unwrap_err();
        let (located, loc) = validate_ai_component_with_location(component).unwrap_err();
        assert_eq!(format!("{legacy:?}"), format!("{located:?}"));
        assert_eq!(legacy.to_string(), located.to_string());
        assert_eq!(legacy.message, message);
        let loc = loc.expect("parsed error has authored position");
        let prefix = &source[..source.find(anchor).unwrap()];
        assert_eq!(loc.line, 2);
        assert_eq!(
            usize::try_from(loc.column).unwrap(),
            prefix.rsplit('\n').next().unwrap().chars().count() + 1
        );
    }
}

#[test]
fn absent_constructed_metadata_is_not_fabricated_and_valid_tools_pass() {
    let mut component =
        fsl_syntax::parse_ai_component("ai_component C { authority { may_execute Missing; } }")
            .unwrap();
    component.authority.may_execute[0].loc = None;
    assert!(
        validate_ai_component_with_location(&component)
            .unwrap_err()
            .1
            .is_none()
    );
    let valid = fsl_syntax::parse_ai_component(
        "ai_component C { tool Search { schema S; } authority { } }",
    )
    .unwrap();
    assert!(validate_ai_component(&valid).is_ok());
    assert!(validate_ai_component_with_location(&valid).is_ok());
}
