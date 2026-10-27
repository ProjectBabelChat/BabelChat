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
    let follower = {
        let (url, client, path) = (url.clone(), client.clone(), standby_path.clone());
        tokio::spawn(async move { replication::follow(&url, &client, &path).await })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    put(&primary, "after", 2);
    let mut batch = Batch::new();
    batch.delete("t", b"before".to_vec());
    primary.write(batch).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Stop following (which closes the copy) and look at it.
    follower.abort();
    let _ = follower.await;
    let standby = RocksStore::open_existing(&standby_path).unwrap();
    assert_eq!(standby.latest_sequence(), primary.latest_sequence());
    assert_eq!(standby.get("t", b"before").unwrap(), None);
    assert_eq!(standby.get("t", b"after").unwrap(), Some(vec![2]));
}
