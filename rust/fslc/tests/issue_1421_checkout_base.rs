// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1421: a v5/v6 record's `path_base` is relative to the Git root of the spec being
//! checked. Another checkout's documents and inputs are its own, so tampering them
//! must show, and nothing may read the checkout that created the record.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SPEC: &str = "spec.fsl";
const DOCUMENT: &str = "requirements.md";
const GLOSSARY: &str = "glossary.json";

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false"])
        .args(["-c", "user.name=Approval Test"])
        .args(["-c", "user.email=approval-test@example.invalid"])
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {arguments:?}: {output:?}");
}

fn run(cwd: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(cwd)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc")
}

fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!("expected JSON: {error}: {output:?}");
    })
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 test path")
}

/// `home/A` holds a tracked spec, document and glossary and created the record;
/// `home/rec.json` and the keys stay outside every checkout.
struct Fixture {
    home: PathBuf,
    public: PathBuf,
    signed: bool,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

impl Fixture {
    fn new(signed: bool) -> Self {
        Self::in_subdirectory(signed, "")
    }

    /// Create the record with the spec, document and glossary in `subdirectory`.
    fn in_subdirectory(signed: bool, subdirectory: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let home = std::env::temp_dir().join(format!(
            "fslc-1421-base-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&home).expect("create home");
        let home = home.canonicalize().expect("physical home");
        let a = home.join("A");
        let creation = a.join(subdirectory);
        std::fs::create_dir_all(&creation).expect("create creation directory");
        git(&a, &["init", "-q"]);
        git(&a, &["config", "core.autocrlf", "false"]);
        let source = std::fs::read_to_string(repository_file("tests/fixtures/approval.fsl"))
            .expect("read fixture");
        std::fs::write(creation.join(SPEC), source.replace("\r\n", "\n")).expect("write spec");
        std::fs::write(
            creation.join(GLOSSARY),
            json!({"schema":"fslc.document-glossary.v1","locale":"ja",
                "labels":{"action:approve":"承認する"}})
            .to_string(),
        )
        .expect("write glossary");
        let generated = run(
            &creation,
            &[
                "document",
                "generate",
                SPEC,
                "--lang",
                "ja",
                "--glossary",
                GLOSSARY,
                "-o",
                DOCUMENT,
            ],
        );
        assert_eq!(generated.status.code(), Some(0), "{generated:?}");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-qm", "reviewed baseline"]);
        let signing = SigningKey::from_bytes(&[7; 32]);
        let private = home.join("private.pem");
        let public = home.join("public.pem");
        #[allow(clippy::default_trait_access)]
        std::fs::write(
            &private,
            signing
                .to_pkcs8_pem(Default::default())
                .expect("encode private key")
                .as_bytes(),
        )
        .expect("write private key");
        #[allow(clippy::default_trait_access)]
        std::fs::write(
            &public,
            signing
                .verifying_key()
                .to_public_key_pem(Default::default())
                .expect("encode public key"),
        )
        .expect("write public key");
        let record = home.join("rec.json");
        let mut arguments = vec![
            "approval",
            "create",
            SPEC,
            "--kind",
            "requirements_document",
            "--artifact",
            DOCUMENT,
            "--approver",
            "alice",
            "--glossary",
            GLOSSARY,
            "-o",
            path_text(&record),
        ];
        if signed {
            arguments.extend(["--signing-key", path_text(&private)]);
        }
        let created = run(&creation, &arguments);
        assert_eq!(created.status.code(), Some(0), "{created:?}");
        Self {
            home,
            public,
            signed,
        }
    }

    fn a(&self) -> PathBuf {
        self.home.join("A")
    }

    fn record_path(&self) -> PathBuf {
        self.home.join("rec.json")
    }

    fn record(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.record_path()).expect("read record"))
            .expect("parse record")
    }

    fn write_record(&self, record: &Value) {
        std::fs::write(
            self.record_path(),
            serde_json::to_vec_pretty(record).expect("encode record"),
        )
        .expect("write record");
    }

    fn clone(&self, name: &str) -> PathBuf {
        git(&self.home, &["clone", "-q", "A", name]);
        self.home.join(name)
    }

    fn worktree(&self, name: &str) -> PathBuf {
        git(&self.a(), &["worktree", "add", "-q", &format!("../{name}")]);
        self.home.join(name)
    }

    fn check(&self, cwd: &Path, spec: &str) -> Output {
        let record = self.record_path();
        let mut arguments = vec!["approval", "check", spec, "--record", path_text(&record)];
        if self.signed {
            arguments.extend(["--trust-key", path_text(&self.public)]);
        }
        run(cwd, &arguments)
    }

    fn assert_status(&self, cwd: &Path, spec: &str, status: &str, reasons: &Value) {
        let output = self.check(cwd, spec);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let checked = value(&output);
        assert_eq!(checked["status"], status, "{checked:#}");
        assert_eq!(&checked["reasons"], reasons, "{checked:#}");
    }

    fn assert_approved(&self, cwd: &Path, spec: &str) {
        self.assert_status(cwd, spec, "approved", &json!([]));
    }

    fn assert_error(&self, cwd: &Path, spec: &str, kind: &str, message: &str) {
        let output = self.check(cwd, spec);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let checked = value(&output);
        assert_eq!(checked["result"], "error", "{checked:#}");
        assert_eq!(checked["kind"], kind, "{checked:#}");
        assert_eq!(checked["message"], message, "{checked:#}");
    }
}

fn append(path: &Path, text: &str) {
    let mut bytes = std::fs::read(path).expect("read file to tamper");
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(path, bytes).expect("tamper file");
}

fn other_checkouts(fixture: &Fixture) -> [PathBuf; 2] {
    [fixture.clone("B"), fixture.worktree("W")]
}

#[test]
fn another_checkout_reads_its_own_document_and_glossary() {
    for signed in [false, true] {
        let fixture = Fixture::new(signed);
        let a_document = std::fs::read(fixture.a().join(DOCUMENT)).unwrap();
        for checkout in other_checkouts(&fixture) {
            fixture.assert_approved(&checkout, "./spec.fsl");

            let document = checkout.join(DOCUMENT);
            let reviewed = std::fs::read(&document).unwrap();
            append(&document, "TAMPERED\n");
            fixture.assert_status(
                &checkout,
                "./spec.fsl",
                "drifted",
                &json!(["artifact_changed"]),
            );
            std::fs::write(&document, reviewed).unwrap();
            fixture.assert_approved(&checkout, "./spec.fsl");

            // The rendering carries the glossary digest, so a parseable change drifts.
            let glossary = checkout.join(GLOSSARY);
            let original = std::fs::read(&glossary).unwrap();
            append(&glossary, "\n");
            fixture.assert_status(
                &checkout,
                "./spec.fsl",
                "drifted",
                &json!(["rendering_changed"]),
            );
            append(&glossary, "TAMPERED\n");
            let output = fixture.check(&checkout, "./spec.fsl");
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            assert_eq!(value(&output)["kind"], "document", "{output:?}");
            std::fs::write(&glossary, original).unwrap();
            fixture.assert_approved(&checkout, "./spec.fsl");
        }
        assert_eq!(
            std::fs::read(fixture.a().join(DOCUMENT)).unwrap(),
            a_document
        );
    }
}

#[test]
fn ledger_and_generate_overlays_read_the_checkout_of_the_spec() {
    let fixture = Fixture::new(false);
    let checkout = fixture.clone("B");
    append(&checkout.join(DOCUMENT), "TAMPERED\n");
    let record = fixture.record_path();
    let ledger = run(
        &checkout,
        &[
            "ledger",
            SPEC,
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--approval",
            path_text(&record),
        ],
    );
    let rendered = String::from_utf8(ledger.stdout).expect("ledger Markdown");
    assert!(rendered.contains("drifted"), "{rendered}");
    assert!(!rendered.contains("approved (unsigned)"), "{rendered}");

    let generated = run(
        &checkout,
        &[
            "document",
            "generate",
            SPEC,
            "--lang",
            "ja",
            "--glossary",
            GLOSSARY,
            "--approval",
            path_text(&record),
        ],
    );
    assert_eq!(generated.status.code(), Some(2), "{generated:?}");
    let refused = value(&generated);
    assert!(refused.get("approval_digest").is_none(), "{refused:#}");
    assert_eq!(
        refused["reasons"],
        json!(["artifact_changed"]),
        "{refused:#}"
    );
}

#[test]
fn nothing_reads_the_creating_checkout() {
    for signed in [false, true] {
        let fixture = Fixture::new(signed);
        let relocated = fixture.home.join("relocated");
        std::fs::rename(fixture.a(), &relocated).expect("relocate checkout");
        fixture.assert_approved(&relocated, "./spec.fsl");
        std::fs::rename(&relocated, fixture.a()).expect("restore checkout");

        let checkout = fixture.clone("B");
        std::fs::remove_dir_all(fixture.a()).expect("delete creating checkout");
        fixture.assert_approved(&checkout, "./spec.fsl");
    }
}

#[test]
fn the_caller_directory_is_ignored() {
    let fixture = Fixture::new(false);
    let checkout = fixture.clone("B");
    let write_decoys = |directory: &Path| {
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(DOCUMENT), "decoy\n").unwrap();
        std::fs::write(directory.join(GLOSSARY), "decoy\n").unwrap();
    };
    let below = checkout.join("below");
    write_decoys(&below);
    fixture.assert_approved(&below, "../spec.fsl");
    let sibling = fixture.home.join("sibling");
    write_decoys(&sibling);
    fixture.assert_approved(&sibling, "../B/spec.fsl");

    // Another repository's root must not stand in for the spec's own.
    let other = fixture.home.join("other");
    write_decoys(&other);
    git(&other, &["init", "-q"]);
    fixture.assert_approved(&other, "../B/spec.fsl");
}

#[test]
fn a_subdirectory_base_is_stored_with_slashes_and_read_from_the_checkout() {
    for signed in [false, true] {
        let fixture = Fixture::in_subdirectory(signed, "review/deep");
        assert_eq!(fixture.record()["path_base"], "review/deep");
        let checkout = fixture.clone("B");
        let spec = "review/deep/spec.fsl";
        fixture.assert_approved(&checkout, spec);
        append(&checkout.join("review/deep").join(DOCUMENT), "TAMPERED\n");
        fixture.assert_status(&checkout, spec, "drifted", &json!(["artifact_changed"]));
    }
}

#[test]
fn creation_refuses_a_document_or_input_outside_the_root() {
    let fixture = Fixture::new(false);
    let a = fixture.a();
    let outside_document = fixture.home.join("outside.md");
    std::fs::copy(a.join(DOCUMENT), &outside_document).unwrap();
    let outside_glossary = fixture.home.join("outside.json");
    std::fs::copy(a.join(GLOSSARY), &outside_glossary).unwrap();
    let inside_absolute = a.join(DOCUMENT);
    let record = fixture.home.join("refused.json");
    let cases: [(&str, &str); 4] = [
        ("../outside.md", GLOSSARY),
        (path_text(&inside_absolute), GLOSSARY),
        (path_text(&outside_document), GLOSSARY),
        (DOCUMENT, "../outside.json"),
    ];
    for (document, glossary) in cases {
        let output = run(
            &a,
            &[
                "approval",
                "create",
                SPEC,
                "--kind",
                "requirements_document",
                "--artifact",
                document,
                "--approver",
                "alice",
                "--glossary",
                glossary,
                "-o",
                path_text(&record),
            ],
        );
        assert_eq!(output.status.code(), Some(2), "{document}: {output:?}");
        let refused = value(&output);
        assert_eq!(refused["kind"], "semantics", "{refused:#}");
        let named = if glossary == GLOSSARY {
            document
        } else {
            glossary
        };
        assert_eq!(
            refused["message"],
            format!("approval document path '{named}' lies outside the repository root")
        );
        assert!(!record.exists());
    }
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_root_is_refused_at_creation_and_at_check() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new(false);
    let a = fixture.a();
    let outside = fixture.home.join("outside.md");
    std::fs::copy(a.join(DOCUMENT), &outside).unwrap();
    symlink(&outside, a.join("linked.md")).unwrap();
    let refused_record = fixture.home.join("refused.json");
    let output = run(
        &a,
        &[
            "approval",
            "create",
            SPEC,
            "--kind",
            "requirements_document",
            "--artifact",
            "linked.md",
            "--approver",
            "alice",
            "--glossary",
            GLOSSARY,
            "-o",
            path_text(&refused_record),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(value(&output)["kind"], "semantics");
    assert!(!refused_record.exists());

    let checkout = fixture.clone("B");
    std::fs::remove_file(checkout.join(DOCUMENT)).unwrap();
    symlink(a.join(DOCUMENT), checkout.join(DOCUMENT)).unwrap();
    fixture.assert_error(
        &checkout,
        "./spec.fsl",
        "semantics",
        "approval document path 'requirements.md' lies outside the repository root",
    );
}

#[cfg(unix)]
#[test]
fn a_document_record_cannot_be_created_from_outside_the_root() {
    let fixture = Fixture::new(false);
    let outside = fixture.home.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(fixture.a(), outside.join("link")).unwrap();
    let refused_record = fixture.home.join("refused.json");
    let output = run(
        &outside,
        &[
            "approval",
            "create",
            "link/spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "link/requirements.md",
            "--approver",
            "alice",
            "--glossary",
            "link/glossary.json",
            "-o",
            path_text(&refused_record),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let refused = value(&output);
    assert_eq!(refused["kind"], "semantics", "{refused:#}");
    assert_eq!(
        refused["message"],
        "a requirements_document approval must be created inside the spec's Git repository"
    );
    assert!(!refused_record.exists());
}

#[test]
fn check_refuses_a_stored_path_that_escapes_the_root() {
    let fixture = Fixture::new(false);
    let checkout = fixture.clone("B");
    let record = fixture.record();
    std::fs::copy(checkout.join(DOCUMENT), fixture.home.join("outside.md")).unwrap();
    for stored in ["../outside.md", "below/../../outside.md"] {
        let mut escaped = record.clone();
        escaped["target"]["path"] = json!(stored);
        fixture.write_record(&escaped);
        fixture.assert_error(
            &checkout,
            "./spec.fsl",
            "semantics",
            &format!("approval document path '{stored}' lies outside the repository root"),
        );
    }
    let mut escaped = record.clone();
    escaped["target"]["inputs"]["glossary"]["path"] = json!("../outside.json");
    fixture.write_record(&escaped);
    fixture.assert_error(
        &checkout,
        "./spec.fsl",
        "semantics",
        "approval document path '../outside.json' lies outside the repository root",
    );
}

#[test]
fn a_tampered_base_fails_in_v5_and_v6() {
    let unsigned = Fixture::new(false);
    let checkout = unsigned.clone("B");
    let record = unsigned.record();
    let mut moved = record.clone();
    moved["path_base"] = json!("elsewhere");
    unsigned.write_record(&moved);
    let output = unsigned.check(&checkout, "./spec.fsl");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(value(&output)["kind"], "io", "{output:?}");

    // A decoy at the tampered base must still meet the record's digests.
    let elsewhere = checkout.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::copy(checkout.join(GLOSSARY), elsewhere.join(GLOSSARY)).unwrap();
    std::fs::write(elsewhere.join(DOCUMENT), "decoy\n").unwrap();
    unsigned.assert_status(
        &checkout,
        "./spec.fsl",
        "drifted",
        &json!(["artifact_changed"]),
    );

    let signed = Fixture::new(true);
    let checkout = signed.clone("B");
    let mut moved = signed.record();
    moved["path_base"] = json!("elsewhere");
    signed.write_record(&moved);
    let output = signed.check(&checkout, "./spec.fsl");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(value(&output)["status"], "signature-invalid");
}

#[test]
fn schema_and_reader_agree_on_the_stored_forms() {
    let fixture = Fixture::new(false);
    let checkout = fixture.clone("B");
    let record = fixture.record();
    let text = std::fs::read_to_string(repository_file(
        "schemas/fslc/approval/approval-record.v5.schema.json",
    ))
    .expect("read schema");
    let schema: Value = serde_json::from_str(&text).expect("schema JSON");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    assert!(validator.is_valid(&record), "{record:#}");

    let refused_by_reader = |changed: &Value| {
        fixture.write_record(changed);
        let output = fixture.check(&checkout, "./spec.fsl");
        let checked = value(&output);
        output.status.code() == Some(2)
            && checked["kind"] == "io"
            && checked["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("approval record requires"))
    };
    let bases = [
        (".", true),
        ("review", true),
        ("review/deep", true),
        ("a:b", false),
        ("ab:c", true),
        ("", false),
        ("/work", false),
        ("C:\\work", false),
        ("C:/work", false),
        ("C:", false),
        ("\\\\server\\share", false),
        ("review\\deep", false),
        ("./review", false),
        ("review/.", false),
        ("review/", false),
        ("review//deep", false),
        ("..", false),
        ("../review", false),
        ("review/../deep", false),
    ];
    for (base, accepted) in bases {
        let mut changed = record.clone();
        changed["path_base"] = json!(base);
        assert_eq!(validator.is_valid(&changed), accepted, "schema: {base}");
        assert_eq!(refused_by_reader(&changed), !accepted, "reader: {base}");
    }
    for stored in [
        "/abs/doc.md",
        "C:\\doc.md",
        "C:/doc.md",
        "\\\\server\\doc.md",
    ] {
        let mut changed = record.clone();
        changed["target"]["path"] = json!(stored);
        assert!(!validator.is_valid(&changed), "schema target: {stored}");
        assert!(refused_by_reader(&changed), "reader target: {stored}");
        let mut changed = record.clone();
        changed["target"]["inputs"]["glossary"]["path"] = json!(stored);
        assert!(!validator.is_valid(&changed), "schema input: {stored}");
        assert!(refused_by_reader(&changed), "reader input: {stored}");
    }
}

#[test]
fn a_solver_record_does_not_depend_on_its_base() {
    let fixture = Fixture::new(false);
    let a = fixture.a();
    let review = a.join("review.md");
    let spec = a.join(SPEC);
    let outside = fixture.home.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let generated = run(
        &outside,
        &[
            "ledger",
            path_text(&spec),
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            path_text(&review),
        ],
    );
    assert_eq!(generated.status.code(), Some(0), "{generated:?}");
    let record = fixture.record_path();
    let created = run(
        &outside,
        &[
            "approval",
            "create",
            path_text(&spec),
            "--kind",
            "ledger",
            "--artifact",
            path_text(&review),
            "--approver",
            "alice",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            path_text(&record),
        ],
    );
    assert_eq!(created.status.code(), Some(0), "{created:?}");
    assert_eq!(fixture.record()["path_base"], ".");
    let checkout = fixture.clone("B");
    fixture.assert_approved(&checkout, "./spec.fsl");
}
