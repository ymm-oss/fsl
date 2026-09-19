// SPDX-License-Identifier: Apache-2.0

//! Where the versioned payload lives, and what this binary's release is called.

use std::path::{Path, PathBuf};

/// The directory holding versioned payloads, matching `install.sh`.
#[must_use]
pub fn data_dir(fsl_data_dir: Option<&Path>, xdg_data_home: Option<&Path>, home: &Path) -> PathBuf {
    if let Some(explicit) = fsl_data_dir {
        return explicit.to_path_buf();
    }
    if let Some(xdg) = xdg_data_home {
        return xdg.join("fsl");
    }
    home.join(".local").join("share").join("fsl")
}

/// The release directory name for a version and the digest of its binary.
///
/// The same `<tag>-<first twelve hex>` shape `install.sh:162` builds, so both
/// routes name the same directory for the same released binary.
#[must_use]
pub fn release_name(version: &str, binary_digest: &str) -> String {
    // `install.sh:162` names the directory `$RELEASE_TAG-${CLI_HASH:0:12}`.
    // That shell expansion takes twelve characters, so taking twelve here is
    // what makes both routes name one directory for one released binary. The
    // digest is in the name at all so two builds of one tag stay apart.
    let short: String = binary_digest.chars().take(12).collect();
    format!("v{version}-{short}")
}
