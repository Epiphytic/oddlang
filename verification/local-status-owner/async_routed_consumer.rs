use river_gauge::BoundOwner;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::oneshot,
};

async fn exchange(path: &Path, request: &[u8]) -> Vec<u8> {
    let mut connection = UnixStream::connect(path).await.unwrap();
    connection.write_u32(request.len() as u32).await.unwrap();
    connection.write_all(request).await.unwrap();
    connection.shutdown().await.unwrap();
    let mut reply = vec![0; connection.read_u32().await.unwrap() as usize];
    connection.read_exact(&mut reply).await.unwrap();
    reply
}

#[tokio::test]
async fn waits_for_async_reply_and_bounds_a_pending_route() {
    let path = PathBuf::from(std::env::var_os("GAUGE_TEST_DIR").unwrap()).join("async");
    let owner = BoundOwner::bind(&path).unwrap();
    let (stop, shutdown) = oneshot::channel();
    let task = tokio::spawn(owner.serve_async_routed_until(
        async { shutdown.await.unwrap() },
        |body| async move {
            match body.as_slice() {
                br#"{"action":"sample"}"# => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    Some(br#"{"sample":19}"#.to_vec())
                }
                br#"{"action":"hang"}"# => std::future::pending().await,
                _ => None,
            }
        },
    ));
    let invalid = br#"{"valid":false,"reading":null,"problem":"unrecognized sample"}"#;
    let status = br#"{"valid":true,"reading":{"phase":"calibrating"},"problem":null}"#;
    assert_eq!(
        exchange(&path, br#"{"action":"sample"}"#).await,
        br#"{"sample":19}"#
    );
    let (hanging, inspected) = tokio::join!(
        exchange(&path, br#"{"action":"hang"}"#),
        exchange(&path, br#"{"action":"inspect"}"#),
    );
    assert_eq!(hanging, invalid);
    assert_eq!(inspected, status);
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(!path.exists());
}
