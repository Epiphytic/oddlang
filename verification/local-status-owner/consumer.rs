use river_gauge::BoundOwner;
use std::path::{Path, PathBuf};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::UnixStream, sync::oneshot};

async fn snapshot(path: &Path) -> Vec<u8> {
    let mut stream = UnixStream::connect(path).await.unwrap();
    let request = b"{\"action\":\"inspect\"}";
    stream.write_u32(request.len() as u32).await.unwrap();
    stream.write_all(request).await.unwrap();
    stream.shutdown().await.unwrap();
    let size = stream.read_u32().await.unwrap();
    let mut body = vec![0; size as usize];
    stream.read_exact(&mut body).await.unwrap();
    assert_eq!(stream.read(&mut [0]).await.unwrap(), 0);
    body
}

#[tokio::test]
async fn visible_owner_transitions_are_committed_and_stopped() {
    let path = PathBuf::from(std::env::var_os("GAUGE_TEST_DIR").unwrap()).join("state");
    let owner = BoundOwner::bind(&path).unwrap();
    let state = owner.snapshot_publisher();
    let (stop, shutdown) = oneshot::channel();
    let server = tokio::spawn(owner.serve_until(async {
        shutdown.await.unwrap();
    }));
    assert_eq!(snapshot(&path).await, br#"{"valid":true,"reading":{"phase":"calibrating"},"problem":null}"#);
    assert!(state.publish("wrong").is_err());
    assert!(state.publish("closed").is_err());
    assert_eq!(state.current(), "calibrating");
    state.publish("recording").unwrap();
    assert_eq!(snapshot(&path).await, br#"{"valid":true,"reading":{"phase":"recording"},"problem":null}"#);
    state.publish("calibrating").unwrap();
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    assert_eq!(state.current(), "closed");
    assert!(state.publish("recording").is_err());
    assert!(!path.exists());

    let held = BoundOwner::bind(&path).unwrap();
    let retained = held.snapshot_publisher();
    drop(held);
    assert_eq!(retained.current(), "closed");
    assert!(!path.exists());

    let cancelled = BoundOwner::bind(&path).unwrap();
    let retained = cancelled.snapshot_publisher();
    let server = tokio::spawn(cancelled.serve_until(std::future::pending()));
    assert!(!snapshot(&path).await.is_empty());
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    assert_eq!(retained.current(), "closed");
    assert!(!path.exists());
}
