use gauge_reading_queue::queue::{
    Enqueued, LocalQueue, QueueError, QueueScope, Request, RequestKind,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const SCOPE: QueueScope = QueueScope {
    account: [1; 4],
    room: [2; 8],
};
const FRAME: &[u8] = &[0, 255, 10, 127];

fn request(id: u8, invocation: u8, kind: RequestKind, frame: &[u8]) -> Request<'_> {
    Request {
        scope: SCOPE,
        request_id: [id; 16],
        invocation_id: [invocation; 16],
        kind,
        digest: Sha256::digest(frame).into(),
        frame,
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let phase = args.next().unwrap();
    let path = args.next().unwrap();
    let mut db = Connection::open(&path).unwrap();
    assert!(db
        .pragma_query_value::<String, _>(None, "cipher_version", |row| row.get(0))
        .is_ok());
    if phase == "recover" {
        let unkeyed = Connection::open(&path).unwrap();
        assert!(unkeyed
            .query_row("SELECT value FROM gauge_owner_state", [], |row| row
                .get::<_, i64>(0))
            .is_err());
    }
    db.pragma_update(None, "key", "fixture-only-queue-key")
        .unwrap();
    let initial = request(3, 4, RequestKind::Initial, FRAME);
    if phase == "write" {
        db.execute_batch("CREATE TABLE gauge_owner_state(value INTEGER); INSERT INTO gauge_owner_state VALUES (17);")
            .unwrap();
        db.pragma_update(None, "synchronous", "OFF").unwrap();
        assert!(matches!(
            LocalQueue::attach(&mut db, SCOPE),
            Err(QueueError::NonDurable)
        ));
        db.pragma_update(None, "synchronous", "FULL").unwrap();
        let mut queue = LocalQueue::attach(&mut db, SCOPE).unwrap();
        let receipt = queue.enqueue(initial).unwrap();
        assert_eq!(receipt.receipt().request_id, initial.request_id);
        assert_eq!(receipt.receipt().invocation_id, initial.invocation_id);
        assert_eq!(receipt.receipt().digest, initial.digest);
        assert!(matches!(receipt, Enqueued::Queued(_)));
        assert_eq!(
            queue.enqueue(initial),
            Ok(Enqueued::AlreadyQueued(*receipt.receipt()))
        );
        let wrong_room = QueueScope {
            room: [9; 8],
            ..SCOPE
        };
        let wrong_account = QueueScope {
            account: [9; 4],
            ..SCOPE
        };
        for scope in [wrong_room, wrong_account] {
            assert_eq!(
                queue.enqueue(Request { scope, ..initial }),
                Err(QueueError::Scope)
            );
            assert_eq!(
                queue.read(scope, initial.request_id),
                Err(QueueError::Scope)
            );
        }
        assert_eq!(
            queue.enqueue(Request {
                digest: [0; 32],
                ..initial
            }),
            Err(QueueError::Digest)
        );
        for frame in [&[][..], &[0; 5][..]] {
            assert_eq!(
                queue.enqueue(request(5, 6, RequestKind::Initial, frame)),
                Err(QueueError::Frame)
            );
        }
        assert_eq!(
            queue.enqueue(request(3, 5, RequestKind::Initial, FRAME)),
            Err(QueueError::RequestCollision)
        );
        assert_eq!(
            queue.enqueue(request(8, 4, RequestKind::Initial, FRAME)),
            Err(QueueError::InvocationCollision)
        );
        assert_eq!(
            queue.enqueue(request(8, 4, RequestKind::Initial, b"bad")),
            Err(QueueError::InvocationCollision)
        );
        assert_eq!(
            queue.enqueue(request(7, 6, RequestKind::FollowUp, b"next")),
            Err(QueueError::MissingInvocation)
        );
        assert!(queue.read(SCOPE, [8; 16]).unwrap().is_none());
        assert!(matches!(
            queue.enqueue(request(7, 4, RequestKind::FollowUp, b"next")),
            Ok(Enqueued::Queued(_))
        ));
        assert!(matches!(
            queue.enqueue(request(7, 4, RequestKind::FollowUp, b"next")),
            Ok(Enqueued::AlreadyQueued(_))
        ));
        db.pragma_update(None, "foreign_keys", "ON").unwrap();
        db.execute_batch(
            "CREATE TABLE fixture_parent (id INTEGER PRIMARY KEY);
             CREATE TABLE fixture_child (
                 parent INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER gauge_reject_commit AFTER INSERT ON oddlang_queue_frames_v1
             BEGIN INSERT INTO fixture_child VALUES (1); END;",
        )
        .unwrap();
        let failed = request(10, 11, RequestKind::Initial, b"rise");
        let mut queue = LocalQueue::attach(&mut db, SCOPE).unwrap();
        assert_eq!(queue.enqueue(failed), Err(QueueError::Storage));
        assert!(queue.read(SCOPE, failed.request_id).unwrap().is_none());
        db.execute_batch("DROP TRIGGER gauge_reject_commit")
            .unwrap();
        let mut queue = LocalQueue::attach(&mut db, SCOPE).unwrap();
        assert!(matches!(queue.enqueue(failed), Ok(Enqueued::Queued(_))));
    } else {
        assert_eq!(
            db.query_row("SELECT value FROM gauge_owner_state", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            17
        );
        let mut queue = LocalQueue::attach(&mut db, SCOPE).unwrap();
        let saved = queue.read(SCOPE, initial.request_id).unwrap().unwrap();
        assert_eq!(saved.frame, FRAME);
        assert_eq!(saved.receipt.digest, initial.digest);
        assert_eq!(saved.receipt.kind, RequestKind::Initial);
        assert_eq!(
            queue.enqueue(initial),
            Ok(Enqueued::AlreadyQueued(saved.receipt))
        );
        let follow_up = queue.read(SCOPE, [7; 16]).unwrap().unwrap();
        assert_eq!(follow_up.frame, b"next");
        assert_eq!(follow_up.receipt.invocation_id, initial.invocation_id);
        assert_eq!(follow_up.receipt.kind, RequestKind::FollowUp);
        assert_eq!(queue.read(SCOPE, [10; 16]).unwrap().unwrap().frame, b"rise");
        let other_scope = QueueScope {
            room: [3; 8],
            ..SCOPE
        };
        let queue = LocalQueue::attach(&mut db, other_scope).unwrap();
        assert!(queue
            .read(other_scope, initial.request_id)
            .unwrap()
            .is_none());
    }
}
