// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);
pub const OLD: &[u8] = b"complete previous artifact\n";
pub const PM: &str = "examples/pm/cancel_system.fsl";
pub const DOMAIN: &str = "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_owned()
}

pub fn scratch(label: &str) -> PathBuf {
    let dir = root().join(format!(
        "rust/target/atomic-output-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::fs::create_dir(&dir)
        .expect("fresh PID/counter directory; never remove someone else's files");
    std::fs::create_dir(dir.join("out")).unwrap();
    std::fs::create_dir(dir.join("observations")).unwrap();
    dir
}

pub fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 fixture path")
}

pub fn args_with_output(args: &[&str], path: &Path) -> Vec<String> {
    args.iter()
        .copied()
        .chain(["-o", text(path)])
        .map(str::to_owned)
        .collect()
}

// All bytes and names retained, including unexpected temporary files. No masks.
pub fn state(dir: &Path) -> Value {
    let mut entries = serde_json::Map::new();
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        let mut value = if metadata.file_type().is_symlink() {
            let target = if std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
                let bytes = std::fs::read(&path).unwrap();
                json!({"bytes": bytes, "sha256": format!("{:x}", Sha256::digest(&bytes))})
            } else {
                Value::Null
            };
            json!({"symlink": std::fs::read_link(&path).unwrap(), "regular_target": target})
        } else if metadata.is_dir() {
            json!({"directory": state(&path)})
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            json!({"sha256": format!("{:x}", Sha256::digest(&bytes)), "bytes": bytes})
        } else {
            json!({"nonregular": true})
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            value["mode"] = json!(metadata.mode());
            value["inode"] = json!(metadata.ino());
        }
        entries.insert(
            path.file_name().unwrap().to_str().unwrap().to_owned(),
            value,
        );
    }
    Value::Object(entries)
}

// A shell is used only for child-local limits/umask, never interpolated argv.
pub fn run(home: &Path, cwd: &Path, args: &[String], shell: Option<&str>) -> Output {
    let before = state(&home.join("out"));
    let mut command = if let Some(script) = shell {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            script,
            "atomic-output-control",
            env!("CARGO_BIN_EXE_fslc"),
        ]);
        command
    } else {
        Command::new(env!("CARGO_BIN_EXE_fslc"))
    };
    let output = command
        .args(args)
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run existing native CLI");
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let base = home.join("observations").join(format!("run-{id}"));
    std::fs::write(base.with_extension("stdout"), &output.stdout).unwrap();
    std::fs::write(base.with_extension("stderr"), &output.stderr).unwrap();
    std::fs::write(
        base.with_extension("json"),
        serde_json::to_vec_pretty(&json!({
            "binary": env!("CARGO_BIN_EXE_fslc"), "cwd": cwd, "argv": args, "shell": shell,
            "exit": output.status.code(), "status": output.status.to_string(),
            "stdout_json": serde_json::from_slice::<Value>(&output.stdout).ok(),
            "before": before, "after": state(&home.join("out")),
        }))
        .unwrap(),
    )
    .unwrap();
    output
}

pub fn json(output: &Output, status: i32) -> Value {
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("existing JSON envelope")
}

pub fn io(output: &Output) -> Value {
    let value = json(output, 2);
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "io", "{value}");
    value
}

#[derive(Clone, Copy)]
pub enum TestgenIoCase {
    MissingParent,
    FinalNonregular,
}

// Exact matched base/head missing-parent stdout, SHA-256 08f4c4d8…73fd34.
// The version wire key is "fsl", not an invented "fsl_version" alias.
const TESTGEN_MISSING_PARENT_ENVELOPE: &str = r#"{
  "fsl": "1.0",
  "result": "error",
  "message": "No such file or directory (os error 2)",
  "target": "pytest",
  "warnings": [
    {
      "message": "reachable SoldOut not witnessed at depth 3; try --depth >= 4",
      "hint": "try --depth >= 4"
    }
  ]
}
"#;

pub fn testgen_io_packet(
    exit: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    case: TestgenIoCase,
) -> Result<Value, &'static str> {
    if exit != Some(2) {
        return Err("testgen exit must be 2");
    }
    if !stderr.is_empty() {
        return Err("testgen stderr must be empty");
    }
    let value: Value = serde_json::from_slice(stdout).map_err(|_| "testgen stdout must be JSON")?;
    if value.get("kind").is_some() {
        return Err("testgen kind must be absent, including not null");
    }
    let expected = match case {
        TestgenIoCase::MissingParent => TESTGEN_MISSING_PARENT_ENVELOPE.to_owned(),
        TestgenIoCase::FinalNonregular => TESTGEN_MISSING_PARENT_ENVELOPE.replacen(
            "No such file or directory (os error 2)",
            "output target is not a regular file",
            1,
        ),
    };
    if stdout != expected.as_bytes() {
        return Err("testgen complete envelope differs");
    }
    Ok(value)
}

pub fn testgen_io(output: &Output, case: TestgenIoCase) -> Value {
    testgen_io_packet(output.status.code(), &output.stdout, &output.stderr, case)
        .expect("observed testgen-only error envelope")
}

pub fn names(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    paths
}
