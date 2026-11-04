// SPDX-License-Identifier: Apache-2.0
//! Telling a panic in the `z3` bindings from any other panic in this process.

/// The panic text and location of the latest panic raised in the `z3` crate.
static BINDING_PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The `z3` crate's own source, from its versioned directory. Not this workspace's
/// `fsl-solver-z3`, and not a checkout that happens to sit under a directory so named.
fn binding_source(file: &str) -> Option<&str> {
    let start = file
        .match_indices(['/', '\\'])
        .map(|(separator, _)| separator + 1)
        .find(|&start| {
            file[start..].strip_prefix("z3-").is_some_and(|version| {
                version.starts_with(|first: char| first.is_ascii_digit())
                    || version.starts_with("sys-")
            })
        })?;
    Some(&file[start..])
}

pub(crate) fn record(info: &std::panic::PanicHookInfo<'_>) {
    let Some(location) = info.location() else {
        return;
    };
    let Some(source) = binding_source(location.file()) else {
        return;
    };
    let payload = info.payload();
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-text panic payload");
    let record = format!(
        "{text} at {source}:{}:{}",
        location.line(),
        location.column()
    );
    *BINDING_PANIC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(record);
}

/// A caller that caught a solver thread's panic goes on without it, so a later crash in
/// this process is not that panic.
pub(crate) fn forget() {
    *BINDING_PANIC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// A panic in the bindings stops the run before any verdict. The usual cause is Z3
/// returning no term out of memory, but a defect panics the same way and nothing here
/// tells them apart, so this names the panic and does not claim a cause.
fn refusal_message(panic: &str) -> String {
    format!(
        "solver execution stopped [solver-panic]: the z3 bindings panicked, so no verdict \
         was reached: {panic}. Z3 returning no term when memory runs out is the usual cause, \
         but this run did not establish it"
    )
}

/// The refusal for the recorded panic, if the panic that ended the worker was one.
pub(crate) fn take_refusal() -> Option<String> {
    BINDING_PANIC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .map(|panic| refusal_message(&panic))
}

#[cfg(test)]
mod tests {
    use super::{binding_source, refusal_message};

    #[test]
    fn only_the_crates_own_source_counts() {
        let registry = "/home/u/.cargo/registry/src/index.crates.io-1/z3-0.20.2/src/ast/int.rs";
        assert_eq!(binding_source(registry), Some("z3-0.20.2/src/ast/int.rs"));
        assert_eq!(
            binding_source(r"C:\cargo\z3-sys-0.10.0\src\lib.rs"),
            Some(r"z3-sys-0.10.0\src\lib.rs")
        );
        for other in [
            "rust/fsl-solver-z3/src/lib.rs",
            "/work/z3-checkout/rust/fslc/src/main.rs",
            "/rustc/abc/library/core/src/option.rs",
        ] {
            assert_eq!(binding_source(other), None, "{other}");
        }
    }

    #[test]
    fn the_refusal_names_the_panic_and_claims_no_cause() {
        let message = refusal_message("boom at z3-0.20.2/src/ast/int.rs:50:14");
        assert!(message.contains("[solver-panic]"), "{message}");
        assert!(!message.contains("[resource]"), "{message}");
        assert!(message.contains("no verdict"), "{message}");
        assert!(
            message.contains("boom at z3-0.20.2/src/ast/int.rs:50:14"),
            "{message}"
        );
    }
}
