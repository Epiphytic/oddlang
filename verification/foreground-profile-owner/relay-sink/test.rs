#[cfg(test)]
mod tests {
    use futures_util::Sink;
    use std::{
        cell::{Cell, RefCell},
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    include!("../../../src/foreground_profile_owner/checked_relay_sink.rs.in");

    struct Recording<'a> {
        frames: &'a RefCell<Vec<u8>>,
        closed: &'a Cell<bool>,
        fail_flush: bool,
    }

    impl Sink<u8> for Recording<'_> {
        type Error = &'static str;

        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, frame: u8) -> Result<(), Self::Error> {
            self.frames.borrow_mut().push(frame);
            Ok(())
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            if std::mem::take(&mut self.get_mut().fail_flush) {
                Poll::Ready(Err("socket flush"))
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.closed.set(true);
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn refuses_invalid_frames_without_poisoning_flush_or_close() {
        let context = &mut Context::from_waker(Waker::noop());
        let frames = RefCell::new(Vec::new());
        let closed = Cell::new(false);
        let stopped = Cell::new(false);
        let inner = Recording {
            frames: &frames,
            closed: &closed,
            fail_flush: true,
        };
        let mut guarded = CheckedRelaySink::new(inner, |frame: &[u8]| {
            if stopped.get() || frame.len() != 1 {
                return Err("frame");
            }
            Ok(frame[0])
        });
        let mut guarded = Pin::new(&mut guarded);
        assert_eq!(guarded.as_mut().poll_ready(context), Poll::Ready(Ok(())));
        assert_eq!(guarded.as_mut().start_send(&b"xy"[..]), Err("frame"));
        assert_eq!(
            guarded.as_mut().poll_flush(context),
            Poll::Ready(Err("socket flush"))
        );
        assert_eq!(guarded.as_mut().poll_flush(context), Poll::Ready(Ok(())));
        assert_eq!(guarded.as_mut().poll_ready(context), Poll::Ready(Ok(())));
        assert_eq!(guarded.as_mut().start_send(&b"x"[..]), Ok(()));
        stopped.set(true);
        assert_eq!(guarded.as_mut().poll_ready(context), Poll::Ready(Ok(())));
        assert_eq!(guarded.as_mut().start_send(&b"x"[..]), Err("frame"));
        assert_eq!(guarded.as_mut().poll_close(context), Poll::Ready(Ok(())));
        assert_eq!(frames.into_inner(), vec![b'x']);
        assert!(closed.get());
    }
}
