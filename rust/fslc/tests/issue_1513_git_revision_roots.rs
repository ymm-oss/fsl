// SPDX-License-Identifier: Apache-2.0

#[path = "support/issue_1513_git_revision_roots.rs"]
mod cases;
use cases::*;
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::json;

// Finite packet rows: command => complete oracle; exit. Keep the roster compact.
macro_rules! case {
    ($o:ident, $s:ident; $id:expr => $command:expr => $expected:expr; $exit:expr) => {
        $o.output(&$s, $id, &$s.cli($command), $expected, $exit)
    };
}

#[test]
#[allow(clippy::too_many_lines)]
fn git_revision_root_slots_and_packets() {
    assert!(!strict(&json!(true), &json!(1)));
    assert!(!strict(&json!({}), &json!({"x":null})));
    assert!(!strict(&json!([1, 2]), &json!([2, 1])));
    let literal = source("N");
    assert_eq!(
        &literal.as_bytes()[60..90],
        "\"REQ-PROBE-001: 日本語😀\"".as_bytes()
    );
    let mut observations = Observations::default();
    for (old, new) in [
        ("C", "C"),
        ("W", "C"),
        ("C", "W"),
        ("W", "W"),
        ("N", "C"),
        ("C", "N"),
        ("N", "N"),
        ("W", "N"),
        ("N", "W"),
    ] {
        let s = Repo::new(&format!("git-{old}-{new}"));
        for file in ["a.fsl", "b.fsl"] {
            s.write(file, &source(old));
        }
        s.commit("BASE");
        for file in ["a.fsl", "b.fsl"] {
            s.write(file, &format!("{}// selected head\n", source(new)));
        }
        s.commit("NEXT");
        for batch_mode in [false, true] {
            let command = format!(
                "diff --git BASE..NEXT {} --depth 2",
                if batch_mode { "" } else { "a.fsl" }
            );
            let (expected, exit) = if old == "N" || new == "N" {
                (
                    refusal(if old == "N" {
                        "BASE:a.fsl"
                    } else {
                        "NEXT:a.fsl"
                    }),
                    2,
                )
            } else if batch_mode {
                (
                    batch(
                        vcs(&s, "BASE", "NEXT"),
                        ["a.fsl", "b.fsl"]
                            .map(|f| git_pair(&s, "BASE", "NEXT", f, old, new))
                            .to_vec(),
                        &["a.fsl", "b.fsl"],
                    ),
                    0,
                )
            } else {
                (git_pair(&s, "BASE", "NEXT", "a.fsl", old, new), 0)
            };
            case!(observations, s; &format!("git/{old}/{new}/batch={batch_mode}") => &command => expected; exit);
        }
    }
    let s = Repo::new("git-identical-and-direct");
    s.write("a.fsl", &source("W"));
    s.commit("BASE");
    s.write("a.fsl", "invalid live root; select BASE bytes twice\n");
    case!(observations, s; "git/identical-slot-duplicate-warnings" => "diff --git BASE..BASE a.fsl --depth 2" => git_pair(&s,"BASE","BASE","a.fsl","W","W"); 0);
    case!(observations, s; "git/identical-batch-not-changed" => "diff --git BASE..BASE --depth 2" => error("io","Git range contains no changed .fsl files"); 2);
    case!(observations, s; "git/bad-range" => "diff --git BASE...BASE a.fsl" => error("usage","--git range must be BASE..HEAD"); 2);
    case!(observations, s; "git/missing-path" => "diff --git BASE..BASE missing.fsl --depth 2" => error("io","'missing.fsl' must exist in both revisions for semantic diff"); 2);
    for (old, new) in [("N", "C"), ("C", "N"), ("N", "N"), ("W", "W"), ("C5", "C5")] {
        s.write("old.fsl", &source(old));
        s.write("new.fsl", &source(new));
        let (expected, exit) = if old == "N" || new == "N" {
            (refusal(if old == "N" { "old.fsl" } else { "new.fsl" }), 2)
        } else {
            let warnings = [(old, "old.fsl"), (new, "new.fsl")]
                .into_iter()
                .filter(|(v, _)| *v == "W")
                .map(|(_, p)| finding(p, false))
                .collect();
            (finish(relation("old.fsl", "new.fsl"), old, warnings), 0)
        };
        case!(observations, s; &format!("direct/{old}/{new}") => "diff old.fsl new.fsl --depth 2" => expected; exit);
    }
    // Pair-local refusal, not global batch preflight. Earlier valid pairs may run.
    // spec_loading::run_admitted_roots calls its source consumer only after both
    // admissions; this sibling is otherwise an error inside that consumer.
    let s = Repo::new("git-pair-local");
    s.write("a.fsl", &source("C"));
    s.write("b.fsl", &source("N"));
    s.commit("BASE");
    s.write("a.fsl", &format!("{}// changed\n", source("C")));
    s.write("b.fsl","spec S { state { done: Bool = false } action finish() { done = absent } invariant Typed { true } }\n");
    s.commit("NEXT");
    case!(observations, s; "git/pair-local-refusal-before-failing-sibling" => "diff --git BASE..NEXT --depth 2" => refusal("BASE:b.fsl"); 2);
    s.write("control.fsl", &source("C"));
    let mut sibling_error = error(
        "semantics",
        "invalid model expression: public Kernel cannot type identifier 'absent' at 1:57",
    );
    sibling_error["loc"] = json!({"line":1,"column":57});
    case!(observations, s; "preserved/failing-sibling-without-refusal" => "diff control.fsl b.fsl --depth 2" => sibling_error.clone(); 2);
    let bad = std::fs::read_to_string(s.path.join("b.fsl")).unwrap();
    s.write("b.fsl", &source("W"));
    s.commit("WARN");
    s.write("b.fsl", &bad);
    s.commit("BAD");
    let mut warned_error = finish(sibling_error, "W", vec![finding("WARN:b.fsl", false)]);
    warned_error["old"] = json!({"file":"WARN:b.fsl","spec":null});
    warned_error["new"] = json!({"file":"BAD:b.fsl","spec":null});
    warned_error["vcs"] = vcs(&s, "WARN", "BAD");
    case!(observations, s; "git/fsl4-preserves-comparison-error-and-warning" => "diff --git WARN..BAD b.fsl --depth 2" => warned_error; 2);
    // Historical mapping remains raw; root admission must cover both branches.
    for tracked in [false, true] {
        let s = Repo::new(&format!("git-mapping-{tracked}"));
        s.write("spec.fsl", &source("C"));
        if tracked {
            s.write("map.fsl", MAPPING);
        }
        s.commit("BASE");
        s.write("spec.fsl", &source("N"));
        s.commit("NEXT");
        if !tracked {
            s.write("map.fsl", MAPPING);
        }
        case!(observations, s; &format!("git/mapping-root-refusal/tracked={tracked}") => "diff --git BASE..NEXT spec.fsl --mapping map.fsl --depth 2" => refusal("NEXT:spec.fsl"); 2);
    }
    let s = Repo::new("historical-nonroot-scope");
    s.write("spec.fsl", &source("C"));
    s.write(
        "map.fsl",
        &format!(
            "fsl 5\n{}",
            MAPPING.replace("map done = done", "map done = forall i in 0..1: done")
        ),
    );
    s.write("dep.fsl", &source("W"));
    let compose = "compose Root { use S as s from \"dep.fsl\" invariant Typed { true } }\n";
    s.write("root.fsl", compose);
    s.commit("BASE");
    s.write("spec.fsl", &format!("{}// head\n", source("C")));
    s.write("root.fsl", &format!("{compose}// head\n"));
    s.commit("NEXT");
    s.write("map.fsl", "invalid live homonym\n");
    s.write("dep.fsl", "invalid live dependency homonym\n");
    case!(observations, s; "preserved/historical-mapping-raw-and-selected" => "diff --git BASE..NEXT spec.fsl --mapping map.fsl --depth 2" => git_pair(&s,"BASE","NEXT","spec.fsl","C","C"); 0);
    let mut transitive = git_pair(&s, "BASE", "NEXT", "root.fsl", "C", "C");
    transitive["old"]["spec"] = json!("Root");
    transitive["new"]["spec"] = json!("Root");
    case!(observations, s; "preserved/historical-transitive-warning-scope" => "diff --git BASE..NEXT root.fsl --depth 2" => transitive; 0);
    observations.finish();
}

#[test]
#[allow(clippy::too_many_lines, clippy::default_trait_access)]
fn approval_baseline_roots_and_precedence() {
    let mut observations = Observations::default();
    let s = Repo::new("approval");
    s.git(&["commit", "--allow-empty", "-qm", "empty baseline"]);
    let empty = s.git(&["rev-parse", "HEAD"]);
    s.write("spec.fsl", &source("W"));
    let legacy4 = s.commit("LEGACY4");
    s.successful("ledger spec.fsl --depth 2 -o ledger.md");
    s.successful("approval create spec.fsl --kind ledger --artifact ledger.md --approver edition-test --depth 2 -o record4.json");
    let record4 = s.record("record4.json");
    assert_eq!(record4["schema"], "fslc.approval.v5");
    assert_eq!(record4["spec"]["git_commit"], legacy4);
    assert_eq!(
        record4["spec"]["digest"],
        "sha256:9fc33b5801967f0ea94727f18197891ce95b5e2628f96665fe79028af182a356"
    );
    let key = SigningKey::from_bytes(&[7; 32]);
    s.write(
        "private.pem",
        &key.to_pkcs8_pem(Default::default()).unwrap(),
    );
    s.write(
        "public.pem",
        &key.verifying_key()
            .to_public_key_pem(Default::default())
            .unwrap(),
    );
    let key_id = format!("sha256:{}", hash(key.verifying_key().as_bytes()));
    s.successful("approval create spec.fsl --kind ledger --artifact ledger.md --approver edition-test --depth 2 --signing-key private.pem -o signed4.json");
    let original_signed = s.record("signed4.json");
    assert!(
        strict(&resign(original_signed.clone(), &key), &original_signed),
        "fixture signer reproduces actual CLI signature"
    );
    s.write("spec.fsl", &source("N"));
    let legacy5 = s.commit("LEGACY5");
    assert_eq!(
        s.git(&["show", &format!("{legacy4}:spec.fsl")])[6..],
        s.git(&["show", &format!("{legacy5}:spec.fsl")])[6..]
    );
    let mut record5 = record4.clone();
    record5["spec"]["git_commit"] = json!(legacy5);
    let mut restored = record5.clone();
    restored["spec"]["git_commit"] = json!(legacy4);
    assert!(
        strict(&restored, &record4),
        "only unsigned baseline commit rebound"
    );
    s.write_record("record5.json", &record5);
    let mut direct = refusal("spec.fsl");
    direct["versions"] = versions();
    case!(observations, s; "approval/direct-check-negative-full" => "check spec.fsl" => direct; 2);
    // Actual Ed25519 signature, same proven historical digest, matching trust.
    let mut signed5 = original_signed;
    signed5["spec"]["git_commit"] = json!(legacy5);
    let signed5 = resign(signed5, &key);
    s.write_record("signed5.json", &signed5);
    assert_eq!(signed5["schema"], "fslc.approval.v6");
    assert_eq!(signed5["spec"], record5["spec"]);
    assert_eq!(signed5["signature"]["algorithm"], "ed25519");
    assert_eq!(signed5["signature"]["key_id"], key_id);
    s.write("spec.fsl", &source("C5"));
    s.commit("CANON");
    s.successful("ledger spec.fsl --depth 2 -o ledger.md");
    s.successful("approval create spec.fsl --kind ledger --artifact ledger.md --approver edition-test --depth 2 -o canonical.json");
    let canonical = s.record("canonical.json");
    case!(observations, s; "approval/canonical-baseline" => "approval diff spec.fsl --record canonical.json --depth 2" => approval(&canonical,"canonical.json",None); 0);
    s.successful("approval create spec.fsl --kind ledger --artifact ledger.md --approver edition-test --depth 2 --signing-key private.pem -o signed-canonical.json");
    let signed_canonical = s.record("signed-canonical.json");
    case!(observations, s; "approval/signed-canonical-baseline" => "approval diff spec.fsl --record signed-canonical.json --trust-key public.pem --depth 2" => approval(&signed_canonical,"signed-canonical.json",Some(&key_id)); 0);
    let warning = finding(&format!("{legacy4}:spec.fsl"), false);
    case!(observations, s; "approval/fsl4-baseline-one-warning" => "approval diff spec.fsl --record record4.json --depth 2" => finish(approval(&record4,"record4.json",None),"C5",vec![warning.clone()]); 0);
    case!(observations, s; "approval/fsl5-baseline-refusal" => "approval diff spec.fsl --record record5.json --depth 2" => refusal(&format!("{legacy5}:spec.fsl")); 2);
    case!(observations, s; "approval/signed-fsl5-baseline-refusal" => "approval diff spec.fsl --record signed5.json --trust-key public.pem --depth 2" => refusal(&format!("{legacy5}:spec.fsl")); 2);
    let mut tampered = signed5;
    tampered["approval"]["approver"] = json!("mallory");
    s.write_record("tampered.json", &tampered);
    let invalid = finish(
        json!({"fsl":"1.0","result":"approval_diff","status":"signature-invalid","signature_status":"signature-invalid","key_id":key_id}),
        "C5",
        vec![],
    );
    case!(observations, s; "approval/signature-before-baseline-admission" => "approval diff spec.fsl --record tampered.json --trust-key public.pem --depth 2" => invalid; 1);
    let mut mismatched = record5.clone();
    mismatched["spec"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    s.write_record("mismatch.json", &mismatched);
    case!(observations, s; "approval/digest-before-baseline-admission" => "approval diff spec.fsl --record mismatch.json --depth 2" => finish(error("semantics","approval baseline commit does not match the recorded specification digest"),"C5",vec![]); 2);
    let mut missing = record4.clone();
    missing["spec"]["git_commit"] = json!(empty);
    s.write_record("missing.json", &missing);
    case!(observations, s; "approval/missing-baseline-path" => "approval diff spec.fsl --record missing.json --depth 2" => finish(error("io",&format!("approved spec 'spec.fsl' is missing from baseline {empty}")),"C5",vec![]); 2);
    let mut wrong_path = record5;
    wrong_path["spec"]["path"] = json!("other.fsl");
    s.write_record("wrong-path.json", &wrong_path);
    case!(observations, s; "approval/path-mismatch" => "approval diff spec.fsl --record wrong-path.json --depth 2" => finish(error("semantics","approval record targets 'other.fsl' but current spec is 'spec.fsl'"),"C5",vec![]); 2);
    s.write("spec.fsl", &source("W"));
    let mut live_warning = approval(&record4, "record4.json", None);
    live_warning.as_object_mut().unwrap().remove("edition");
    case!(observations, s; "approval/live-and-baseline-warning-once-each" => "approval diff spec.fsl --record record4.json --depth 2" => finish(live_warning,"W",vec![warning,finding("spec.fsl",false)]); 0);
    s.write("spec.fsl", &source("N"));
    for (id, record, trust) in [
        ("missing", "absent.json", ""),
        ("signature", "tampered.json", "--trust-key public.pem"),
        ("digest", "mismatch.json", ""),
    ] {
        case!(observations, s; &format!("approval/live-refusal-before-{id}") => &format!("approval diff spec.fsl --record {record} {trust} --depth 2") => refusal("spec.fsl"); 2);
    }
    observations.finish();
}
