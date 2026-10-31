// SPDX-License-Identifier: Apache-2.0

/// Retain complete native API observations, not a normalized agreement packet.
/// Debug preserves every field even when consumers have different result types.
pub fn capture(context: &str, stage: &str, value: &impl std::fmt::Debug) {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../.codex/tmp/issue-1622-packets-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("observation directory");
    let filename = format!("{}-{stage}.txt", context.replace(' ', "_"));
    std::fs::write(directory.join(filename), format!("{value:#?}\n"))
        .expect("complete native observation");
}
