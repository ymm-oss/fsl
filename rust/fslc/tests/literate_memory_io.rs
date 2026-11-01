// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
//! A1–A3/A8 draft. Fixed fixture anchors, real child PIDs, no external oracle directory.
#![cfg(unix)]
#[path = "support/literate_memory.rs"]
mod support;
use std::io::Write;
use std::os::unix::{fs::OpenOptionsExt, process::ExitStatusExt};
use std::process::Stdio;
use support::child::{ReapedChild, wait_for_output};
use support::*;

fn readonly(command: &'static str) {
    let s = Scratch::new();
    let source = fixture("literate_toggle.md");
    s.write("src/doc.md", &source);
    s.write("src/.doc.literate-abandoned.fsl", "abandoned\0bytes\n");
    let names = s.names();
    let writable = s.run(&args(command, "src/doc.md"));
    raw("writable", &writable);
    normal(command, &writable);
    let lock = ReadOnly::new(s.0.join("src"));
    let readonly = s.run(&args(command, "src/doc.md"));
    raw("readonly", &readonly);
    normal(command, &readonly);
    assert_eq!(s.names(), names);
    assert_eq!(
        std::fs::read(s.0.join("src/doc.md")).unwrap(),
        source.as_bytes()
    );
    assert_eq!(
        std::fs::read(s.0.join("src/.doc.literate-abandoned.fsl")).unwrap(),
        b"abandoned\0bytes\n"
    );
    drop(lock);
    // One Toggle/command anchor for both, with each invocation's own PID.
    compare_packets(vec![
        (case("toggle", command, "src/doc.md"), writable),
        (case("toggle", command, "src/doc.md"), readonly),
    ]);
}
#[test]
fn readonly_check_requires_real_denial() {
    readonly("check");
}
#[test]
fn readonly_verify_requires_real_denial() {
    readonly("verify");
}
#[test]
fn readonly_scenarios_requires_real_denial() {
    readonly("scenarios");
}

fn fifo(path: &std::path::Path) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}
fn attach(path: &std::path::Path, child: &mut ReapedChild) -> std::fs::File {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(writer) => return writer, // Only the real CLI is a reader; cleanup cannot fake it.
            Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {}
            Err(error) => panic!("dependency rendezvous: {error}"),
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "CLI exited before dependency read"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "dependency read handshake timed out"
        );
        std::thread::yield_now();
    }
}
#[allow(clippy::too_many_lines)]
fn held_lifecycle(mode: &str) {
    let mut packets = Vec::new();
    let s = Scratch::new();
    let doc = markdown(COMPOSE);
    s.write("src/doc.md", &doc);
    s.write("src/.doc.literate-abandoned.fsl", "do not scavenge\0\n");
    fifo(&s.0.join("src/dep.fsl"));
    let names = s.names();
    let mut children = Vec::new();
    let mut writers = Vec::new();
    let mut during = Vec::new();
    for index in 0..if mode == "concurrent" { 6 } else { 3 } {
        let command = ["check", "verify", "scenarios"][index % 3];
        let mut child = ReapedChild::new(
            s.command(&args(command, "src/doc.md"))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let pid = child.child.id();
        let mut writer = attach(&s.0.join("src/dep.fsl"), &mut child);
        writer
            .write_all(if mode == "reject" {
                b"spec S {"
            } else {
                LEAF.as_bytes()
            })
            .unwrap();
        during.push(s.names()); // EOF withheld on this inode until all children are held.
        let next = s.0.join("replacement");
        fifo(&next);
        std::fs::rename(next, s.0.join("src/dep.fsl")).unwrap();
        children.push((command, pid, child));
        writers.push(writer);
    }
    during.push(s.names());
    let expected_doc = if mode == "captured" {
        markdown(LEAF)
    } else {
        doc.clone()
    };
    if mode == "captured" {
        s.write("src/doc.md", &expected_doc);
    }
    s.write(
        "replacement",
        if mode == "reject" { "spec S {" } else { LEAF },
    );
    std::fs::rename(s.0.join("replacement"), s.0.join("src/dep.fsl")).unwrap();
    if mode == "kill" {
        for (command, pid, child) in &mut children {
            let (kill_error, status) = child.terminate_and_reap_with(std::process::Child::kill);
            let status = status.expect("unconditional reap even if kill failed");
            eprintln!(
                "1420 terminal {command}: pid={pid}; kill_error={kill_error:?}; status={status:?}"
            );
            assert!(kill_error.is_none());
            assert_eq!(status.signal(), Some(libc::SIGKILL));
        }
    }
    drop(writers);
    if mode != "kill" {
        for (command, pid, mut child) in children {
            let output = Observed {
                pid,
                output: wait_for_output(&mut child),
            };
            raw(mode, &output);
            if mode == "reject" {
                assert_eq!(output.status.code(), Some(2));
                assert_eq!(json(&output)["result"], "error");
            } else {
                normal(command, &output);
            }
            if mode == "captured" {
                assert_eq!(json(&output)["spec"], "C", "must consume captured C, not S");
            }
            packets.push((
                case(
                    if mode == "reject" {
                        "bad-dependency"
                    } else {
                        "compose"
                    },
                    command,
                    "src/doc.md",
                ),
                output,
            ));
        }
    }
    for observed in during {
        assert_eq!(observed, names, "new source entry during {mode}");
    }
    assert_eq!(s.names(), names, "new source entry after {mode}");
    assert_eq!(
        std::fs::read(s.0.join("src/doc.md")).unwrap(),
        expected_doc.as_bytes()
    );
    assert_eq!(
        std::fs::read(s.0.join("src/.doc.literate-abandoned.fsl")).unwrap(),
        b"do not scavenge\0\n"
    );
    compare_packets(packets);
}
#[test]
fn no_sibling_during_or_after_normal() {
    held_lifecycle("normal");
}
#[test]
fn no_sibling_during_or_after_rejection() {
    held_lifecycle("reject");
}
#[test]
fn no_sibling_after_abrupt_death() {
    held_lifecycle("kill");
}
#[test]
fn six_concurrent_commands_never_materialize() {
    held_lifecycle("concurrent");
}
#[test]
fn captured_markdown_survives_path_replacement() {
    held_lifecycle("captured");
}
