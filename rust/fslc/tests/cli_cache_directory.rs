// SPDX-License-Identifier: Apache-2.0

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

#[test]
fn cache_directory_name_is_not_reused_after_thread_exit() {
    // Each thread owns a claim and drops it on exit, just like separate CLI tests.
    let first = std::thread::spawn(fslc_cli::cache_dir)
        .join()
        .expect("first test thread");
    let second = std::thread::spawn(fslc_cli::cache_dir)
        .join()
        .expect("second test thread");
    assert_ne!(
        first, second,
        "a pending Windows deletion must not be reused"
    );
}
