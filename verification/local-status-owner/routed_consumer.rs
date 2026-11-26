use river_gauge::BoundOwner;
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::UnixStream, sync::oneshot};

async fn exchange(path: &std::path::Path, request: &[u8]) -> Vec<u8> {
    let mut connection = UnixStream::connect(path).await.unwrap();
    connection.write_u32(request.len() as u32).await.unwrap();
    connection.write_all(request).await.unwrap();
    connection.shutdown().await.unwrap();
    let mut reply = vec![0; connection.read_u32().await.unwrap() as usize];
    connection.read_exact(&mut reply).await.unwrap();
    reply
}

#[tokio::test]
async fn routed_owner_preserves_status_and_bounds_every_extension() {
    let path = PathBuf::from(std::env::var_os("GAUGE_TEST_DIR").unwrap()).join("routed");
    let owner = BoundOwner::bind(&path).unwrap();
    let (stop, shutdown) = oneshot::channel();
    let routes = Arc::new(AtomicUsize::new(0));
    let calls = routes.clone();
    let task = tokio::spawn(owner.serve_routed_until(async { shutdown.await.unwrap() }, move |body| {
        calls.fetch_add(1, Ordering::Relaxed);
        match body {
            br#"{"action":"sample"}"# => Some(br#"{"sample":19}"#.to_vec()),
            br#"{"action":"overflow"}"# => Some(vec![b'x'; 129]),
            _ => None,
        }
    }));
    let invalid = br#"{"valid":false,"reading":null,"problem":"unrecognized sample"}"#;
    assert_eq!(exchange(&path, br#"{"action":"inspect"}"#).await, br#"{"valid":true,"reading":{"phase":"calibrating"},"problem":null}"#);
    assert_eq!(exchange(&path, br#"{"action":"sample"}"#).await, br#"{"sample":19}"#);
    assert_eq!(exchange(&path, br#"{"action":"overflow"}"#).await, invalid);
    assert_eq!(exchange(&path, b"[]").await, invalid);
    assert_eq!(exchange(&path, b"{").await, invalid);
    assert_eq!(routes.load(Ordering::Relaxed), 2);
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(!path.exists());
}
