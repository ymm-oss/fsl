// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::fs;
use std::io::{self, Read};

fn main() {
    let source = std::env::args().nth(1).map_or_else(
        || {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .expect("read FSL source from stdin");
            source
        },
        |path| fs::read_to_string(path).expect("read FSL source file"),
    );
    match fsl_syntax::parse_surface_document(&source) {
        Ok(document) => println!(
            "{}",
            serde_json::to_string(&document.python_ast()).expect("serialize AST")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
