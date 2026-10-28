// SPDX-License-Identifier: Apache-2.0

//! Phase-I-owned receiver adapter. Phase T preserves the old blocking read;
//! Phase I must honor the absolute deadline, preserve buffered frames, report
//! partial output on expiry, and route stdio.rs's receive through this adapter.

use std::io::{BufRead, BufReader, Read};
use std::process::ChildStdout;
use std::time::Instant;

use serde_json::Value;

pub struct LspReceiver {
    stdout: BufReader<ChildStdout>,
}

impl LspReceiver {
    pub fn new(stdout: ChildStdout) -> Self {
        Self {
            stdout: BufReader::new(stdout),
        }
    }

    pub fn recv(&mut self, _deadline: Instant) -> Result<Value, String> {
        let mut content_length = None;
        loop {
            let mut header = String::new();
            let read = self
                .stdout
                .read_line(&mut header)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("fslc-lsp closed its output".to_owned());
            }
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length: ") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|error| error.to_string())?,
                );
            }
        }
        let mut body = vec![0; content_length.ok_or("missing content length header")?];
        self.stdout
            .read_exact(&mut body)
            .map_err(|error| error.to_string())?;
        serde_json::from_slice(&body).map_err(|error| error.to_string())
    }
}
