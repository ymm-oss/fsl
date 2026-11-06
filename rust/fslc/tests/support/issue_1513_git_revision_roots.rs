// SPDX-License-Identifier: Apache-2.0

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Separates scratch names built in one test binary (#1712): the process id is
/// shared, and the clock can read the same value on two threads (macOS reports
/// microseconds), so two tests with the same label could build the same path.
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub const BODY: &str = "spec S { state { done: Bool = false } action finish() \"REQ-PROBE-001: 日本語😀\" { done = true } invariant Typed { true } }\n";
pub const ANNOTATION: &str = "@requirement(\"REQ-PROBE-001\", \"日本語😀\")";
pub const MAPPING: &str =
    "refinement M { impl S abs S map done = done action finish() -> finish() }\n";

pub fn source(variant: &str) -> String {
    let body = BODY.replace(
        "action finish() \"REQ-PROBE-001: 日本語😀\"",
        &format!("{ANNOTATION} action finish()"),
    );
    match variant {
        "C" => body,
        "C5" => format!("fsl 5\n{body}"),
        "W" => format!("fsl 4\n{BODY}"),
        "N" => format!("fsl 5\n{BODY}"),
        _ => panic!("unknown fixture {variant}"),
    }
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct Repo {
    pub path: PathBuf,
    evidence: Option<PathBuf>,
    sequence: Cell<usize>,
}

impl Repo {
    pub fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "fslc-1513-{}-{label}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let evidence = std::env::var_os("FSL_1513_EVIDENCE").map(|p| PathBuf::from(p).join(label));
        if let Some(p) = &evidence {
            std::fs::create_dir_all(p).unwrap();
        }
        let repo = Self {
            path,
            evidence,
            sequence: Cell::new(0),
        };
        repo.save(&json!({"binary":env!("CARGO_BIN_EXE_fslc"),"binary_sha256":hash(&std::fs::read(env!("CARGO_BIN_EXE_fslc")).unwrap()),"cwd":repo.path}));
        repo.git(&["init", "-q"]);
        repo
    }

    pub fn save(&self, value: &Value) {
        if let Some(p) = &self.evidence {
            let n = self.sequence.get();
            self.sequence.set(n + 1);
            std::fs::write(
                p.join(format!("{n:04}.json")),
                serde_json::to_vec_pretty(value).unwrap(),
            )
            .unwrap();
        }
    }

    pub fn write(&self, name: &str, source: &str) {
        std::fs::write(self.path.join(name), source).unwrap();
        self.save(&json!({"fixture":name,"bytes":source.as_bytes(),"source":source,"sha256":hash(source.as_bytes())}));
    }

    fn invoke(&self, executable: &str, args: &[&str]) -> Output {
        let settings = [
            ("FSLC_UNSTABLE", "fsl5"),
            ("FSLC_CACHE", "off"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_AUTHOR_NAME", "Issue 1513"),
            ("GIT_AUTHOR_EMAIL", "1513@example.invalid"),
            ("GIT_COMMITTER_NAME", "Issue 1513"),
            ("GIT_COMMITTER_EMAIL", "1513@example.invalid"),
            ("GIT_AUTHOR_DATE", "2026-10-31T00:00:00Z"),
            ("GIT_COMMITTER_DATE", "2026-10-31T00:00:00Z"),
        ];
        let output = Command::new(executable)
            .args(args)
            .current_dir(&self.path)
            .envs(settings)
            .output()
            .unwrap();
        let inherited = [
            "RUSTUP_TOOLCHAIN",
            "RUSTC",
            "RUSTDOC",
            "CARGO_BUILD_JOBS",
            "RUST_TEST_THREADS",
            "CARGO_TARGET_DIR",
            "TMPDIR",
            "PATH",
        ]
        .map(|k| (k, std::env::var(k).ok()));
        self.save(&json!({"executable":executable,"argv":args,"cwd":self.path,"env_overrides":settings,"inherited_build_env":inherited,"exit":output.status.code(),"stdout":output.stdout,"stderr":output.stderr}));
        output
    }

    pub fn cli(&self, args: &str) -> Output {
        self.invoke(
            env!("CARGO_BIN_EXE_fslc"),
            &args.split_whitespace().collect::<Vec<_>>(),
        )
    }

    pub fn git(&self, args: &[&str]) -> String {
        let mut full = vec![
            "-c",
            "user.name=Issue 1513",
            "-c",
            "user.email=1513@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ];
        full.extend_from_slice(args);
        let out = self.invoke("git", &full);
        assert!(out.status.success(), "Git setup {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().into()
    }

    pub fn commit(&self, label: &str) -> String {
        self.git(&["add", "--all", "--", "*.fsl"]);
        self.git(&["commit", "-qm", label]);
        self.git(&["tag", label]);
        self.git(&["rev-parse", label])
    }

    pub fn record(&self, file: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.path.join(file)).unwrap()).unwrap()
    }

    pub fn write_record(&self, file: &str, record: &Value) {
        self.write(file, &serde_json::to_string_pretty(record).unwrap());
    }

    pub fn successful(&self, command: &str) -> Value {
        let out = self.cli(command);
        assert_eq!(out.status.code(), Some(0), "setup {command}: {out:?}");
        assert!(out.stderr.is_empty(), "setup stderr {command}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        if self.evidence.is_none() {
            std::fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

// Literal authored oracle: byte offsets differ from Unicode-aware columns.
pub fn finding(file: &str, next: bool) -> Value {
    json!({"code":"legacy_string_metadata","kind":"legacy_string_metadata","taxonomy":"deprecated","severity":if next {"error"} else {"warning"},"edition":if next {"next"} else {"current"},"message":"legacy string metadata is deprecated; use a typed annotation","loc":{"file":file,"line":2,"column":55,"end_line":2,"end_column":76},"canonical_replacement":ANNOTATION,"machine_applicable":true,"edits":[{"start":44,"end":44,"replacement":format!("{ANNOTATION}\n{}"," ".repeat(38))},{"start":60,"end":90,"replacement":""}]})
}
pub fn refusal(file: &str) -> Value {
    json!({"fsl":"1.0","result":"error","kind":"unsupported_in_edition","edition":"next","fsl_edition":5,"findings":[finding(file,true)]})
}
pub fn error(kind: &str, message: &str) -> Value {
    json!({"fsl":"1.0","result":"error","kind":kind,"message":message})
}
pub fn finish(mut value: Value, variant: &str, warnings: Vec<Value>) -> Value {
    if variant == "W" {
        value["fsl_edition"] = json!(4);
    }
    if matches!(variant, "N" | "C5") {
        value["edition"] = json!("next");
        value["fsl_edition"] = json!(5);
    }
    if !warnings.is_empty() {
        value["warnings"] = Value::Array(warnings);
    }
    value
}
pub fn relation(old: &str, new: &str) -> Value {
    json!({"fsl":"1.0","result":"semantic_diff","old":{"file":old,"spec":"S"},"new":{"file":new,"spec":"S"},"bounded":{"depth":2,"completeness":"bounded"},"scope":{"old":{"instances":{},"values":{}},"new":{"instances":{},"values":{}},"comparison":"new","applied_to_old":{"instances":{},"values":{}}},"directions":{"new_to_old":{"result":"refines","checked_to_depth":2},"old_to_new":{"result":"refines","checked_to_depth":2}},"summary":["no_semantic_change"],"findings":[],"gate":{"forbidden":[],"violations":[],"passed":true}})
}
pub fn vcs(repo: &Repo, base: &str, head: &str) -> Value {
    json!({"kind":"git","range":format!("{base}..{head}"),"base":{"revision":base,"commit":repo.git(&["rev-parse",base])},"head":{"revision":head,"commit":repo.git(&["rev-parse",head])},"materialization":"git_archive_full_tree"})
}

pub fn git_pair(repo: &Repo, base: &str, head: &str, file: &str, old: &str, new: &str) -> Value {
    let (a, b) = (format!("{base}:{file}"), format!("{head}:{file}"));
    let warnings = [(old, &a), (new, &b)]
        .into_iter()
        .filter(|(v, _)| *v == "W")
        .map(|(_, p)| finding(p, false))
        .collect();
    let mut value = finish(relation(&a, &b), old, warnings);
    value["vcs"] = vcs(repo, base, head);
    value
}

pub fn batch(vcs: Value, comparisons: Vec<Value>, files: &[&str]) -> Value {
    let mut value = json!({"fsl":"1.0","result":"semantic_diff_batch","specs":files,"gate":{"violations":[],"passed":true}});
    value["vcs"] = vcs;
    value["comparisons"] = Value::Array(comparisons);
    value
}

pub fn approval(record: &Value, file: &str, signed_key: Option<&str>) -> Value {
    let commit = record["spec"]["git_commit"].as_str().unwrap();
    let mut value = relation(&format!("{commit}:spec.fsl"), "spec.fsl");
    value["approval"] = json!({"record":file,"baseline_digest":record["spec"]["digest"],"baseline_commit":commit,"signature_status":if signed_key.is_some() {"signed"} else {"unsigned"},"key_id":signed_key});
    finish(value, "C5", vec![])
}
pub fn versions() -> Value {
    json!({"verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},"core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},"solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}})
}

pub fn resign(mut record: Value, key: &SigningKey) -> Value {
    let mut unsigned = record.clone();
    let object = unsigned["signature"].as_object_mut().unwrap();
    object.remove("value");
    unsigned.sort_all_objects();
    let mut payload = b"fslc.approval.v6\0".to_vec();
    payload.extend(serde_json::to_vec(&unsigned).unwrap());
    let signature = key.sign(&payload);
    let public = key.verifying_key();
    public.verify_strict(&payload, &signature).unwrap();
    record["signature"]["value"] =
        json!(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes()));
    record
}

// Type first, complete object keys, ordered arrays: missing != null, Bool != number.
pub fn strict(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|w| strict(v, w)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(v, w)| strict(v, w))
        }
        _ => std::mem::discriminant(a) == std::mem::discriminant(b) && a == b,
    }
}

#[derive(Default)]
pub struct Observations {
    failures: Vec<String>,
    count: usize,
}

impl Observations {
    pub fn output(&mut self, repo: &Repo, id: &str, out: &Output, value: Value, exit: i32) {
        let actual = json!({"exit":out.status.code(),"stdout":serde_json::from_slice::<Value>(&out.stdout).unwrap(),"stderr":out.stderr});
        let mut expected = json!({"exit":exit,"stderr":Vec::<u8>::new()});
        expected["stdout"] = value;
        let agrees = strict(&actual, &expected);
        repo.save(&json!({"case":id,"actual":actual,"expected":expected,"agrees":agrees}));
        self.count += 1;
        println!(
            "CASE {id} agreement={agrees} actual_exit={:?} expected_exit={exit}",
            out.status.code()
        );
        if !agrees {
            self.failures
                .push(format!("{id}: actual={actual} expected={expected}"));
        }
    }

    pub fn finish(self) {
        assert!(
            self.failures.is_empty(),
            "{} / {} disagreements:\n{}",
            self.failures.len(),
            self.count,
            self.failures.join("\n")
        );
    }
}
