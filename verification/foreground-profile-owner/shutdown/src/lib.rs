#![cfg(test)]
use tokio::{task::JoinHandle, time::{Instant, timeout_at}};
include!("../../../../src/foreground_profile_owner/shutdown.rs.in");

use std::{cell::RefCell, future, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};

struct FlushWatch(Arc<AtomicBool>);
impl Drop for FlushWatch {
    fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
}

#[tokio::test]
async fn failing_meter_flush_still_joins_and_closes_the_receiver() {
    let logged = RefCell::new(Vec::new());
    let joined = Arc::new(AtomicBool::new(false));
    let (ready, active) = tokio::sync::oneshot::channel();
    let (close, closing) = tokio::sync::oneshot::channel();
    let observer = FlushWatch(joined.clone());
    let sender = tokio::spawn(async move {
        let _owned = observer;
        ready.send(()).unwrap();
        closing.await.unwrap();
    });
    active.await.unwrap();

    let passed = stop_provider_stages(
        Instant::now() + Duration::from_secs(1),
        async { logged.borrow_mut().push("flush"); false },
        async { logged.borrow_mut().push("stop reader"); true },
        Some(sender),
        async {
            assert!(!joined.load(Ordering::SeqCst));
            logged.borrow_mut().push("close receiver");
            close.send(()).unwrap();
        },
    ).await;
    assert!(!passed);
    assert!(joined.load(Ordering::SeqCst));
    assert_eq!(*logged.borrow(), ["flush", "stop reader", "close receiver"]);
}

#[tokio::test]
async fn waking_past_the_deadline_does_not_claim_completion() {
    assert!(!stop_provider_stages(
        Instant::now(),
        future::pending(),
        async { true },
        None,
        async {},
    ).await);
    assert!(stop_provider_stages(
        Instant::now() + Duration::from_secs(1),
        async { true },
        async { true },
        None,
        async {},
    ).await);
}
