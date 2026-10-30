// SPDX-License-Identifier: Apache-2.0

pub fn assert_names(message: &str, names: &[&str]) {
    let words: Vec<_> = message
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .collect();
    for name in names {
        assert!(words.contains(name), "{name}: {message}");
    }
}
