//! A standby copies a running service's store over mutual TLS: a checkpoint
//! first, then every write as it happens. Only the service itself (its
//! certificate) may do so.

use std::{sync::Arc, time::Duration};

use axum::Router;
use babel_common::{Identity, Role};
use babel_net::{Policy, pki::Pki, replication, serve::serve, tls::Credentials};
use babel_store::{Batch, KvStore, RocksStore};

fn credentials(dir: &std::path::Path, name: &str) -> Credentials {
    Credentials::load(
        &dir.join("ca.crt"),
        &dir.join(format!("{name}.crt")),
        &dir.join(format!("{name}.key")),
    )
    .unwrap()
}

fn put(store: &RocksStore, key: &str, value: u8) {
    let mut batch = Batch::new();
    batch.put("t", key.as_bytes().to_vec(), vec![value]);
    store.write(batch).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn standbys_follow_their_primary() {
    let pki = tempfile::tempdir().unwrap();
    let authority = Pki::new(pki.path());
    authority.init().unwrap();
    let cell = Identity {
        role: Role::MessageCell,
        name: "message-cell-0".into(),
    };
    let edge = Identity {
        role: Role::Edge,
        name: "edge-0".into(),
    };
    authority.issue(&cell).unwrap();
    authority.issue(&edge).unwrap();

    let data = tempfile::tempdir().unwrap();
    let primary = Arc::new(RocksStore::open(&data.path().join("primary"), &["t"]).unwrap());
    put(&primary, "before", 1);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let server = credentials(pki.path(), "message-cell-0");
    let router = Router::new().merge(replication::router(primary.clone()));
    tokio::spawn(async move {
        serve(
            router,
            listener,
            &server,
            Policy::roles(&[Role::Edge]).with_replication(&cell),
        )
        .await
    });

    // Edges may call the service, but not copy it.
    let as_edge = credentials(pki.path(), "edge-0")
        .http_client(&Identity {
            role: Role::MessageCell,
            name: "message-cell-0".into(),
        })
        .unwrap();
    let refused = as_edge
        .get(format!("{url}/replication/updates?after=0&wait_ms=0"))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 403);

    let own = credentials(pki.path(), "message-cell-0");
    let client = own
        .http_client(&Identity {
            role: Role::MessageCell,
            name: "message-cell-0".into(),
        })
        .unwrap();
    let standby_path = data.path().join("standby");
    let follow = || {
        let (url, client, path) = (url.clone(), client.clone(), standby_path.clone());
        tokio::spawn(async move { replication::follow(&url, &client, &path).await })
    };
    let mut follower = follow();
    tokio::time::sleep(Duration::from_millis(500)).await;
    put(&primary, "after", 2);
    let mut batch = Batch::new();
    batch.delete("t", b"before".to_vec());
    primary.write(batch).unwrap();

    // Stop following (which closes the copy) and look at it; if it is not
    // there yet (a slow machine), follow again: a standby resumes from what
    // it has, or from a new checkpoint if it never finished the first.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let standby = loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        follower.abort();
        let _ = follower.await;
        // The copy closes once a write in progress is done.
        let mut opened = None;
        for _ in 0..20 {
            match RocksStore::open_existing(&standby_path) {
                Ok(standby) => {
                    opened = Some(standby);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        if let Some(standby) = opened {
            if standby.latest_sequence() == primary.latest_sequence() {
                break standby;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the standby never caught up"
        );
        follower = follow();
    };
    assert_eq!(standby.get("t", b"before").unwrap(), None);
    assert_eq!(standby.get("t", b"after").unwrap(), Some(vec![2]));
}

/// Serves `store`'s replication routes on `listener` as `message-cell-0`.
fn serve_primary(
    pki: &std::path::Path,
    store: Arc<RocksStore>,
    listener: tokio::net::TcpListener,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    let cell = Identity {
        role: Role::MessageCell,
        name: "message-cell-0".into(),
    };
    let server = credentials(pki, "message-cell-0");
    let router = Router::new().merge(replication::router(store));
    tokio::spawn(async move {
        serve(
            router,
            listener,
            &server,
            Policy::roles(&[Role::Edge]).with_replication(&cell),
        )
        .await
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn standbys_copy_again_when_the_primary_gains_a_table() {
    let pki = tempfile::tempdir().unwrap();
    let authority = Pki::new(pki.path());
    authority.init().unwrap();
    let cell = Identity {
        role: Role::MessageCell,
        name: "message-cell-0".into(),
    };
    authority.issue(&cell).unwrap();
    let data = tempfile::tempdir().unwrap();
    let (primary_path, standby_path) = (data.path().join("primary"), data.path().join("standby"));
    let follow = |url: String| {
        let client = credentials(pki.path(), "message-cell-0")
            .http_client(&cell)
            .unwrap();
        let path = standby_path.clone();
        tokio::spawn(async move { replication::follow(&url, &client, &path).await })
    };

    // A copy of the service as it was.
    let primary = Arc::new(RocksStore::open(&primary_path, &["t"]).unwrap());
    put(&primary, "old", 1);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let server = serve_primary(pki.path(), primary.clone(), listener);
    let follower = follow(url);
    tokio::time::sleep(Duration::from_secs(1)).await;
    follower.abort();
    let _ = follower.await;
    server.abort();
    let _ = server.await;
    drop(primary);

    // Its new version has one more table, written to at once.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let primary = loop {
        match RocksStore::open(&primary_path, &["t", "u"]) {
            Ok(store) => break Arc::new(store),
            Err(_) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("the primary stayed open: {error}"),
        }
    };
    let mut batch = Batch::new();
    batch.put("u", b"new".to_vec(), vec![2]);
    primary.write(batch).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let _server = serve_primary(pki.path(), primary.clone(), listener);

    // The standby resumes from its copy, meets the table, and starts over
    // from a new checkpoint, which has it.
    let follower = follow(url);
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!follower.is_finished(), "the standby stopped");
        // RocksDB lists a store's tables in its OPTIONS files.
        let copied = std::fs::read_dir(&standby_path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("OPTIONS-"))
            .any(|entry| {
                std::fs::read_to_string(entry.path())
                    .is_ok_and(|options| options.contains("[CFOptions \"u\"]"))
            });
        if copied {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the standby never copied the new table"
        );
    }
    follower.abort();
    let _ = follower.await;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let standby = loop {
        match RocksStore::open_existing(&standby_path) {
            Ok(standby) => break standby,
            Err(_) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => panic!("the standby stayed open: {error}"),
        }
    };
    assert_eq!(standby.get("u", b"new").unwrap(), Some(vec![2]));
    assert_eq!(standby.get("t", b"old").unwrap(), Some(vec![1]));
}
