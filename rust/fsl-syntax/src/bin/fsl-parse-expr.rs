// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::io::{self, Read};

fn main() {
    let source = std::env::args().nth(1).unwrap_or_else(|| {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .expect("read expression from stdin");
        source
    });
    match fsl_syntax::parse_expr(&source) {
        Ok(expr) => println!(
            "{}",
            serde_json::to_string(&expr.python_ast()).expect("serialize AST")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
