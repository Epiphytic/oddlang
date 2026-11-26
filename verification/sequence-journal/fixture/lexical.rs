#[cfg(test)]
mod native_fields {
    use super::*;

    #[test]
    fn cursor_literal_updates_authoritative_state() {
        let mut queue = BoundaryQueue {
            lower: u64::MAX,
            upper: u64::MAX,
            ..Default::default()
        };
        assert_eq!(queue.write(b"x"), Err(super::super::QueueError::Cursor));
        assert_eq!(queue.at_limit(), u64::MAX);
    }

    #[test]
    fn receipt_field_is_the_actual_queue() {
        let mut queue = TelemetryQueue::default();
        assert_eq!(queue.post(0, b"a"), Ok(true));
        assert_eq!(queue.confirmations.len(), 1);
        assert_eq!(queue.ack(1), Ok(()));
        assert_eq!(queue.post(1, b"b"), Ok(true));
        assert_eq!(queue.confirmations.len(), 2);
        assert_eq!(queue.ack(2), Ok(()));
        assert_eq!(queue.post(2, b"c"), Ok(true));
        assert_eq!(queue.confirmations.len(), 2);
        assert_eq!(queue.post(0, b"a"), Err(super::super::QueueError::Stale));
        assert_eq!(queue.post(1, b"b"), Ok(false));
    }
}
