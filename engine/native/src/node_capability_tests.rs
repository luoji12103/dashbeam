use super::*;
use std::sync::atomic::AtomicUsize;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

#[derive(Clone, Debug)]
struct IdentityPeer {
    endpoint_id: String,
    mode: Arc<AtomicUsize>,
    hold_next_reply: Arc<AtomicBool>,
    query_seen: Arc<Notify>,
    release_reply: Arc<Notify>,
    invites: Arc<AtomicUsize>,
}

impl IdentityPeer {
    fn new(endpoint_id: String) -> Self {
        Self {
            endpoint_id,
            mode: Arc::new(AtomicUsize::new(1)),
            hold_next_reply: Arc::new(AtomicBool::new(false)),
            query_seen: Arc::new(Notify::new()),
            release_reply: Arc::new(Notify::new()),
            invites: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ProtocolHandler for IdentityPeer {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        loop {
            let Ok((mut send, mut recv)) = connection.accept_bi().await else {
                break;
            };
            let Ok(message) = read_message(&mut recv).await else {
                continue;
            };
            match message {
                ControlMessage::WhoAreYou => {
                    if self.hold_next_reply.swap(false, Ordering::SeqCst) {
                        self.query_seen.notify_one();
                        self.release_reply.notified().await;
                    }
                    let mode = self.mode.load(Ordering::SeqCst);
                    if mode == 3 {
                        tokio::time::sleep(Duration::from_millis(900)).await;
                        continue;
                    }
                    let mut identity = serde_json::json!({
                        "type":"identity",
                        "endpoint_id":if mode == 2 {"00".repeat(32)} else {self.endpoint_id.clone()},
                        "display_name":"temporary peer","device_type":"desktop","os":"linux",
                    });
                    if mode != 0 {
                        identity["capabilities"] =
                            serde_json::json!([protocol::control::SOURCE_STREAM_CAPABILITY]);
                    }
                    let bytes = serde_json::to_vec(&identity).unwrap();
                    let _ = send.write_all(&(bytes.len() as u32).to_be_bytes()).await;
                    let _ = send.write_all(&bytes).await;
                    let _ = send.finish();
                }
                ControlMessage::Invite { .. } => {
                    self.invites.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

async fn node() -> (tempfile::TempDir, Arc<NodeService>) {
    let directory = tempfile::tempdir().unwrap();
    let identity = Arc::new(DeviceIdentity::temporary_for_tests(directory.path()));
    let node = NodeService::start_with_identity(
        directory.path(),
        identity,
        RelayMode::Disabled,
        DiscoveryModeOption::Custom {
            pkarr_relay_url: "http://127.0.0.1:9/pkarr".parse().unwrap(),
            dns_origin: None,
        },
        Discoverability::Off,
        None,
    )
    .await
    .unwrap();
    (directory, Arc::new(node))
}

#[tokio::test]
async fn paired_invite_cancel_gate_prevents_write_and_started_error_is_not_retried() {
    let (_directory, node) = node().await;
    let (endpoint, router, handler) = mock_peer().await;
    let connection = link(&node, &endpoint).await;
    let id = endpoint.id().to_string();
    let current_ticket = Arc::new(Mutex::new(Some("ticket")));
    let mut cancel_guard = current_ticket.lock().await;
    let gate_calls = Arc::new(AtomicUsize::new(0));
    let gate_seen = Arc::new(Notify::new());
    let invite = {
        let node = node.clone();
        let id = id.clone();
        let current_ticket = current_ticket.clone();
        let gate_calls = gate_calls.clone();
        let gate_seen = gate_seen.clone();
        tokio::spawn(async move {
            node.invite_paired_device_guarded(&id, "ticket", 1, 4, || async {
                gate_calls.fetch_add(1, Ordering::SeqCst);
                gate_seen.notify_one();
                let guard = current_ticket.lock().await;
                if guard.as_ref() == Some(&"ticket") {
                    Some(guard)
                } else {
                    None
                }
            })
            .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), gate_seen.notified())
        .await
        .unwrap();
    // Cancellation owns the same gate before the Invite can start writing.
    *cancel_guard = None;
    drop(cancel_guard);
    assert!(invite
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    assert_eq!(gate_calls.load(Ordering::SeqCst), 1);
    assert_eq!(handler.invites.load(Ordering::SeqCst), 0);

    // Once a send has been approved, even a transport failure must not lead
    // to a reconnect/replay. Close at that exact boundary to force open_bi Err.
    let failure = node
        .invite_paired_device_guarded(&id, "ticket", 1, 4, || async {
            gate_calls.fetch_add(1, Ordering::SeqCst);
            connection.close(0u32.into(), b"synthetic write failure");
            Some(())
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(
        gate_calls.load(Ordering::SeqCst),
        2,
        "one gate per explicit call"
    );
    assert_eq!(
        handler.invites.load(Ordering::SeqCst),
        0,
        "no replay after send started"
    );
    node.shutdown().await.unwrap();
    router.shutdown().await.unwrap();
}

async fn link(node: &NodeService, endpoint: &Endpoint) -> Connection {
    let local = node.runtime.lock().await.endpoint.clone();
    let mut address = endpoint.addr();
    for socket in endpoint.bound_sockets() {
        if !socket.ip().is_unspecified() {
            address = address.with_ip_addr(socket);
        }
    }
    let connection = local.connect(address, CONTROL_ALPN).await.unwrap();
    node.access.write().await.allowed.insert(endpoint.id());
    node.paired_connections
        .register_inbound(&endpoint.id().to_string(), connection.clone())
        .await;
    connection
}

async fn mock_peer() -> (Endpoint, Router, IdentityPeer) {
    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap())
        .unwrap()
        .bind()
        .await
        .unwrap();
    let handler = IdentityPeer::new(endpoint.id().to_string());
    let router = Router::builder(endpoint.clone())
        .accept(CONTROL_ALPN, handler.clone())
        .spawn();
    (endpoint, router, handler)
}

#[tokio::test]
async fn live_capability_absence_mismatch_and_timeout_preserve_control() {
    let (_directory, node) = node().await;
    let (endpoint, router, handler) = mock_peer().await;
    let connection = link(&node, &endpoint).await;
    let id = endpoint.id().to_string();
    assert!(node.negotiate_source_stream(&id).await.unwrap().is_some());
    for mode in [0, 2, 3] {
        handler.mode.store(mode, Ordering::SeqCst);
        assert!(node.negotiate_source_stream(&id).await.unwrap().is_none());
        assert!(
            connection.close_reason().is_none(),
            "an old/unknown peer must keep its CONTROL session"
        );
    }
    handler.mode.store(0, Ordering::SeqCst);
    assert!(node
        .invite_paired_device(&id, "synthetic-legacy-ticket", 1, 4)
        .await
        .unwrap());
    tokio::time::timeout(Duration::from_secs(2), async {
        while handler.invites.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    node.shutdown().await.unwrap();
    router.shutdown().await.unwrap();
}

#[tokio::test]
async fn live_capability_late_reply_and_old_lease_cannot_cross_session_replacement() {
    let (_directory, node) = node().await;
    let (endpoint, router, handler) = mock_peer().await;
    let first = link(&node, &endpoint).await;
    let id = endpoint.id().to_string();
    let lease = node.negotiate_source_stream(&id).await.unwrap().unwrap();

    handler.hold_next_reply.store(true, Ordering::SeqCst);
    let query_node = node.clone();
    let query_id = id.clone();
    let query =
        tokio::spawn(async move { query_node.negotiate_source_stream(&query_id).await.unwrap() });
    handler.query_seen.notified().await;
    node.paired_connections.forget(&id).await;
    let replacement = link(&node, &endpoint).await;
    assert_ne!(first.stable_id(), replacement.stable_id());
    handler.release_reply.notify_one();
    assert!(
        query.await.unwrap().is_none(),
        "a late old-session reply must be discarded"
    );
    assert!(node
        .invite_source_stream_peer(&lease, "synthetic-stream-ticket", 4)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        handler.invites.load(Ordering::SeqCst),
        0,
        "fallback is allowed only before any Invite"
    );

    handler.mode.store(0, Ordering::SeqCst); // Same device, now an older client.
    assert!(node.negotiate_source_stream(&id).await.unwrap().is_none());
    handler.mode.store(1, Ordering::SeqCst);
    let fresh = node.negotiate_source_stream(&id).await.unwrap().unwrap();
    assert_eq!(
        node.invite_source_stream_peer(&fresh, "synthetic-stream-ticket", 4)
            .await
            .unwrap(),
        Some(true)
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while handler.invites.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    node.shutdown().await.unwrap();
    router.shutdown().await.unwrap();
}
