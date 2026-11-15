//! Registration on a caller-owned Router: no extra socket, relay login, store,
//! pre-hash or source copy. Dropping a share revokes only that capability.
use super::*;
use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, Weak};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::{watch, Semaphore};

type PeerAuthorizer = Arc<dyn Fn(iroh::EndpointId) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = bool> + Send>> + Send + Sync>;

const MAX_SHARES: usize = 16;

struct Entry {
    source: Arc<Source>,
    capability: [u8; 32],
    limits: StreamLimits,
    revoked: watch::Sender<bool>,
}

#[derive(Default)]
struct Registry {
    closed: bool,
    entries: HashMap<[u8; 32], Arc<Entry>>,
}

struct Inner {
    registry: StdMutex<Registry>,
    sessions: Arc<Semaphore>,
    limits: StreamLimits,
    authorize_peer: Option<PeerAuthorizer>,
}

/// Install once with Router::builder(...).accept(STREAM_ALPN, host.clone()).
/// Endpoint policy (including handshake hooks and server transport windows)
/// is inherited, not replaced by per-share StreamLimits.
#[derive(Clone)]
pub struct StreamHost(Arc<Inner>);

impl fmt::Debug for StreamHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamHost").finish_non_exhaustive()
    }
}

impl StreamHost {
    pub fn new(limits: StreamLimits) -> anyhow::Result<Self> {
        limits.validate()?;
        Ok(Self(Arc::new(Inner {
            registry: StdMutex::new(Registry::default()),
            sessions: Arc::new(Semaphore::new(MAX_CONNECTIONS)), limits, authorize_peer: None,
        })))
    }

    pub(crate) fn with_peer_authorizer<F, Fut>(mut self, authorize: F) -> Self
    where F: Fn(iroh::EndpointId) -> Fut + Send + Sync + 'static,
          Fut: std::future::Future<Output = bool> + Send + 'static,
    {
        Arc::get_mut(&mut self.0).expect("configure stream host before cloning")
            .authorize_peer = Some(Arc::new(move |peer| Box::pin(authorize(peer))));
        self
    }

    pub async fn register_file(
        &self, endpoint: Endpoint, path: PathBuf, limits: StreamLimits,
        emitter: protocol::AppHandle,
    ) -> anyhow::Result<RegisteredStreamShare> {
        let path = tokio::fs::canonicalize(path).await?;
        let name = path.file_name().and_then(|name| name.to_str())
            .context("source filename must be UTF-8")?.to_owned();
        let file = File::open(&path).await?.into_std().await;
        self.register_owned(endpoint, file, name, Some(path), limits, emitter)
    }

    /// Takes sole ownership of a seekable regular descriptor (including a
    /// future synchronously duplicated Android PFD). Never reopens /proc paths.
    pub fn register_open_file(
        &self, endpoint: Endpoint, file: std::fs::File, display_name: String,
        limits: StreamLimits, emitter: protocol::AppHandle,
    ) -> anyhow::Result<RegisteredStreamShare> {
        self.register_owned(endpoint, file, display_name, None, limits, emitter)
    }

    fn register_owned(
        &self, endpoint: Endpoint, file: std::fs::File, name: String,
        original_path: Option<PathBuf>, limits: StreamLimits,
        emitter: protocol::AppHandle,
    ) -> anyhow::Result<RegisteredStreamShare> {
        ensure!(!endpoint.is_closed(), "source stream endpoint is closed");
        let source = prepare_source(file, name, original_path, &limits, emitter)?;
        let (revoked, _) = watch::channel(false);
        let mut registry = self.0.registry.lock().expect("stream registry poisoned");
        ensure!(!registry.closed, "source stream host is closed");
        ensure!(registry.entries.len() < MAX_SHARES, "source stream share limit reached");
        let capability = loop {
            let capability: [u8; 32] = rand::random();
            if !registry.entries.contains_key(blake3::hash(&capability).as_bytes()) {
                break capability;
            }
        };
        let key = *blake3::hash(&capability).as_bytes();
        let entry = Arc::new(Entry { source, capability, limits, revoked });
        registry.entries.insert(key, entry.clone());
        Ok(RegisteredStreamShare { host: Arc::downgrade(&self.0), endpoint, key, entry })
    }

    /// Permanently revokes this runtime generation, without closing its node.
    /// Router owns/aborts accept futures after its handler shutdown callbacks.
    pub fn revoke_all(&self) {
        let mut registry = self.0.registry.lock().expect("stream registry poisoned");
        registry.closed = true;
        for (_, entry) in registry.entries.drain() {
            entry.revoked.send_replace(true);
        }
    }

    async fn serve_connection(&self, connection: Connection) -> anyhow::Result<()> {
        // Do not queue waiting tasks when saturated. Router still owns and
        // owns handshake work; this bounds active post-handshake sessions.
        let _connection = ConnectionOwner(connection.clone());
        let _permit = self.0.sessions.clone().try_acquire_owned()
            .context("source stream connection limit reached")?;
        loop {
            let (tx, mut rx) = timeout(self.0.limits.request_timeout, connection.accept_bi())
                .await.context("source stream request timeout")??;
            let request: Request = timeout(self.0.limits.request_timeout,
                read_control(&mut rx, self.0.limits.idle_timeout))
                .await.context("source stream request timeout")??;
            ensure!(request.version == VERSION, "source stream request version");
            if let Some(authorize) = &self.0.authorize_peer {
                ensure!(timeout(self.0.limits.request_timeout, authorize(connection.remote_id()))
                    .await.context("source stream authorization timeout")?,
                    "source stream peer is no longer paired or is blocked");
            }
            let entry = {
                let registry = self.0.registry.lock().expect("stream registry poisoned");
                ensure!(!registry.closed, "source stream host is closed");
                registry.entries.get(blake3::hash(&request.capability).as_bytes()).cloned()
                    .context("source stream unauthorized")?
            };
            // send_replace preserves revocation even without subscribers.
            let mut revoked = entry.revoked.subscribe();
            ensure!(!*revoked.borrow(), "source stream share revoked");
            let result = tokio::select! {
                biased;
                _ = revoked.changed() => Err(anyhow::anyhow!("source stream share revoked")),
                result = timeout(entry.limits.whole_timeout,
                    send_requested(tx, rx, request, &entry.source, entry.capability, &entry.limits)) =>
                    result.context("source stream whole transfer timeout").and_then(|result| result),
            };
            result?;
            // Drop this Entry's watcher before accepting another share on the
            // same connection: revoking A must not close an in-flight B.
        }
    }
}

impl ProtocolHandler for StreamHost {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        // No detached spawn: Router tracks and cancels this whole future.
        let _ = self.serve_connection(connection).await;
        Ok(())
    }

    async fn shutdown(&self) {
        self.revoke_all();
    }
}

/// A lease on one registered source; never owns the app endpoint or router.
pub struct RegisteredStreamShare {
    host: Weak<Inner>,
    endpoint: Endpoint,
    key: [u8; 32],
    entry: Arc<Entry>,
}

impl RegisteredStreamShare {
    pub fn ticket(&self) -> StreamTicket {
        let mut addr = self.endpoint.addr();
        for socket in self.endpoint.bound_sockets() {
            if !socket.ip().is_unspecified() { addr = addr.with_ip_addr(socket); }
        }
        let mut ticket = StreamTicket::from_parts(addr, self.entry.capability);
        ticket.file_info = Some(StreamFileInfo {
            name: self.entry.source.name.clone(), bytes: self.entry.source.stamp.len,
        });
        ticket
    }

    pub fn source_bytes_read(&self) -> u64 {
        self.entry.source.bytes_read.load(Ordering::Relaxed)
    }

    pub fn completed_transfers(&self) -> u64 {
        self.entry.source.completed.load(Ordering::Relaxed)
    }

    pub fn revoke(&self) {
        if let Some(host) = self.host.upgrade() {
            host.registry.lock().expect("stream registry poisoned").entries.remove(&self.key);
        }
        self.entry.revoked.send_replace(true);
    }

    /// Stops only this share, then waits (bounded) for its source cursor to be
    /// released. Drop is nonblocking revocation, not a completed shutdown.
    pub async fn shutdown(self) {
        self.revoke();
        let _ = timeout(SHUTDOWN_TIMEOUT, self.entry.source.file.lock()).await;
    }
}

impl Drop for RegisteredStreamShare {
    fn drop(&mut self) { self.revoke(); }
}

#[cfg(test)]
#[path = "stream_host_tests.rs"]
mod tests;
