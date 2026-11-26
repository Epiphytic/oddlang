#[cfg(test)]
mod tests {
    use futures_util::{Sink, SinkExt, sink};
    use std::{
        cell::Cell,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    include!("../../../src/foreground_profile_owner/checked_relay_sink.rs.in");

    #[test]
    fn rejects_oversize_and_stopped_frames_then_flushes_and_closes() {
        let context = &mut Context::from_waker(Waker::noop());
        let seen = Cell::new(0);
        let stopped = Cell::new(false);
        let mut guarded = CheckedRelaySink::new(
            sink::drain().sink_map_err(|_| "socket"),
            |frame: &[u8]| {
                if stopped.get() || frame.len() > 1 {
                    return Err("frame");
                }
                seen.set(seen.get() + 1);
                Ok(frame[0])
            },
        );
        let mut guarded = Pin::new(&mut guarded);
        assert_eq!(guarded.as_mut().poll_ready(context), Poll::Ready(Ok(())));
        assert_eq!(guarded.as_mut().start_send(b"xy"), Err("frame"));
        assert_eq!(guarded.as_mut().poll_flush(context), Poll::Ready(Ok(())));
        assert_eq!(guarded.as_mut().start_send(b"x"), Ok(()));
        stopped.set(true);
        assert_eq!(guarded.as_mut().start_send(b"x"), Err("frame"));
        assert_eq!(guarded.as_mut().poll_close(context), Poll::Ready(Ok(())));
        assert_eq!(seen.get(), 1);
    }
}
