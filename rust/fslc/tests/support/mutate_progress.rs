// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use serde_json::Value;

/// Validate every raw stderr line, including finite nonnegative seconds and
/// the closed set of private route labels. Return the body and engine.
pub fn progress(output: &Output) -> Vec<(&str, &str)> {
    let stderr = std::str::from_utf8(&output.stderr).expect("UTF-8 stderr");
    assert!(
        !stderr.is_empty(),
        "successful mutate report must emit progress"
    );
    stderr
        .lines()
        .map(|line| {
            assert!(line.starts_with("mutate "), "{line:?}");
            let (body, timing) = line.split_once(" (engine=").expect("engine suffix");
            let (engine, seconds) = timing.split_once(", ").expect("duration separator");
            assert!(
                matches!(engine, "explicit" | "bmc" | "probe" | "none"),
                "{line}"
            );
            let seconds: f64 = seconds
                .strip_suffix("s)")
                .expect("seconds suffix")
                .parse()
                .expect("numeric seconds");
            assert!(seconds.is_finite() && seconds >= 0.0, "{line}");
            (body, engine)
        })
        .collect()
}

pub fn assert_report_progress(output: &Output, value: &Value) {
    let lines = progress(output);
    assert!(matches!(
        lines[0].0,
        "mutate baseline: proved" | "mutate baseline: verified"
    ));
    assert!(matches!(lines[0].1, "explicit" | "bmc"));
    let mutants = value["mutants"].as_array().expect("mutants");
    assert_eq!(
        lines.len(),
        mutants.len() + 1,
        "one line per evaluated record plus baseline"
    );
    let builtin_count = mutants
        .iter()
        .filter(|record| record["source"] == "builtin")
        .count();
    let external_count = mutants.len() - builtin_count;
    let (mut builtin, mut external) = (0, 0);
    for ((body, engine), record) in lines[1..].iter().zip(mutants) {
        let status = record["status"].as_str().expect("status");
        let killer = record["killed_by"]
            .as_str()
            .map_or_else(String::new, |name| format!(" {name}"));
        if record["source"] == "builtin" {
            builtin += 1;
            assert_eq!(
                *body,
                format!(
                    "mutate [{builtin}/{builtin_count}] {} {}: {status}{killer}",
                    record["op"].as_str().expect("op"),
                    record["target"].as_str().expect("target")
                )
            );
        } else {
            external += 1;
            assert!(
                body.starts_with(&format!("mutate external [{external}/{external_count}] ")),
                "{body}"
            );
            assert!(
                body.contains(record["id"].as_str().expect("external id"))
                    || body.contains(record["target"].as_str().expect("external target")),
                "{body}"
            );
            assert!(body.ends_with(&format!(": {status}{killer}")), "{body}");
            if status == "invalid" {
                assert_eq!(*engine, "none", "invalid records never reach the oracle");
            }
        }
    }
}
