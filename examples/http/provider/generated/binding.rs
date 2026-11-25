// Generated from the pinned HTTP contract.
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    #[serde(rename = "balance")]
    pub r#balance: Decimal<{0_i64},{9223372036854775807_i64}>,
    #[serde(rename = "id")]
    pub r#id: String,
    #[serde(rename = "owner")]
    pub r#owner: String,
}
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    #[serde(rename = "id")]
    pub r#id: String,
}
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    #[serde(rename = "destination")]
    pub r#destination: Account,
    #[serde(rename = "source")]
    pub r#source: Account,
}
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    #[serde(rename = "amount")]
    pub r#amount: Decimal<{-9223372036854775808_i64},{9223372036854775807_i64}>,
    #[serde(rename = "destination")]
    pub r#destination: String,
    #[serde(rename = "source")]
    pub r#source: String,
}
pub const GET_PATH: &str = "/accounts/{id}";
pub const EMPTY_PATH: &str = "/accounts/";
pub const POST_PATH: &str = "/transfers";
pub const BEHAVIOR: &str = "urn:oddlang:ledger:TransferFunds";
const GET_STATUS:u16=200;
const POST_STATUS:u16=200;
fn error_status(iri: &str) -> u16 { match iri {
"urn:oddlang:http:error:AccountMissing" => 404,
"urn:oddlang:http:error:InvalidRequest" => 400,
"urn:oddlang:http:error:UnsupportedMediaType" => 415,
"urn:oddlang:ledger:error:DestinationMissing" => 404,
"urn:oddlang:ledger:error:DestinationOverflow" => 409,
"urn:oddlang:ledger:error:InsufficientFunds" => 409,
"urn:oddlang:ledger:error:NonPositiveAmount" => 422,
"urn:oddlang:ledger:error:SameAccount" => 422,
"urn:oddlang:ledger:error:SourceMissing" => 404,
_ => 500,
}}
use axum::{
    Json, Router,
    extract::{OriginalUri, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Decimal<const MIN: i64, const MAX: i64>(i64);
impl<const MIN: i64, const MAX: i64> Decimal<MIN, MAX> {
    pub fn new(value: i64) -> Result<Self, &'static str> {
        if (MIN..=MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err("decimal outside range")
        }
    }
    pub fn value(self) -> i64 {
        self.0
    }
}
impl<const MIN: i64, const MAX: i64> serde::Serialize for Decimal<MIN, MAX> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}
impl<'de, const MIN: i64, const MAX: i64> serde::Deserialize<'de> for Decimal<MIN, MAX> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let text = <String as serde::Deserialize>::deserialize(d)?;
        let value: i64 = text.parse().map_err(D::Error::custom)?;
        if value.to_string() != text {
            return Err(D::Error::custom("noncanonical decimal"));
        }
        Self::new(value).map_err(D::Error::custom)
    }
}

pub trait Service: Send + Sync + 'static {
    fn lookup(&self, request: Lookup) -> Result<Account, &'static str>;
    fn transfer(&self, request: Transfer) -> Result<Pair, &'static str>;
}
pub fn router(service: Arc<dyn Service>) -> Router {
    Router::new()
        .route(GET_PATH, get(lookup))
        .route(EMPTY_PATH, get(lookup))
        .route(POST_PATH, post(transfer))
        .with_state(service)
}
const INVALID: &str = "urn:oddlang:http:error:InvalidRequest";
const MEDIA: &str = "urn:oddlang:http:error:UnsupportedMediaType";
const MAX_REQUEST_BODY: usize = 1024 * 1024;
fn response<T: serde::Serialize>(status: u16, result: Result<T, &'static str>) -> Response {
    match result {
        Ok(value) => (StatusCode::from_u16(status).unwrap(), Json(value)).into_response(),
        Err(iri) => (
            StatusCode::from_u16(error_status(iri)).unwrap(),
            Json(serde_json::json!({"error":iri})),
        )
            .into_response(),
    }
}
fn segment(encoded: &str) -> Option<String> {
    let mut decoded = Vec::new();
    let mut bytes = encoded.bytes();
    while let Some(b) = bytes.next() {
        decoded.push(if b == b'%' {
            let hi = (bytes.next()? as char).to_digit(16)?;
            let lo = (bytes.next()? as char).to_digit(16)?;
            (hi * 16 + lo) as u8
        } else {
            b
        });
    }
    let id = String::from_utf8(decoded).ok()?;
    if matches!(id.as_str(), "" | "." | "..") {
        None
    } else {
        Some(id)
    }
}
async fn lookup(
    State(service): State<Arc<dyn Service>>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let id = uri.path().strip_prefix(EMPTY_PATH).and_then(segment);
    response(
        GET_STATUS,
        id.ok_or(INVALID)
            .and_then(|id| service.lookup(Lookup { id })),
    )
}
fn json_media(value: &str) -> bool {
    let mut parts = value.split(';').map(str::trim);
    if !parts
        .next()
        .is_some_and(|s| s.eq_ignore_ascii_case("application/json"))
    {
        return false;
    }
    if let Some(param) = parts.next() {
        let Some((key, value)) = param.split_once('=') else {
            return false;
        };
        if !key.trim().eq_ignore_ascii_case("charset")
            || !value.trim().eq_ignore_ascii_case("utf-8")
        {
            return false;
        }
    }
    parts.next().is_none()
}
async fn transfer(State(service): State<Arc<dyn Service>>, request: Request) -> Response {
    let mut media = request.headers().get_all(header::CONTENT_TYPE).iter();
    if !media
        .next()
        .and_then(|s| s.to_str().ok())
        .is_some_and(json_media)
        || media.next().is_some()
    {
        return response::<Pair>(POST_STATUS, Err(MEDIA));
    }
    let result = match axum::body::to_bytes(request.into_body(), MAX_REQUEST_BODY).await {
        Ok(bytes) if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{') => {
            serde_json::from_slice(&bytes)
                .map_err(|_| INVALID)
                .and_then(|request| service.transfer(request))
        }
        _ => Err(INVALID),
    };
    response(POST_STATUS, result)
}
