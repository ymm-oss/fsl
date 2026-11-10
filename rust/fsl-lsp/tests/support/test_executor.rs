// SPDX-License-Identifier: Apache-2.0

//! Wake-driven test executor with a caller-selected absolute deadline.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

pub(crate) fn block_on_until<F: Future>(future: F, deadline: Instant) -> F::Output {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        assert!(Instant::now() < deadline, "test executor deadline expired");
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                std::thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
            }
        }
    }
}
