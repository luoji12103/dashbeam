use super::*;
use iroh::protocol::Router;
use tempfile::TempDir;

const PING_ALPN: &[u8] = b"dashbeam-test-ping/1";

#[derive(Debug, Clone)]
struct Ping;
impl ProtocolHandler for Ping {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (mut tx, mut rx) = connection.accept_bi().await?;
        let mut byte = [0];
        tokio::io::AsyncReadExt::read_exact(&mut rx, &mut byte).await?;
        tokio::io::AsyncWriteExt::write_all(&mut tx, &byte).await?;
        tx.finish()?;
        let _ = tx.stopped().await;
        Ok(())
    }
}

fn limits() -> StreamLimits {
    StreamLimits {
        request_timeout: Duration::from_secs(2), idle_timeout: Duration::from_secs(2),
        whole_timeout: Duration::from_secs(15), max_bytes: 16 * 1024 * 1024,
        transport_window_bytes: 1024 * 1024,
    }
}

fn private_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    dir
}

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap()).unwrap()
        .bind().await.unwrap()
}

async fn setup() -> (Endpoint, StreamHost, Router, StreamReceiver) {
    let endpoint = endpoint().await;
    let host = StreamHost::new(limits()).unwrap();
    let router = Router::builder(endpoint.clone())
        .accept(STREAM_ALPN, host.clone()).accept(PING_ALPN, Ping).spawn();
    let receiver = StreamReceiver::from_endpoint(self::endpoint().await, limits()).unwrap();
    (endpoint, host, router, receiver)
}

async fn share(host: &StreamHost, endpoint: &Endpoint, dir: &TempDir, name: &str, data: &[u8]) -> RegisteredStreamShare {
    let path = dir.path().join(name);
    tokio::fs::write(&path, data).await.unwrap();
    host.register_file(endpoint.clone(), path, limits(), None).await.unwrap()
}

async fn receive(receiver: &mut StreamReceiver, share: &RegisteredStreamShare, out: &TempDir) -> StreamReceiveResult {
    let (_cancel, rx) = oneshot::channel();
    receiver.receive(&share.ticket(), out.path().canonicalize().unwrap(), rx).await.unwrap()
}

async fn ping(from: &Endpoint, target: &Endpoint) {
    let mut addr = target.addr();
    for socket in target.bound_sockets() {
        if !socket.ip().is_unspecified() { addr = addr.with_ip_addr(socket); }
    }
    let connection = from.connect(addr, PING_ALPN).await.unwrap();
    let (mut tx, mut rx) = connection.open_bi().await.unwrap();
    tx.write_all(b"p").await.unwrap(); tx.finish().unwrap();
    let mut byte = [0]; rx.read_exact(&mut byte).await.unwrap();
    assert_eq!(byte, *b"p");
    connection.close(0u32.into(), b"test done");
}

#[tokio::test]
async fn registration_is_zero_read_and_routes_a_b_a_on_one_connection() {
    let (endpoint, host, router, mut receiver) = setup().await;
    let src = private_dir(); let out = private_dir();
    let a = share(&host, &endpoint, &src, "a.bin", b"first source").await;
    let b = share(&host, &endpoint, &src, "b.bin", b"second source").await;
    assert_eq!(a.source_bytes_read(), 0); assert_eq!(b.source_bytes_read(), 0);
    assert_eq!(a.ticket().addr.id, b.ticket().addr.id);
    assert_ne!(a.ticket().capability, b.ticket().capability);
    for (lease, expected) in [(&a, b"first source".as_slice()), (&b, b"second source".as_slice()), (&a, b"first source".as_slice())] {
        let result = receive(&mut receiver, lease, &out).await;
        assert_eq!(tokio::fs::read(result.path).await.unwrap(), expected);
        assert!(result.completion_ack_sent);
    }
    assert_eq!(a.source_bytes_read(), 24); assert_eq!(b.source_bytes_read(), 13);
    let borrowed = receiver.endpoint.0.clone();
    receiver.shutdown().await;
    assert!(!borrowed.is_closed()); ping(&borrowed, &endpoint).await;
    a.shutdown().await; b.shutdown().await;
    assert!(!endpoint.is_closed());
    router.shutdown().await.unwrap(); borrowed.close().await;
}

#[tokio::test]
async fn revoking_a_after_switching_to_b_does_not_close_b_or_control() {
    let (endpoint, host, router, mut receiver) = setup().await;
    let src = private_dir(); let out = private_dir();
    let a = share(&host, &endpoint, &src, "a.bin", b"a").await;
    let b = share(&host, &endpoint, &src, "b.bin", &vec![7; 1024 * 1024]).await;
    receive(&mut receiver, &a, &out).await;
    // Hold B's cursor until the reused connection has selected its new Entry.
    let locked = b.entry.source.file.lock().await;
    let ticket = b.ticket(); let output = out.path().canonicalize().unwrap();
    let receive_task = tokio::spawn(async move {
        let (_cancel, rx) = oneshot::channel();
        let result = receiver.receive(&ticket, output, rx).await;
        (receiver, result)
    });
    timeout(Duration::from_secs(2), async {
        while b.entry.revoked.receiver_count() == 0 { tokio::task::yield_now().await; }
    }).await.unwrap();
    let old_ticket = a.ticket(); a.shutdown().await;
    drop(locked);
    let (mut receiver, result) = receive_task.await.unwrap();
    assert_eq!(result.unwrap().bytes, 1024 * 1024);
    let (_cancel, rx) = oneshot::channel();
    assert!(receiver.receive(&old_ticket, out.path().canonicalize().unwrap(), rx).await.is_err());
    assert!(!receiver.endpoint.0.is_closed()); ping(&receiver.endpoint.0, &endpoint).await;
    let borrowed = receiver.endpoint.0.clone(); receiver.shutdown().await;
    b.shutdown().await; router.shutdown().await.unwrap(); borrowed.close().await;
}

#[tokio::test]
async fn revoke_while_waiting_for_source_stops_only_that_share() {
    let (endpoint, host, router, mut receiver) = setup().await;
    let src = private_dir(); let out = private_dir();
    let a = share(&host, &endpoint, &src, "a.bin", &vec![1; 1024 * 1024]).await;
    let locked = a.entry.source.file.lock().await;
    let ticket = a.ticket(); let output = out.path().canonicalize().unwrap();
    let task = tokio::spawn(async move {
        let (_cancel, rx) = oneshot::channel();
        let result = receiver.receive(&ticket, output, rx).await;
        (receiver, result)
    });
    timeout(Duration::from_secs(2), async {
        while a.entry.revoked.receiver_count() == 0 { tokio::task::yield_now().await; }
    }).await.unwrap();
    a.revoke(); drop(locked);
    let (receiver, result) = task.await.unwrap(); assert!(result.is_err());
    assert_eq!(a.source_bytes_read(), 0);
    ping(&receiver.endpoint.0, &endpoint).await;
    let borrowed = receiver.endpoint.0.clone(); receiver.shutdown().await;
    router.shutdown().await.unwrap(); borrowed.close().await;
}

#[tokio::test]
async fn registry_cap_is_released_and_host_shutdown_is_permanent() {
    let (endpoint, host, router, receiver) = setup().await;
    let src = private_dir(); let path = src.path().join("same.bin");
    tokio::fs::write(&path, b"same").await.unwrap();
    let mut leases = Vec::new();
    for _ in 0..MAX_SHARES {
        leases.push(host.register_file(endpoint.clone(), path.clone(), limits(), None).await.unwrap());
    }
    assert!(host.register_file(endpoint.clone(), path.clone(), limits(), None).await.is_err());
    drop(leases.pop());
    leases.push(host.register_file(endpoint.clone(), path.clone(), limits(), None).await.unwrap());
    host.revoke_all();
    assert!(leases.iter().all(|lease| *lease.entry.revoked.borrow()));
    assert!(host.register_file(endpoint.clone(), path, limits(), None).await.is_err());
    assert!(!endpoint.is_closed()); ping(&receiver.endpoint.0, &endpoint).await;
    let borrowed = receiver.endpoint.0.clone(); receiver.shutdown().await;
    router.shutdown().await.unwrap(); borrowed.close().await;
}

#[tokio::test]
async fn revoked_before_subscribe_is_observed_and_ticket_info_is_only_a_hint() {
    let (endpoint, host, router, mut receiver) = setup().await;
    let src = private_dir(); let out = private_dir();
    let a = share(&host, &endpoint, &src, "real.bin", b"real").await;
    let mut ticket = a.ticket();
    ticket.file_info = Some(StreamFileInfo { name: "different.bin".into(), bytes: u64::MAX });
    let ticket = StreamTicket::from_token(&ticket.to_token().unwrap()).unwrap();
    let (_cancel, rx) = oneshot::channel();
    let result = receiver.receive(&ticket, out.path().canonicalize().unwrap(), rx).await.unwrap();
    assert_eq!(result.bytes, 4);
    assert_eq!(result.path.file_name().unwrap(), "real.bin");
    let entry = a.entry.clone(); a.revoke();
    let late = entry.revoked.subscribe(); assert!(*late.borrow());
    let borrowed = receiver.endpoint.0.clone(); drop(receiver);
    assert!(!borrowed.is_closed()); ping(&borrowed, &endpoint).await;
    router.shutdown().await.unwrap(); borrowed.close().await;
}

#[tokio::test]
async fn reused_connection_rechecks_peer_authorization_before_each_source_read() {
    use std::sync::atomic::AtomicBool;
    let allowed = Arc::new(AtomicBool::new(true));
    let gate = allowed.clone();
    let host = StreamHost::new(limits()).unwrap().with_peer_authorizer(move |_| {
        let allowed = gate.clone(); async move { allowed.load(Ordering::SeqCst) }
    });
    let endpoint = endpoint().await;
    let router = Router::builder(endpoint.clone()).accept(STREAM_ALPN, host.clone())
        .accept(PING_ALPN, Ping).spawn();
    let borrowed = self::endpoint().await;
    let mut receiver = StreamReceiver::from_endpoint(borrowed.clone(), limits()).unwrap();
    let src = private_dir(); let out = private_dir();
    let a = share(&host, &endpoint, &src, "a.bin", b"authorized").await;
    receive(&mut receiver, &a, &out).await;
    let read = a.source_bytes_read(); allowed.store(false, Ordering::SeqCst);
    let (_cancel, rx) = oneshot::channel();
    assert!(receiver.receive(&a.ticket(), out.path().canonicalize().unwrap(), rx).await.is_err());
    assert_eq!(a.source_bytes_read(), read);
    ping(&borrowed, &endpoint).await;
    receiver.shutdown().await; router.shutdown().await.unwrap(); borrowed.close().await;
}

#[derive(Default)]
struct Events(StdMutex<Vec<String>>);
impl protocol::EventEmitter for Events {
    fn emit_event(&self, name: &str) -> Result<(), String> {
        self.0.lock().unwrap().push(name.to_owned()); Ok(())
    }
    fn emit_event_with_payload(&self, name: &str, _: &str) -> Result<(), String> {
        self.emit_event(name)
    }
}

#[tokio::test]
async fn router_abort_emits_one_terminal_event_for_a_started_transfer() {
    let (endpoint, host, router, receiver) = setup().await;
    let src = private_dir(); let path = src.path().join("large.bin");
    tokio::fs::write(&path, vec![9; 4 * 1024 * 1024]).await.unwrap();
    let events = Arc::new(Events::default());
    let a = host.register_file(endpoint.clone(), path, limits(), Some(events.clone())).await.unwrap();
    let connection = receiver.endpoint.0.connect(a.ticket().addr, STREAM_ALPN).await.unwrap();
    let (mut tx, _rx) = connection.open_bi().await.unwrap();
    write_control(&mut tx, &Request { version: VERSION, capability: a.entry.capability,
        max_bytes: limits().max_bytes }, limits().idle_timeout).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while !events.0.lock().unwrap().iter().any(|name| name == "transfer-started") {
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    router.shutdown().await.unwrap();
    let names = events.0.lock().unwrap().clone();
    assert_eq!(names.iter().filter(|name| *name == "transfer-started").count(), 1);
    assert_eq!(names.iter().filter(|name| *name == "transfer-failed").count(), 1);
    assert!(!names.iter().any(|name| name == "transfer-completed"));
    let borrowed = receiver.endpoint.0.clone(); receiver.shutdown().await; borrowed.close().await;
}

#[tokio::test]
async fn fifth_session_is_rejected_without_queueing_and_slots_are_reusable() {
    let (endpoint, host, router, receiver) = setup().await;
    let mut addr = endpoint.addr();
    for socket in endpoint.bound_sockets() {
        if !socket.ip().is_unspecified() { addr = addr.with_ip_addr(socket); }
    }
    let mut connections = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        connections.push(receiver.endpoint.0.connect(addr.clone(), STREAM_ALPN).await.unwrap());
    }
    timeout(Duration::from_secs(1), async {
        while host.0.sessions.available_permits() != 0 { tokio::task::yield_now().await; }
    }).await.unwrap();
    let extra = receiver.endpoint.0.connect(addr.clone(), STREAM_ALPN).await.unwrap();
    timeout(Duration::from_secs(1), extra.closed()).await.unwrap();
    for connection in connections { connection.close(0u32.into(), b"test free slot"); }
    timeout(Duration::from_secs(1), async {
        while host.0.sessions.available_permits() != MAX_CONNECTIONS { tokio::task::yield_now().await; }
    }).await.unwrap();
    let fresh = receiver.endpoint.0.connect(addr, STREAM_ALPN).await.unwrap();
    assert!(fresh.close_reason().is_none());
    fresh.close(0u32.into(), b"test done");
    let borrowed = receiver.endpoint.0.clone(); receiver.shutdown().await;
    router.shutdown().await.unwrap(); borrowed.close().await;
}
