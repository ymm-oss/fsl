// SPDX-License-Identifier: Apache-2.0

//! Deadline-bound LSP test receiver. Only the helper thread blocks on the pipe;
//! the caller retains partial bytes and any subsequent buffered frames.

use std::io::Read;
use std::process::ChildStdout;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use serde_json::Value;

pub struct LspReceiver {
    chunks: Receiver<Result<Vec<u8>, String>>,
    buffered: Vec<u8>,
}

impl LspReceiver {
    pub fn new(mut stdout: ChildStdout) -> Self {
        let (send, chunks) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let mut bytes = [0; 4096];
                let result = match stdout.read(&mut bytes) {
                    Ok(0) => return,
                    Ok(length) => Ok(bytes[..length].to_vec()),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => Err(error.to_string()),
                };
                let failed = result.is_err();
                if send.send(result).is_err() || failed {
                    return;
                }
            }
        });
        Self {
            chunks,
            buffered: Vec::new(),
        }
    }

    pub fn recv(&mut self, deadline: Instant) -> Result<Value, String> {
        loop {
            if let Some(header_end) = self.buffered.windows(4).position(|s| s == b"\r\n\r\n") {
                let header = std::str::from_utf8(&self.buffered[..header_end])
                    .map_err(|error| error.to_string())?;
                let content_length = header
                    .split("\r\n")
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .ok_or("missing content length header")?
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| error.to_string())?;
                let body_start = header_end + 4;
                if self.buffered.len() - body_start >= content_length {
                    let frame_end = body_start + content_length;
                    let value = serde_json::from_slice(&self.buffered[body_start..frame_end])
                        .map_err(|error| error.to_string())?;
                    self.buffered.drain(..frame_end);
                    return Ok(value);
                }
            }
            if Instant::now() >= deadline {
                return Err(self.failure("LSP receive deadline expired"));
            }
            match self
                .chunks
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(Ok(bytes)) => self.buffered.extend(bytes),
                Ok(Err(error)) => return Err(self.failure(&error)),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(self.failure("LSP receive deadline expired"));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.failure("fslc-lsp closed its output"));
                }
            }
        }
    }

    fn failure(&self, reason: &str) -> String {
        format!(
            "{reason}; partial output: {}",
            String::from_utf8_lossy(&self.buffered)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_buffered_frame_is_returned_after_the_deadline() {
        let (_send, chunks) = mpsc::channel();
        let mut receiver = LspReceiver {
            chunks,
            buffered: b"Content-Length: 2\r\n\r\n{}".to_vec(),
        };
        let deadline = Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .expect("past deadline");
        assert_eq!(receiver.recv(deadline).unwrap(), serde_json::json!({}));
        assert!(receiver.buffered.is_empty(), "returned frame is consumed");
    }
}
