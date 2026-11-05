// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

#[path = "support/atomic_output_cli.rs"]
#[allow(dead_code)]
mod cli;

use cli::{OLD, io, json, names, root, run, scratch, text};
use std::os::unix::fs::symlink;

#[test]
#[allow(clippy::too_many_lines)]
fn causal_keeps_evidence_then_lifecycle_order_and_completed_prefix() {
    let home = scratch("causal-order");
    let out = home.join("out");
    let evidence = out.join("evidence.json");
    let lifecycle = out.join("lifecycle.json");
    // Exact existing causal_cli.rs::run_observe fixture and flags.
    let args: Vec<String> = vec![
        "causal",
        "observe-expectations",
        "examples/causal/incident_response.fsl",
        "--from-log",
        "examples/causal/evidence/incident-observation-log.jsonl",
        "--mapping",
        "examples/causal/evidence/incident-log-mapping.fsl",
        "--scope",
        "examples/causal/evidence/incident-replay-scope.json",
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
        "--out",
        text(&evidence),
        "--lifecycle-out",
        text(&lifecycle),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let healthy = run(&home, &root(), &args, None);
    let envelope = json(&healthy, 0);
    let mut ordered = Vec::new();
    for expectation in envelope["expectations"].as_array().unwrap() {
        if expectation["derived_from_claim"].is_null() {
            continue;
        }
        let id = expectation["id"]
            .as_str()
            .unwrap()
            .strip_prefix("expectation:")
            .unwrap();
        let evidence_id = format!("OBS_{id}_{}", envelope["model"].as_str().unwrap());
        for stem in ["evidence", "lifecycle"] {
            let path = out.join(format!("{stem}.{evidence_id}.json"));
            ordered.push((path.clone(), std::fs::read(path).unwrap()));
        }
    }
    assert!(
        ordered.len() >= 4,
        "exercise two actual evidence/lifecycle pairs"
    );
    assert_eq!(ordered.len(), names(&out).len());
    let repeated = run(&home, &root(), &args, None);
    json(&repeated, 0);
    assert_eq!(
        healthy.stdout, repeated.stdout,
        "full causal envelope, no masks"
    );
    assert_eq!(healthy.stderr, repeated.stderr);
    for (path, bytes) in &ordered {
        assert_eq!(std::fs::read(path).unwrap(), *bytes);
    }
    for stop in 0..ordered.len() {
        for (path, _) in &ordered {
            std::fs::remove_file(path).unwrap();
            std::fs::write(path, OLD).unwrap();
        }
        let target = home.join("blocked-target");
        std::fs::write(&target, OLD).unwrap();
        let blocked = &ordered[stop].0;
        std::fs::remove_file(blocked).unwrap();
        symlink(&target, blocked).unwrap();
        let before = cli::state(&out);
        let error = io(&run(&home, &root(), &args, None));
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .starts_with(&format!("cannot write {}:", blocked.display())),
            "primary caller error path retained: {error}"
        );
        assert_eq!(std::fs::read_link(blocked).unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), OLD);
        let after = cli::state(&out);
        assert_eq!(
            before.as_object().unwrap().keys().collect::<Vec<_>>(),
            after.as_object().unwrap().keys().collect::<Vec<_>>(),
            "no owned temporary residue"
        );
        for (index, (path, bytes)) in ordered.iter().enumerate() {
            assert_eq!(
                std::fs::read(path).unwrap(),
                if index < stop { bytes.as_slice() } else { OLD },
                "prefix retained, no later writes: {path:?}, stop {stop}"
            );
            if index >= stop {
                assert_eq!(
                    before[path.file_name().unwrap().to_str().unwrap()],
                    after[path.file_name().unwrap().to_str().unwrap()],
                    "untouched suffix inode/mode/link"
                );
            }
        }
    }
    std::fs::remove_file(&ordered.last().unwrap().0).unwrap();
    let repaired = run(&home, &root(), &args, None);
    json(&repaired, 0);
    assert_eq!(healthy.stdout, repaired.stdout);
    assert_eq!(healthy.stderr, repaired.stderr);
    for (path, bytes) in ordered {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
