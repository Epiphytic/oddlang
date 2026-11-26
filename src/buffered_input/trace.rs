//! Classify the portable policies from deliberately discriminating trials.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Trace {
    format: String,
    empty_read: EmptyTrial,
    consume: ConsumeTrial,
    refill_error: ErrorTrial,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmptyTrial {
    payload: Vec<u8>,
    returned: u64,
    calls_after_empty: u64,
    after_fill: Vec<u8>,
    calls_after_fill: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConsumeTrial {
    payload: Vec<u8>,
    requested: usize,
    after_partial: Vec<u8>,
    calls_after_partial: u64,
    next_read_prefix: Vec<u8>,
    after_refill: Vec<u8>,
    calls_after_refill: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ErrorTrial {
    payload: Vec<u8>,
    returned_error: bool,
    calls_after_error: u64,
    after_retry: Vec<u8>,
    calls_after_retry: u64,
}

fn validate_payload(payload: &[u8], capacity: u64) -> Result<()> {
    if payload.len() < 2
        || payload.len() as u64 > capacity
        || payload.contains(&0)
        || payload.iter().collect::<BTreeSet<_>>().len() != payload.len()
    {
        return Err("probe payload must contain distinct nonzero octets and fit the buffer".into());
    }
    Ok(())
}

fn zeros_at_extent(bytes: &[u8], payload: &[u8]) -> bool {
    bytes.len() == payload.len() && bytes.iter().all(|&value| value == 0)
}

impl Trace {
    pub(super) fn infer(&self, capacity: u64) -> Result<Transitions> {
        if self.format != "oddlang-buffered-input-trace-v1" {
            return Err("unexpected buffered-input trace format".into());
        }
        for payload in [
            &self.empty_read.payload,
            &self.consume.payload,
            &self.refill_error.payload,
        ] {
            validate_payload(payload, capacity)?;
        }
        let empty = &self.empty_read;
        if empty.returned != 0 || empty.calls_after_fill != 1 || empty.after_fill != empty.payload {
            return Err("inconsistent empty-read evidence".into());
        }
        let empty_read = match empty.calls_after_empty {
            0 => EmptyRead::ReturnZero,
            1 => EmptyRead::RefillIfExhausted,
            _ => return Err("inconsistent empty-read evidence".into()),
        };

        let consume = &self.consume;
        if consume.requested == 0
            || consume.requested >= consume.payload.len()
            || consume.after_partial != consume.payload[consume.requested..]
            || consume.calls_after_partial != 1
            || !consume.after_refill.is_empty()
            || consume.calls_after_refill != 2
        {
            return Err("inconsistent partial-consume evidence".into());
        }
        let consumed_bytes = if zeros_at_extent(&consume.next_read_prefix, &consume.payload) {
            ConsumedBytes::Erase
        } else if consume.next_read_prefix == consume.payload {
            ConsumedBytes::Retain
        } else {
            return Err("inconsistent retirement evidence".into());
        };

        let failed = &self.refill_error;
        if !failed.returned_error || failed.calls_after_error != 2 {
            return Err("inconsistent refill-error evidence".into());
        }
        let retained_view = match consumed_bytes {
            ConsumedBytes::Erase => zeros_at_extent(&failed.after_retry, &failed.payload),
            ConsumedBytes::Retain => failed.after_retry == failed.payload,
        };
        let refill_error = match failed.calls_after_retry {
            2 if retained_view => RefillError::ResetBeginRetainEnd,
            3 if failed.after_retry.is_empty() => RefillError::DiscardWindow,
            _ => return Err("inconsistent refill-error evidence".into()),
        };
        Ok(Transitions {
            empty_read,
            refill_error,
            consumed_bytes,
            release_storage: Release::Erase,
        })
    }
}
