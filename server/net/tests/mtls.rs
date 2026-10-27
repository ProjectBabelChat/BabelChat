//! Services call each other over mutual TLS: the server learns who calls,
//! refuses roles its policy does not allow, and clients refuse servers that
//! hold another service's certificate.

use axum::{Router, routing::get};
use babel_common::{Identity, Role, caller};
use babel_net::{Policy, pki::Pki, serve::serve, tls::Credentials};

fn identity(role: Role, name: &str) -> Identity {
    Identity {
        role,
        name: name.into(),
    }
}

fn credentials(dir: &std::path::Path, name: &str) -> Credentials {
    Credentials::load(
        &dir.join("ca.crt"),
        &dir.join(format!("{name}.crt")),
        &dir.join(format!("{name}.key")),
    )
    .unwrap()
}

#[tokio::test]
async fn services_know_who_calls() {
    let dir = tempfile::tempdir().unwrap();
    let pki = Pki::new(dir.path());
    pki.init().unwrap();
    let cell = identity(Role::MessageCell, "message-cell-0");
    let edge = identity(Role::Edge, "edge-0");
    let tenant = identity(Role::Tenant, "acme");
    for identity in [&cell, &edge, &tenant] {
        pki.issue(identity).unwrap();
    }

    let router = Router::new().route(
        "/who",
        get(|| async {
            caller()
                .map(|caller| caller.to_string())
                .unwrap_or_default()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}/who", listener.local_addr().unwrap());
    let server = credentials(dir.path(), "message-cell-0");
    tokio::spawn(async move {
        serve(router, listener, &server, Policy::roles(&[Role::Edge]))
            .await
            .unwrap()
    });

    let as_edge = credentials(dir.path(), "edge-0")
        .http_client(&cell)
        .unwrap();
    let response = as_edge.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "edge/edge-0");

    // A tenant may not call a Message Cell.
    let as_tenant = credentials(dir.path(), "acme").http_client(&cell).unwrap();
    assert_eq!(as_tenant.get(&url).send().await.unwrap().status(), 403);

    // The edge meant to reach another cell: the certificate does not match.
    let other = identity(Role::MessageCell, "message-cell-1");
    let misdirected = credentials(dir.path(), "edge-0")
        .http_client(&other)
        .unwrap();
    assert!(misdirected.get(&url).send().await.is_err());

    // Without a client certificate there is no connection at all.
    let anonymous = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    assert!(anonymous.get(&url).send().await.is_err());
}
