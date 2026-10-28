// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Run independent jobs concurrently, returning results in input order.
pub fn map<T: Send>(count: usize, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new((0..count).map(|_| None).collect::<Vec<_>>());
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(count);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    let result = job(index);
                    results.lock().expect("job results")[index] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("job results")
        .into_iter()
        .map(|result| result.expect("completed job"))
        .collect()
}

#[allow(dead_code)]
pub fn failures(count: usize, job: impl Fn(usize) -> Option<String> + Sync) -> Vec<String> {
    let mut failures: Vec<_> = map(count, job).into_iter().flatten().collect();
    failures.sort();
    failures
}
