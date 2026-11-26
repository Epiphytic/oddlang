#[cfg(test)]
mod observation {
    use super::Scanner;
    use std::{
        cell::{Cell, RefCell},
        io::{self, BufRead, Read},
        rc::Rc,
    };

    const PAYLOAD: [u8; 4] = [7, 42, 11, 215];
    const PARTIAL: usize = 2;

    #[derive(Clone, Default)]
    struct Monitor {
        calls: Rc<Cell<usize>>,
        prefix: Rc<RefCell<Vec<u8>>>,
    }

    struct Meter {
        monitor: Monitor,
        interrupt: bool,
    }

    impl Read for Meter {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let count = self.monitor.calls.get() + 1;
            self.monitor.calls.set(count);
            match (count, self.interrupt) {
                (1, _) => {
                    output[..PAYLOAD.len()].copy_from_slice(&PAYLOAD);
                    Ok(PAYLOAD.len())
                }
                (2, true) => Err(io::ErrorKind::Interrupted.into()),
                _ => {
                    *self.monitor.prefix.borrow_mut() = output[..PAYLOAD.len()].to_vec();
                    Ok(0)
                }
            }
        }
    }

    fn specimen(interrupt: bool) -> (Scanner<Meter>, Monitor) {
        let monitor = Monitor::default();
        let reader = Scanner {
            source: Meter {
                monitor: monitor.clone(),
                interrupt,
            },
            block: zeroize::Zeroizing::new([0; 8]),
            begin: 0,
            end: 0,
        };
        (reader, monitor)
    }

    #[test]
    fn capture_observations() {
        let (mut empty, empty_monitor) = specimen(false);
        let returned = empty.read(&mut []).unwrap();
        let calls_after_empty = empty_monitor.calls.get();
        let after_fill = empty.fill_buf().unwrap().to_vec();
        let calls_after_fill = empty_monitor.calls.get();

        let (mut consumed, consume_monitor) = specimen(false);
        assert_eq!(consumed.fill_buf().unwrap(), &PAYLOAD);
        consumed.consume(PARTIAL);
        let after_partial = consumed.fill_buf().unwrap().to_vec();
        let calls_after_partial = consume_monitor.calls.get();
        consumed.consume(PAYLOAD.len() - PARTIAL);
        let after_refill = consumed.fill_buf().unwrap().to_vec();
        let next_read_prefix = consume_monitor.prefix.borrow();
        let calls_after_refill = consume_monitor.calls.get();

        let (mut failed, error_monitor) = specimen(true);
        assert_eq!(failed.fill_buf().unwrap(), &PAYLOAD);
        failed.consume(PAYLOAD.len());
        let returned_error = matches!(failed.fill_buf(),
            Err(error) if error.kind() == io::ErrorKind::Interrupted);
        let calls_after_error = error_monitor.calls.get();
        let after_retry = failed.fill_buf().unwrap().to_vec();
        let calls_after_retry = error_monitor.calls.get();

        let trace = format!(
            concat!(
                r#"{{"format":"oddlang-buffered-input-trace-v1","#,
                r#""emptyRead":{{"payload":{payload:?},"returned":{returned},"callsAfterEmpty":{empty},"afterFill":{fill:?},"callsAfterFill":{filled}}},"#,
                r#""consume":{{"payload":{payload:?},"requested":{partial},"afterPartial":{after_partial:?},"callsAfterPartial":{partial_calls},"nextReadPrefix":{prefix:?},"afterRefill":{refill:?},"callsAfterRefill":{refill_calls}}},"#,
                r#""refillError":{{"payload":{payload:?},"returnedError":{error},"callsAfterError":{error_calls},"afterRetry":{retry:?},"callsAfterRetry":{retry_calls}}}}}"#
            ),
            payload = PAYLOAD,
            returned = returned,
            empty = calls_after_empty,
            fill = after_fill,
            filled = calls_after_fill,
            partial = PARTIAL,
            after_partial = after_partial,
            partial_calls = calls_after_partial,
            prefix = *next_read_prefix,
            refill = after_refill,
            refill_calls = calls_after_refill,
            error = returned_error,
            error_calls = calls_after_error,
            retry = after_retry,
            retry_calls = calls_after_retry,
        );
        std::fs::write(std::env::var_os("ODD_BUFFER_TRACE").unwrap(), trace).unwrap();
    }
}
