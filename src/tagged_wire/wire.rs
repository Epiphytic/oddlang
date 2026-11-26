use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub enum Value {
    UInt(u64),
    Bytes(Vec<u8>),
}

pub struct Contract {
    pub(crate) sha256: String,
    pub(crate) max_payload: u32,
    pub(crate) errors: Errors,
    pub(crate) stream: Option<StreamErrors>,
    pub(crate) variants: BTreeMap<u8, Variant>,
}

pub(crate) struct Errors {
    pub short_header: String,
    pub payload_limit: String,
    pub length_mismatch: String,
    pub unknown_tag: String,
    pub truncated: String,
    pub trailing: String,
    pub invalid_value: String,
}

pub(crate) struct StreamErrors {
    pub failed: String,
    pub incomplete: String,
}

pub(crate) struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
}

pub(crate) struct Field {
    pub name: String,
    pub shape: Shape,
}

pub(crate) enum Shape {
    UInt {
        width: u8,
        range: Option<Range>,
    },
    FixedBytes {
        width: u32,
    },
    RestBytes {
        min: u32,
        max: u32,
        error: String,
        sum: Option<Sum>,
    },
}

pub(crate) struct Range {
    pub max: u64,
    pub error: String,
}

pub(crate) struct Sum {
    pub field: usize,
    pub max: u64,
    pub error: String,
}

impl Contract {
    pub fn encode(&self, tag: u8, values: &[Value]) -> Result<Vec<u8>, &str> {
        let variant = self
            .variants
            .get(&tag)
            .ok_or(self.errors.invalid_value.as_str())?;
        if variant.fields.len() != values.len() {
            return Err(&self.errors.invalid_value);
        }
        let mut bytes = vec![0; 5];
        let deferred = variant.fields.last().and_then(|field| match &field.shape {
            Shape::RestBytes { sum: Some(sum), .. } => Some(sum.field),
            _ => None,
        });
        for (index, (field, value)) in variant.fields.iter().zip(values).enumerate() {
            match (&field.shape, value) {
                (Shape::UInt { width, range }, Value::UInt(value)) => {
                    if let Some(range) = range
                        && deferred != Some(index)
                        && *value > range.max
                    {
                        return Err(&range.error);
                    }
                    if *value > width_max(*width) {
                        return Err(&self.errors.invalid_value);
                    }
                    bytes.extend(&value.to_be_bytes()[8 - usize::from(*width)..]);
                }
                (Shape::FixedBytes { width }, Value::Bytes(value))
                    if value.len() == *width as usize =>
                {
                    bytes.extend(value);
                }
                (
                    Shape::RestBytes {
                        min,
                        max,
                        error,
                        sum,
                    },
                    Value::Bytes(value),
                ) => {
                    validate_rest(value.len(), *min, *max, error, sum.as_ref(), values)?;
                    bytes.extend(value);
                }
                _ => return Err(&self.errors.invalid_value),
            }
        }
        let len = bytes.len() - 5;
        if len > self.max_payload as usize {
            return Err(&self.errors.payload_limit);
        }
        bytes[0] = tag;
        bytes[1..5].copy_from_slice(&(len as u32).to_be_bytes());
        Ok(bytes)
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<(u8, Vec<Value>), &str> {
        let [tag, a, b, c, d, payload @ ..] = bytes else {
            return Err(&self.errors.short_header);
        };
        let mut rest = payload;
        let len = u32::from_be_bytes([*a, *b, *c, *d]);
        if len > self.max_payload {
            return Err(&self.errors.payload_limit);
        }
        if len as usize != rest.len() {
            return Err(&self.errors.length_mismatch);
        }
        let variant = self
            .variants
            .get(tag)
            .ok_or(self.errors.unknown_tag.as_str())?;
        let mut values = Vec::with_capacity(variant.fields.len());
        for field in &variant.fields {
            let value = match &field.shape {
                Shape::UInt { width, range } => {
                    let width = usize::from(*width);
                    let (head, tail) = rest
                        .split_at_checked(width)
                        .ok_or(self.errors.truncated.as_str())?;
                    rest = tail;
                    let mut padded = [0; 8];
                    padded[8 - width..].copy_from_slice(head);
                    let value = u64::from_be_bytes(padded);
                    if let Some(range) = range
                        && value > range.max
                    {
                        return Err(&range.error);
                    }
                    Value::UInt(value)
                }
                Shape::FixedBytes { width } => {
                    let (head, tail) = rest
                        .split_at_checked(*width as usize)
                        .ok_or(self.errors.truncated.as_str())?;
                    rest = tail;
                    Value::Bytes(head.to_vec())
                }
                Shape::RestBytes {
                    min,
                    max,
                    error,
                    sum,
                } => {
                    validate_rest(rest.len(), *min, *max, error, sum.as_ref(), &values)?;
                    Value::Bytes(std::mem::take(&mut rest).to_vec())
                }
            };
            values.push(value);
        }
        if !rest.is_empty() {
            return Err(&self.errors.trailing);
        }
        Ok((*tag, values))
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn decoder(&self) -> Option<Decoder<'_>> {
        self.stream.as_ref().map(|errors| Decoder {
            contract: self,
            errors,
            pending: Vec::new(),
            failed: false,
        })
    }

    pub fn variant_name(&self, tag: u8) -> Option<&str> {
        self.variants.get(&tag).map(|variant| variant.name.as_str())
    }

    pub fn field_names(&self, tag: u8) -> Option<impl Iterator<Item = &str>> {
        self.variants
            .get(&tag)
            .map(|variant| variant.fields.iter().map(|field| field.name.as_str()))
    }
}

fn validate_rest<'a>(
    len: usize,
    min: u32,
    max: u32,
    error: &'a str,
    sum: Option<&'a Sum>,
    values: &[Value],
) -> Result<(), &'a str> {
    if len < min as usize || len > max as usize {
        return Err(error);
    }
    if let Some(sum) = sum {
        let Value::UInt(base) = values[sum.field] else {
            unreachable!("a validated schema references an earlier integer field");
        };
        if base
            .checked_add(len as u64)
            .is_none_or(|total| total > sum.max)
        {
            return Err(&sum.error);
        }
    }
    Ok(())
}

pub(crate) fn width_max(width: u8) -> u64 {
    u64::MAX >> (64 - u32::from(width) * 8)
}

/// An incremental decoder with capacity bounded by one validated frame.
pub struct Decoder<'a> {
    contract: &'a Contract,
    errors: &'a StreamErrors,
    pending: Vec<u8>,
    failed: bool,
}

/// A single feed consumes at most the first complete record.
#[derive(Debug, PartialEq, Eq)]
pub struct Feed {
    pub consumed: usize,
    pub record: Option<(u8, Vec<Value>)>,
}

impl<'a> Decoder<'a> {
    pub fn feed(&mut self, input: &[u8]) -> Result<Feed, &'a str> {
        if self.failed {
            return Err(&self.errors.failed);
        }
        let result = self.feed_inner(input);
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
        }
        result
    }

    pub fn finish(&self) -> Result<(), &'a str> {
        if self.failed || !self.pending.is_empty() {
            return Err(&self.errors.incomplete);
        }
        Ok(())
    }

    fn feed_inner(&mut self, input: &[u8]) -> Result<Feed, &'a str> {
        let header_bytes = 5usize.saturating_sub(self.pending.len()).min(input.len());
        self.pending.extend_from_slice(&input[..header_bytes]);
        if self.pending.len() < 5 {
            return Ok(Feed {
                consumed: header_bytes,
                record: None,
            });
        }
        let length = u32::from_be_bytes(self.pending[1..5].try_into().expect("complete header"));
        if length > self.contract.max_payload {
            return Err(&self.contract.errors.payload_limit);
        }
        let total = (length as usize)
            .checked_add(5)
            .ok_or(self.contract.errors.payload_limit.as_str())?;
        let body_bytes = (total - self.pending.len()).min(input.len() - header_bytes);
        let consumed = header_bytes + body_bytes;
        self.pending
            .extend_from_slice(&input[header_bytes..consumed]);
        let record = if self.pending.len() == total {
            let record = self.contract.decode(&self.pending)?;
            self.pending.clear();
            Some(record)
        } else {
            None
        };
        Ok(Feed { consumed, record })
    }
}
