// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::fs;
use std::io::{self, Read};

fn main() {
    let path = std::env::args().nth(1);
    let source = path.as_ref().map_or_else(
        || {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .expect("read FSL source from stdin");
            source
        },
        |path| fs::read_to_string(path).expect("read FSL source file"),
    );
    let base = path
        .as_ref()
        .and_then(|path| std::path::Path::new(path).parent())
        .unwrap_or_else(|| std::path::Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    match fsl_core::parse_kernel_source(&source, &resolver) {
        Ok(spec) => println!(
            "{}",
            serde_json::to_string(&spec.python_ast()).expect("serialize kernel AST")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
