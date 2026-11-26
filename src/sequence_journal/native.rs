use sha2::{Digest, Sha256};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    pub byte_capacity: usize,
    pub record_capacity: usize,
    pub append_limit: usize,
    pub cursor_limit: u64,
    pub receipt_capacity: usize,
}

#[derive(Clone)]
pub(super) struct Record {
    start: u64,
    skip: usize,
    bytes: Arc<[u8]>,
}

impl Record {
    fn visible(&self) -> &[u8] {
        &self.bytes[self.skip..]
    }

    fn end(&self) -> u64 {
        self.start + self.visible().len() as u64
    }
}

#[derive(Clone)]
pub(super) struct Receipt {
    start: u64,
    end: u64,
    digest: [u8; 32],
}

pub(super) struct Store {
    bounds: Bounds,
    start: u64,
    end: u64,
    final_cursor: Option<u64>,
    records: VecDeque<Record>,
    receipts: VecDeque<Receipt>,
}

impl Store {
    pub fn new(bounds: Bounds, cursor: u64) -> Self {
        Self {
            bounds,
            start: cursor,
            end: cursor,
            final_cursor: None,
            records: VecDeque::new(),
            receipts: VecDeque::new(),
        }
    }

    pub fn view(&self) -> View<'_> {
        View::new(
            self.bounds,
            self.start,
            self.end,
            self.final_cursor,
            &self.records,
            &self.receipts,
        )
    }

    pub fn into_parts(self) -> StorageParts {
        (
            self.start,
            self.end,
            self.final_cursor,
            self.records,
            self.receipts,
        )
    }

    pub fn append(&mut self, bytes: &[u8]) -> bool {
        if self.final_cursor.is_some()
            || bytes.is_empty()
            || bytes.len() > self.bounds.append_limit
            || bytes.len() > self.view().byte_credit()
            || self.view().record_credit() == 0
        {
            return false;
        }
        let Some(end) = self.view().advance(self.end, bytes.len()) else {
            return false;
        };
        self.records.push_back(Record {
            start: self.end,
            skip: 0,
            bytes: Arc::from(bytes),
        });
        self.end = end;
        true
    }

    pub fn trim(&mut self, cursor: u64) -> bool {
        if !(self.start..=self.end).contains(&cursor) {
            return false;
        }
        while self
            .records
            .front()
            .is_some_and(|record| record.end() <= cursor)
        {
            self.records.pop_front();
        }
        if let Some(record) = self.records.front_mut() {
            let offset = (cursor - record.start) as usize;
            record.skip += offset;
            record.start = cursor;
        }
        self.start = cursor;
        true
    }

    pub fn finish(&mut self, cursor: u64) -> bool {
        if cursor != self.end {
            return false;
        }
        self.final_cursor = Some(cursor);
        true
    }

    pub fn evict_receipt(&mut self) -> bool {
        self.receipts.pop_front().is_some()
    }

    pub fn remember(&mut self, cursor: u64, bytes: &[u8]) -> bool {
        let Some(end) = self.view().advance(cursor, bytes.len()) else {
            return false;
        };
        if bytes.is_empty()
            || end > self.end
            || self.view().receipt_credit() == 0
            || self
                .view()
                .last_receipt_end()
                .is_some_and(|previous| cursor < previous)
        {
            return false;
        }
        self.receipts.push_back(Receipt {
            start: cursor,
            end,
            digest: fingerprint(bytes),
        });
        true
    }
}

pub(super) type StorageParts = (u64, u64, Option<u64>, VecDeque<Record>, VecDeque<Receipt>);

pub(super) struct View<'a> {
    bounds: Bounds,
    start: u64,
    end: u64,
    final_cursor: Option<u64>,
    records: &'a VecDeque<Record>,
    receipts: &'a VecDeque<Receipt>,
}

impl<'a> View<'a> {
    pub fn new(
        bounds: Bounds,
        start: u64,
        end: u64,
        final_cursor: Option<u64>,
        records: &'a VecDeque<Record>,
        receipts: &'a VecDeque<Receipt>,
    ) -> Self {
        Self {
            bounds,
            start,
            end,
            final_cursor,
            records,
            receipts,
        }
    }

    pub fn stage(&self) -> Store {
        Store {
            bounds: self.bounds,
            start: self.start,
            end: self.end,
            final_cursor: self.final_cursor,
            records: self.records.clone(),
            receipts: self.receipts.clone(),
        }
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.end
    }

    pub fn final_cursor(&self) -> Option<u64> {
        self.final_cursor
    }

    pub fn buffered(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn byte_credit(&self) -> usize {
        self.bounds.byte_capacity - self.buffered()
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn record_credit(&self) -> usize {
        self.bounds.record_capacity - self.record_count()
    }

    pub fn receipt_count(&self) -> usize {
        self.receipts.len()
    }

    pub fn receipt_credit(&self) -> usize {
        self.bounds.receipt_capacity - self.receipt_count()
    }

    pub fn front_receipt_end(&self) -> Option<u64> {
        self.receipts.front().map(|receipt| receipt.end)
    }

    pub fn last_receipt_start(&self) -> Option<u64> {
        self.receipts.back().map(|receipt| receipt.start)
    }

    pub fn last_receipt_end(&self) -> Option<u64> {
        self.receipts.back().map(|receipt| receipt.end)
    }

    pub fn front_receipt_evictable(&self) -> bool {
        self.front_receipt_end()
            .is_some_and(|end| end <= self.start)
    }

    pub fn advance(&self, cursor: u64, size: usize) -> Option<u64> {
        let size = u64::try_from(size).ok()?;
        cursor
            .checked_add(size)
            .filter(|end| *end <= self.bounds.cursor_limit)
    }

    pub fn slice(&self, cursor: u64, limit: usize) -> Vec<u8> {
        let cursor = cursor.clamp(self.start, self.end);
        let available = ((self.end - cursor) as usize).min(limit);
        let mut result = Vec::with_capacity(available);
        for record in self.records {
            if result.len() == available {
                break;
            }
            if record.end() <= cursor {
                continue;
            }
            let skip = cursor.saturating_sub(record.start) as usize;
            let count = (record.visible().len() - skip).min(available - result.len());
            result.extend_from_slice(&record.visible()[skip..skip + count]);
        }
        result
    }

    pub fn has_receipt(&self, cursor: u64) -> bool {
        self.receipts.iter().any(|receipt| receipt.start == cursor)
    }

    pub fn same_receipt(&self, cursor: u64, end: Option<u64>, bytes: &[u8]) -> bool {
        self.receipts
            .iter()
            .find(|receipt| receipt.start == cursor)
            .is_some_and(|receipt| Some(receipt.end) == end && receipt.digest == fingerprint(bytes))
    }
}

fn fingerprint(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
