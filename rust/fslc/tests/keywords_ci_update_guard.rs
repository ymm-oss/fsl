// SPDX-License-Identifier: Apache-2.0

//! #1413: golden updates must not turn a CI mismatch into a passing rewrite.
//! The pure helper avoids global environment mutation or touching the golden.
//! `fsl5_keywords.rs` uses the same helper; these detector/control assertions
//! pin the CI rejection and the existing local opt-in behavior.

#[path = "support/golden_update.rs"]
mod golden_update;

use golden_update::golden_update_requested;

#[test]
fn ci_rejects_any_set_keyword_update_variable() {
    // Presence, not truthiness: even CI="" or CI="false" denotes a CI context.
    for ci in ["1", "true", "false", ""] {
        for update in ["1", "0", ""] {
            let result = golden_update_requested(Some(update), Some(ci));
            let error = result.expect_err("CI must reject a set FSLC_SKILLS_KEYWORDS_UPDATE");
            assert!(error.contains("CI"), "the error identifies CI: {error}");
            assert!(
                error.contains("FSLC_SKILLS_KEYWORDS_UPDATE"),
                "the error identifies the forbidden update variable: {error}"
            );
        }
    }
}

#[test]
fn local_update_keeps_its_existing_opt_in_value() {
    assert_eq!(golden_update_requested(Some("1"), None), Ok(true));
    for update in [None, Some(""), Some("0"), Some("true")] {
        assert_eq!(golden_update_requested(update, None), Ok(false));
    }
}

#[test]
fn ci_without_an_update_request_still_compares_the_golden() {
    for ci in ["1", "true", "false", ""] {
        assert_eq!(golden_update_requested(None, Some(ci)), Ok(false));
    }
}

/// The edition-header golden (`edition_header.rs`) goes through the same
/// guard: under `CI` a set `FSLC_UPDATE_EDITION_GOLDEN` fails, naming it,
/// instead of rewriting the golden and passing.
#[test]
fn ci_rejects_a_set_edition_golden_update_variable() {
    const VARIABLE: &str = "FSLC_UPDATE_EDITION_GOLDEN";
    for ci in ["1", "true", "false", ""] {
        for update in ["1", "0", ""] {
            let error =
                golden_update::golden_update_requested_for(VARIABLE, Some(update), Some(ci))
                    .expect_err("CI must reject a set FSLC_UPDATE_EDITION_GOLDEN");
            assert!(error.contains("CI") && error.contains(VARIABLE), "{error}");
        }
        assert_eq!(
            golden_update::golden_update_requested_for(VARIABLE, None, Some(ci)),
            Ok(false)
        );
    }
    assert_eq!(
        golden_update::golden_update_requested_for(VARIABLE, Some("1"), None),
        Ok(true)
    );
}
