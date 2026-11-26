#![deny(warnings)]
mod journal;

#[derive(Debug, PartialEq, Eq)]
enum QueueError {
    Bounds,
    Cursor,
    Gap,
    Conflict,
    Stale,
    Closed,
}

fn main() {
    use journal::TelemetryQueue;
    use QueueError::*;
    let mut queue = TelemetryQueue::default();
    assert_eq!(queue.usable(), 5);
    assert_eq!(queue.oldest_receipt(), None);
    assert_eq!(queue.post(0, b"abc"), Ok(true));
    assert_eq!(queue.post(0, b"abc"), Ok(false));
    assert_eq!(queue.post(0, b"abd"), Err(Conflict));
    assert_eq!(queue.post(1, b"bc"), Err(Stale));
    assert_eq!(queue.post(4, b"x"), Err(Gap));
    assert_eq!(queue.post(7, b"x"), Err(Cursor));
    assert_eq!(queue.view(0, 3).unwrap(), b"abc");
    assert_eq!(queue.ack(1), Ok(()));
    assert_eq!(queue.view(1, 3).unwrap(), b"bc");
    assert_eq!(queue.record_count(), 1);
    assert_eq!(queue.usable(), 3);
    assert_eq!(queue.post(3, b"xy"), Ok(true));
    assert_eq!(queue.usable(), 0);
    assert_eq!(queue.remaining_bytes(), 1);
    assert_eq!(queue.post(5, b"z"), Err(Bounds));
    assert_eq!(queue.ack(3), Ok(()));
    assert_eq!(queue.post(5, b"z"), Ok(true));
    assert_eq!(queue.oldest_receipt(), Some(5));
    assert_eq!(queue.latest_receipt(), Some(6));
    assert_eq!(queue.post(0, b"abc"), Err(Stale));
    assert_eq!(queue.ack(6), Ok(()));
    assert_eq!(queue.probe_rollback(b"k"), Err(Cursor));
    assert_eq!(queue.end(), 6);
    assert_eq!(queue.buffered(), 0);
    assert_eq!(queue.latest_receipt(), Some(6));
    assert_eq!(queue.close(5), Err(Cursor));
    assert_eq!(queue.close(6), Ok(()));
    assert_eq!(queue.close(6), Ok(()));
    assert_eq!(queue.usable(), 0);
    assert_eq!(queue.post(5, b"z"), Ok(false));
    assert_eq!(queue.post(6, b"k"), Err(Closed));
    assert_eq!(queue.view(6, 1).unwrap(), b"");
    assert_eq!(queue.view(0, 0), Err(Cursor));
    assert_eq!(queue.view(6, 0), Err(Bounds));
}
