//! Experimental, explicit opt-in streaming of one already-open regular file.
//!
//! This module is independent of BlobTicket, pairing, and the default transfer
//! flow. A ticket is a bearer secret: anyone holding it may read this share until
//! shutdown. Endpoint authentication and encryption are supplied by Iroh QUIC.
//! There is no pre-hash, blob store, full copy, resume, or automatic retry.
//! Calling receive explicitly consents to a transfer within its byte budget.
//! Its request carries that budget, so an authenticated sender can send Offer
//! followed immediately by payload without a separate acceptance round trip.
//! The receiver still validates Offer before creating a partial or reading
//! payload. A rejection/cancellation closes the connection; speculative source
//! reading remains bounded by the QUIC receive/send windows and one sender
//! frame. These transport buffers are separate from the 64 KiB application
//! buffer; a receiver does not allocate a complete file in memory.
//!
//! The digest describes the bytes actually read (a CapturedStream), not an
//! atomic historical snapshot. Metadata checks detect ordinary concurrent
//! changes, but cannot guarantee a snapshot of a file being modified in place.
//!
//! Receivers require an existing private output directory, without symlink
//! components. Its directory entries must not be concurrently manipulated by
//! another actor. The portable path-based commit below does not defend against
//! a malicious process with permission to rename those directories or files.
//! The file is synced before an atomic, no-overwrite hard-link commit; parent
//! directory crash durability is not promised. Once committed, cancellation or
//! a lost ACK never removes the verified final file and must not trigger an
//! automatic retry.

use std::{
    fmt,
    fs::Metadata,
    io::{Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime},
};

use anyhow::{ensure, Context};
use data_encoding::BASE64URL_NOPAD;
use iroh::{
    address_lookup::pkarr::PkarrPublisher,
    endpoint::{presets, Connection, QuicTransportConfig, RelayMode, VarInt},
    Endpoint, EndpointAddr, SecretKey, TransportAddr,
};
use protocol::{DiscoveryModeOption, ReceiveOptions, SendOptions};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::{oneshot, Mutex},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

#[path = "stream_codec.rs"]
mod codec;
use codec::{
    read_control, read_exact, read_frame_length, require_eof, timed, validate_name, write_all,
    write_control, Complete, Manifest, Offer, Request, VERSION,
};
pub use codec::STREAM_BUFFER_BYTES;

pub const STREAM_ALPN: &[u8] = b"dashbeam-source-stream/1";
const TICKET_PREFIX: &str = "dashbeam-stream-v1:";
const MAX_TICKET_BYTES: usize = 8192;
const MAX_CONNECTIONS: usize = 4;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_TRANSPORT_WINDOW: u32 = STREAM_BUFFER_BYTES as u32;
const DEFAULT_TRANSPORT_WINDOW: u32 = 8 * 1024 * 1024;
const MAX_TRANSPORT_WINDOW: u32 = 16 * 1024 * 1024;

/// A private bearer capability for an ephemeral share. Debug redacts the secret.
#[derive(Clone)]
pub struct StreamTicket {
    addr: EndpointAddr,
    capability: [u8; 32],
}

impl fmt::Debug for StreamTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamTicket")
            .field("addr", &self.addr)
            .field("capability", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTicket {
    addr: EndpointAddr,
    capability: [u8; 32],
}

impl StreamTicket {
    pub fn from_parts(addr: EndpointAddr, capability: [u8; 32]) -> Self {
        Self { addr, capability }
    }

    pub fn addr(&self) -> &EndpointAddr {
        &self.addr
    }

    /// The returned string grants access. Do not print it or put it in logs.
    pub fn to_token(&self) -> anyhow::Result<String> {
        let bytes = serde_json::to_vec(&WireTicket {
            addr: self.addr.clone(),
            capability: self.capability,
        })?;
        let token = format!("{TICKET_PREFIX}{}", BASE64URL_NOPAD.encode(&bytes));
        ensure!(token.len() <= MAX_TICKET_BYTES, "source stream ticket too large");
        Ok(token)
    }

    pub fn from_token(token: &str) -> anyhow::Result<Self> {
        ensure!(token.len() <= MAX_TICKET_BYTES, "source stream ticket too large");
        let encoded = token
            .strip_prefix(TICKET_PREFIX)
            .context("not a source stream ticket")?;
        let bytes = BASE64URL_NOPAD
            .decode(encoded.as_bytes())
            .map_err(|_| anyhow::anyhow!("invalid source stream ticket encoding"))?;
        let wire: WireTicket = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid source stream ticket"))?;
        Ok(Self::from_parts(wire.addr, wire.capability))
    }
}

/// Per-operation bounds, independent of file size and configurable by the caller.
#[derive(Clone, Debug)]
pub struct StreamLimits {
    pub request_timeout: Duration,
    pub idle_timeout: Duration,
    pub whole_timeout: Duration,
    /// Per-file byte budget, 8 GiB by default and explicitly overridable. It is
    /// a transfer limit, not a guarantee of available disk space.
    pub max_bytes: u64,
    /// Per-connection QUIC send and receive window limits, not preallocated
    /// memory. Default 8 MiB; valid range 64 KiB through 16 MiB. Explicitly use
    /// 1 MiB for a smaller transport budget at the cost of high-RTT throughput.
    /// Actual memory also depends on outstanding data and transport overhead.
    pub transport_window_bytes: u32,
}

impl Default for StreamLimits {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(30),
            whole_timeout: Duration::from_secs(24 * 60 * 60),
            max_bytes: 8 * 1024 * 1024 * 1024,
            transport_window_bytes: DEFAULT_TRANSPORT_WINDOW,
        }
    }
}

impl StreamLimits {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.request_timeout.is_zero()
                && !self.idle_timeout.is_zero()
                && !self.whole_timeout.is_zero(),
            "source stream timeouts must be positive"
        );
        ensure!(
            (MIN_TRANSPORT_WINDOW..=MAX_TRANSPORT_WINDOW).contains(&self.transport_window_bytes),
            "source stream transport window must be 64 KiB through 16 MiB"
        );
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct StreamReceiveOptions {
    pub network: ReceiveOptions,
    pub limits: StreamLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamPath {
    Ip,
    Relay,
    Unknown,
}

#[derive(Debug)]
pub struct StreamReceiveResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub blake3: String,
    /// Includes connection setup, verification, disk sync, commit and ACK.
    pub elapsed: Duration,
    /// From this receive's start through verified, synced no-clobber commit,
    /// captured before sending the completion ACK.
    pub committed_elapsed: Duration,
    /// The first actual content byte; None for an empty file.
    pub first_payload_elapsed: Option<Duration>,
    pub initial_path: StreamPath,
    pub final_path: StreamPath,
    /// False means the file is committed, but the sender may not know that.
    pub completion_ack_sent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceStamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    ctime: (i64, i64),
}

impl SourceStamp {
    fn read(metadata: &Metadata) -> anyhow::Result<Self> {
        ensure!(metadata.is_file(), "source stream requires a regular file");
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().context("source modification time unavailable")?,
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}

struct Source {
    file: Mutex<File>,
    stamp: SourceStamp,
    name: String,
    original_path: Option<PathBuf>,
    bytes_read: AtomicU64,
    completed: AtomicU64,
}

impl Source {
    async fn unchanged(&self, file: &File) -> anyhow::Result<()> {
        ensure!(
            SourceStamp::read(&file.metadata().await?)? == self.stamp,
            "source changed since share preparation"
        );
        if let Some(path) = &self.original_path {
            ensure!(
                SourceStamp::read(&tokio::fs::metadata(path).await?)? == self.stamp,
                "source path changed since share preparation"
            );
        }
        Ok(())
    }
}

/// Owns only this experimental share's endpoint, source handle and tasks.
pub struct StreamShare {
    endpoint: Endpoint,
    capability: [u8; 32],
    source: Arc<Source>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl StreamShare {
    pub async fn start(path: PathBuf, options: SendOptions) -> anyhow::Result<Self> {
        Self::start_with_limits(path, options, StreamLimits::default()).await
    }

    pub async fn start_with_limits(
        path: PathBuf,
        options: SendOptions,
        limits: StreamLimits,
    ) -> anyhow::Result<Self> {
        let path = tokio::fs::canonicalize(path).await?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("source filename must be UTF-8")?
            .to_owned();
        let file = File::open(&path).await?.into_std().await;
        Self::start_owned(file, name, Some(path), options, limits).await
    }

    /// Takes ownership of an already-open, seekable regular file, including a
    /// duplicated Android PFD. Does not reopen it through /proc or read content.
    pub async fn start_open_file(
        file: std::fs::File,
        display_name: String,
        options: SendOptions,
    ) -> anyhow::Result<Self> {
        Self::start_open_file_with_limits(file, display_name, options, StreamLimits::default()).await
    }

    pub async fn start_open_file_with_limits(
        file: std::fs::File,
        display_name: String,
        options: SendOptions,
        limits: StreamLimits,
    ) -> anyhow::Result<Self> {
        Self::start_owned(file, display_name, None, options, limits).await
    }

    async fn start_owned(
        mut file: std::fs::File,
        name: String,
        original_path: Option<PathBuf>,
        options: SendOptions,
        limits: StreamLimits,
    ) -> anyhow::Result<Self> {
        limits.validate()?;
        validate_name(&name)?;
        let stamp = SourceStamp::read(&file.metadata()?)?;
        ensure!(stamp.len <= limits.max_bytes, "source exceeds stream byte limit");
        file.stream_position().context("source file must be seekable")?;
        let mut file = File::from_std(file);
        file.set_max_buf_size(STREAM_BUFFER_BYTES);
        let source = Arc::new(Source {
            file: Mutex::new(file),
            stamp,
            name,
            original_path,
            bytes_read: AtomicU64::new(0),
            completed: AtomicU64::new(0),
        });
        let relay: RelayMode = options.relay_mode.clone().into();
        let custom = protocol::uses_custom_infra(&options.discovery_mode, &options.relay_mode);
        let mut builder = protocol::with_system_ca_if_custom(
            Endpoint::builder(presets::Minimal),
            custom,
        )
        .secret_key(SecretKey::generate())
        .alpns(vec![STREAM_ALPN.to_vec()])
        .transport_config(transport_config(&limits)?)
        .relay_mode(relay.clone());
        if !matches!(relay, RelayMode::Disabled) {
            builder = match &options.discovery_mode {
                DiscoveryModeOption::Custom { pkarr_relay_url, .. } => {
                    builder.address_lookup(PkarrPublisher::builder(pkarr_relay_url.clone()))
                }
                DiscoveryModeOption::Default => builder.address_lookup(PkarrPublisher::n0_dns()),
            };
        }
        if let Some(addr) = options.magic_ipv4_addr {
            builder = builder.bind_addr(addr)?;
        }
        if let Some(addr) = options.magic_ipv6_addr {
            builder = builder.bind_addr(addr)?;
        }
        let endpoint = builder.bind().await?;
        let owner = EndpointOwner(endpoint.clone());
        if !matches!(relay, RelayMode::Disabled) {
            timeout(limits.request_timeout, endpoint.online())
                .await
                .context("source stream relay online timeout")?;
        }
        let capability: [u8; 32] = rand::random();
        let (stop, stop_rx) = oneshot::channel();
        let task = tokio::spawn(serve(owner, source.clone(), capability, limits, stop_rx));
        Ok(Self {
            endpoint,
            capability,
            source,
            stop: Some(stop),
            task: Some(task),
        })
    }

    /// Captures current addresses so a ticket is not frozen at bind time.
    pub fn ticket(&self) -> StreamTicket {
        let mut addr = self.endpoint.addr();
        // Explicit loopback binds are useful for isolated tests and are not
        // always included in Iroh's advertised interface addresses.
        for socket in self.endpoint.bound_sockets() {
            if !socket.ip().is_unspecified() {
                addr = addr.with_ip_addr(socket);
            }
        }
        StreamTicket::from_parts(addr, self.capability)
    }

    pub fn source_bytes_read(&self) -> u64 {
        self.source.bytes_read.load(Ordering::Relaxed)
    }

    pub fn completed_transfers(&self) -> u64 {
        self.source.completed.load(Ordering::Relaxed)
    }

    pub async fn shutdown(mut self) {
        self.signal_stop();
        if let Some(mut task) = self.task.take() {
            if timeout(SHUTDOWN_TIMEOUT, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
        close_endpoint(&self.endpoint).await;
    }

    fn signal_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for StreamShare {
    fn drop(&mut self) {
        // serve owns a JoinSet: signalling stop aborts every transfer before
        // closing this endpoint. No callback can touch a later share's state.
        self.signal_stop();
    }
}

fn transport_config(limits: &StreamLimits) -> anyhow::Result<QuicTransportConfig> {
    Ok(QuicTransportConfig::builder()
        .max_concurrent_bidi_streams(VarInt::from_u32(1))
        .max_concurrent_uni_streams(VarInt::from_u32(0))
        .stream_receive_window(VarInt::from_u32(limits.transport_window_bytes))
        .receive_window(VarInt::from_u32(limits.transport_window_bytes))
        .send_window(u64::from(limits.transport_window_bytes))
        .max_idle_timeout(Some(limits.idle_timeout.try_into()?))
        .build())
}

struct EndpointOwner(Endpoint);

impl Drop for EndpointOwner {
    fn drop(&mut self) {
        if !self.0.is_closed() {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let endpoint = self.0.clone();
                runtime.spawn(async move { close_endpoint(&endpoint).await });
            }
        }
    }
}

async fn close_endpoint(endpoint: &Endpoint) {
    let _ = timeout(SHUTDOWN_TIMEOUT, endpoint.close()).await;
}

async fn serve(
    endpoint: EndpointOwner,
    source: Arc<Source>,
    capability: [u8; 32],
    limits: StreamLimits,
    mut stop: oneshot::Receiver<()>,
) {
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = &mut stop => break,
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            incoming = endpoint.0.accept() => {
                let Some(incoming) = incoming else { break };
                // Bound the number of tasks before spawning, including peers
                // that stall during the handshake or before authentication.
                if tasks.len() >= MAX_CONNECTIONS {
                    incoming.refuse();
                    continue;
                }
                let source = source.clone();
                let limits = limits.clone();
                tasks.spawn(async move {
                    let Ok(Ok(connection)) =
                        timeout(limits.request_timeout, incoming).await
                    else { return };
                    let _guard = ConnectionOwner(connection.clone());
                    loop {
                        let result = timeout(
                            limits.whole_timeout,
                            send_one(&connection, &source, capability, &limits),
                        ).await;
                        if !matches!(result, Ok(Ok(()))) { break; }
                    }
                });
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    close_endpoint(&endpoint.0).await;
}

struct ConnectionOwner(Connection);

impl Drop for ConnectionOwner {
    fn drop(&mut self) {
        self.0.close(VarInt::from_u32(0), b"source stream closed");
    }
}

async fn send_one(
    connection: &Connection,
    source: &Source,
    capability: [u8; 32],
    limits: &StreamLimits,
) -> anyhow::Result<()> {
    let (mut tx, mut rx, mut file) = timeout(limits.request_timeout, async {
        let (mut tx, mut rx) = connection.accept_bi().await?;
        let request: Request = read_control(&mut rx, limits.idle_timeout).await?;
        // blake3::Hash equality uses constant-time byte comparison.
        ensure!(
            request.version == VERSION
                && blake3::Hash::from(request.capability) == blake3::Hash::from(capability),
            "source stream unauthorized"
        );
        // The request is the receiver's explicit transfer consent. Check both
        // budgets before acquiring the file, inspecting metadata or seeking.
        ensure!(
            source.stamp.len <= limits.max_bytes && source.stamp.len <= request.max_bytes,
            "source exceeds stream request byte limit"
        );
        // File metadata, seeking, and content reads occur only after auth.
        let mut file = source.file.lock().await;
        source.unchanged(&file).await?;
        file.seek(SeekFrom::Start(0)).await?;
        write_control(
            &mut tx,
            &Offer { version: VERSION, name: source.name.clone(), bytes: source.stamp.len },
            limits.idle_timeout,
        ).await?;
        // Payload follows Offer on this same ordered stream. Do not wait for a
        // second application round trip or start a read-ahead producer task.
        anyhow::Ok((tx, rx, file))
    }).await.context("source stream request timeout")??;

    let mut buffer = vec![0u8; STREAM_BUFFER_BYTES];
    let mut remaining = source.stamp.len;
    let mut hasher = blake3::Hasher::new();
    while remaining > 0 {
        let len = remaining.min(STREAM_BUFFER_BYTES as u64) as usize;
        timed(limits.idle_timeout, async {
            file.read_exact(&mut buffer[..len]).await.context("source truncated")?;
            Ok(())
        }).await?;
        source.bytes_read.fetch_add(len as u64, Ordering::Relaxed);
        hasher.update(&buffer[..len]);
        write_all(&mut tx, &(len as u32).to_be_bytes(), limits.idle_timeout).await?;
        write_all(&mut tx, &buffer[..len], limits.idle_timeout).await?;
        remaining -= len as u64;
    }
    timed(limits.idle_timeout, async {
        ensure!(file.read(&mut buffer[..1]).await? == 0, "source grew during transfer");
        source.unchanged(&file).await
    }).await?;
    // Release the shared cursor before waiting for a peer's completion ACK.
    drop(file);
    let digest = *hasher.finalize().as_bytes();
    write_all(&mut tx, &0u32.to_be_bytes(), limits.idle_timeout).await?;
    write_control(
        &mut tx,
        &Manifest { bytes: source.stamp.len, blake3: digest },
        limits.idle_timeout,
    ).await?;
    tx.finish()?;
    let complete: Complete = read_control(&mut rx, limits.idle_timeout).await?;
    require_eof(&mut rx, limits.idle_timeout).await?;
    ensure!(
        complete.version == VERSION
            && complete.bytes == source.stamp.len
            && complete.blake3 == digest,
        "source stream completion mismatch"
    );
    source.completed.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Reuses its own ephemeral endpoint and, for the same share, one QUIC
/// connection. Calls take &mut self so no transfer cursor or completion state
/// can be shared by concurrent receives. No automatic reconnect/retry occurs.
pub struct StreamReceiver {
    endpoint: EndpointOwner,
    limits: StreamLimits,
    connection: Option<Connection>,
}

impl StreamReceiver {
    pub async fn new(options: StreamReceiveOptions) -> anyhow::Result<Self> {
        options.limits.validate()?;
        let direct_only = matches!(
            &options.network.relay_mode,
            protocol::RelayModeOption::Disabled
        );
        let custom = protocol::uses_custom_infra(
            &options.network.discovery_mode,
            &options.network.relay_mode,
        );
        let mut builder = protocol::with_system_ca_if_custom(
            Endpoint::builder(presets::Minimal),
            custom,
        )
        .secret_key(SecretKey::generate())
        .alpns(vec![])
        .transport_config(transport_config(&options.limits)?)
        .relay_mode(options.network.relay_mode.into());
        // Direct-only mode uses explicit ticket addresses and does not publish
        // or resolve endpoint addresses through any discovery service.
        if !direct_only {
            // Keep lookup even when the ticket already includes address hints.
            builder = protocol::discovery::with_receive_discovery(
                builder,
                &options.network.discovery_mode,
            );
        }
        if let Some(addr) = options.network.magic_ipv4_addr {
            builder = builder.bind_addr(addr)?;
        }
        if let Some(addr) = options.network.magic_ipv6_addr {
            builder = builder.bind_addr(addr)?;
        }
        let endpoint = EndpointOwner(builder.bind().await?);
        Ok(Self { endpoint, limits: options.limits, connection: None })
    }

    pub async fn receive(
        &mut self,
        ticket: &StreamTicket,
        output_dir: PathBuf,
        cancel: oneshot::Receiver<()>,
    ) -> anyhow::Result<StreamReceiveResult> {
        self.receive_started(ticket, output_dir, cancel, Instant::now()).await
    }

    async fn receive_started(
        &mut self,
        ticket: &StreamTicket,
        output_dir: PathBuf,
        mut cancel: oneshot::Receiver<()>,
        started: Instant,
    ) -> anyhow::Result<StreamReceiveResult> {
        let whole_timeout = self.limits.whole_timeout;
        let transfer = async {
            let output_dir = private_output_directory(&output_dir)?;
            let reuse = self.connection.as_ref().is_some_and(|connection| {
                connection.remote_id() == ticket.addr.id && connection.close_reason().is_none()
            });
            if !reuse {
                self.close_connection();
                let connection = timeout(
                    self.limits.request_timeout,
                    self.endpoint.0.connect(ticket.addr.clone(), STREAM_ALPN),
                ).await.context("source stream connect timeout")??;
                self.connection = Some(connection);
            }
            let connection = self.connection.as_ref().expect("connection established");
            let initial_path = selected_path(connection);
            let (tx, mut rx, offer) = timeout(self.limits.request_timeout, async {
                let (mut tx, mut rx) = connection.open_bi().await?;
                write_control(
                    &mut tx,
                    &Request {
                        version: VERSION,
                        capability: ticket.capability,
                        max_bytes: self.limits.max_bytes,
                    },
                    self.limits.idle_timeout,
                ).await?;
                let offer: Offer = read_control(&mut rx, self.limits.idle_timeout).await?;
                ensure!(offer.version == VERSION, "source stream offer version");
                validate_name(&offer.name)?;
                ensure!(offer.bytes <= self.limits.max_bytes, "source exceeds receive byte limit");
                anyhow::Ok((tx, rx, offer))
            }).await.context("source stream request timeout")??;
            let mut partial = PartialFile::create(&output_dir)?;
            let verified = receive_payload(
                &mut rx, &mut partial, &offer, &self.limits, started,
            ).await?;
            anyhow::Ok((tx, verified, partial, output_dir, offer.name, initial_path))
        };
        let outcome = tokio::select! {
            biased;
            _ = &mut cancel => Err(anyhow::anyhow!("source stream cancelled")),
            result = timeout(whole_timeout, transfer) =>
                result.context("source stream whole transfer timeout").and_then(|result| result),
        };
        let (mut tx, verified, mut partial, output_dir, name, initial_path) = match outcome {
            Ok(result) => result,
            Err(error) => {
                self.close_connection();
                return Err(error);
            }
        };
        // This is the final cancellation observation after the last awaited
        // disk operation. There is no await from here through commit. A racing
        // cancellation after this observation may see an already committed
        // file; it will be returned as verified, without deleting the file.
        let path = match commit_verified(&mut partial, &output_dir, &name, &mut cancel) {
            Ok(path) => path,
            Err(error) => {
                self.close_connection();
                return Err(error);
            }
        };
        let committed_elapsed = started.elapsed();
        // Commit has happened. Report the verified path even if cancellation,
        // timeout or disconnect prevents the sender from receiving its ACK.
        let ack = async {
            write_control(
                &mut tx,
                &Complete { version: VERSION, bytes: verified.bytes, blake3: verified.digest },
                self.limits.idle_timeout,
            ).await?;
            tx.finish()?;
            let stopped = timeout(self.limits.idle_timeout, tx.stopped())
                .await.context("source stream ACK timeout")??;
            ensure!(stopped.is_none(), "source stream ACK stopped");
            anyhow::Ok(())
        };
        let completion_ack_sent = tokio::select! {
            biased;
            _ = &mut cancel => false,
            result = ack => result.is_ok(),
        };
        let final_path = self.connection.as_ref().map(selected_path).unwrap_or(StreamPath::Unknown);
        if !completion_ack_sent {
            self.close_connection();
        }
        Ok(StreamReceiveResult {
            path,
            bytes: verified.bytes,
            blake3: blake3::Hash::from(verified.digest).to_hex().to_string(),
            elapsed: started.elapsed(),
            committed_elapsed,
            first_payload_elapsed: verified.first_payload,
            initial_path,
            final_path,
            completion_ack_sent,
        })
    }

    fn close_connection(&mut self) {
        if let Some(connection) = self.connection.take() {
            connection.close(VarInt::from_u32(0), b"source stream receiver closed");
        }
    }

    pub async fn shutdown(mut self) {
        self.close_connection();
        close_endpoint(&self.endpoint.0).await;
    }
}

impl Drop for StreamReceiver {
    fn drop(&mut self) {
        self.close_connection();
    }
}

/// One cold receive. Timing starts before binding the receiver endpoint.
pub async fn receive(
    ticket: &StreamTicket,
    output_dir: PathBuf,
    options: StreamReceiveOptions,
    mut cancel: oneshot::Receiver<()>,
) -> anyhow::Result<StreamReceiveResult> {
    let started = Instant::now();
    let request_timeout = options.limits.request_timeout;
    let mut receiver = tokio::select! {
        biased;
        _ = &mut cancel => anyhow::bail!("source stream cancelled"),
        result = timeout(request_timeout, StreamReceiver::new(options)) =>
            result.context("source stream endpoint timeout")??,
    };
    let result = receiver.receive_started(ticket, output_dir, cancel, started).await;
    receiver.shutdown().await;
    result
}

fn selected_path(connection: &Connection) -> StreamPath {
    let paths = connection.paths();
    for path in paths.iter() {
        if path.is_selected() {
            return match path.remote_addr() {
                TransportAddr::Ip(_) => StreamPath::Ip,
                TransportAddr::Relay(_) => StreamPath::Relay,
                _ => StreamPath::Unknown,
            };
        }
    }
    StreamPath::Unknown
}

fn commit_verified(
    partial: &mut PartialFile,
    directory: &Path,
    name: &str,
    cancel: &mut oneshot::Receiver<()>,
) -> anyhow::Result<PathBuf> {
    match cancel.try_recv() {
        Err(oneshot::error::TryRecvError::Empty) => partial.commit(directory, name),
        Ok(()) | Err(oneshot::error::TryRecvError::Closed) => {
            anyhow::bail!("source stream cancelled before commit")
        }
    }
}

#[derive(Debug)]
struct VerifiedPayload {
    bytes: u64,
    digest: [u8; 32],
    first_payload: Option<Duration>,
}

async fn receive_payload<R: tokio::io::AsyncRead + Unpin>(
    rx: &mut R,
    partial: &mut PartialFile,
    offer: &Offer,
    limits: &StreamLimits,
    started: Instant,
) -> anyhow::Result<VerifiedPayload> {
    let mut buffer = vec![0u8; STREAM_BUFFER_BYTES];
    let mut bytes = 0u64;
    let mut first_payload = None;
    let mut hasher = blake3::Hasher::new();
    loop {
        let len = read_frame_length(rx, offer.bytes - bytes, limits.idle_timeout).await?;
        if len == 0 { break; }
        if first_payload.is_none() {
            read_exact(rx, &mut buffer[..1], limits.idle_timeout).await?;
            first_payload = Some(started.elapsed());
            read_exact(rx, &mut buffer[1..len], limits.idle_timeout).await?;
        } else {
            read_exact(rx, &mut buffer[..len], limits.idle_timeout).await?;
        }
        hasher.update(&buffer[..len]);
        timed(limits.idle_timeout, async {
            partial.file.as_mut().expect("open partial").write_all(&buffer[..len]).await?;
            Ok(())
        }).await?;
        bytes += len as u64;
    }
    let manifest: Manifest = read_control(rx, limits.idle_timeout).await?;
    require_eof(rx, limits.idle_timeout).await?;
    let digest = *hasher.finalize().as_bytes();
    ensure!(
        bytes == offer.bytes && manifest.bytes == bytes && manifest.blake3 == digest,
        "source stream length or digest mismatch"
    );
    timed(limits.idle_timeout, async {
        let file = partial.file.as_mut().expect("open partial");
        file.flush().await?;
        file.sync_all().await?;
        ensure!(file.metadata().await?.len() == bytes, "partial file length mismatch");
        Ok(())
    }).await?;
    // Close before linking/unlinking, including on platforms with strict
    // sharing semantics. The guard still owns the temporary pathname.
    partial.file.take();
    Ok(VerifiedPayload { bytes, digest, first_payload })
}

fn private_output_directory(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut checked = PathBuf::new();
    for component in absolute.components() {
        ensure!(!matches!(component, Component::ParentDir), "output directory contains ..");
        checked.push(component);
        let metadata = std::fs::symlink_metadata(&checked)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "output directory must have no symlink components"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::metadata(&absolute)?.permissions().mode();
        ensure!(permissions & 0o022 == 0, "output directory must not be writable by other users");
    }
    Ok(absolute)
}

struct PartialFile {
    file: Option<File>,
    path: PathBuf,
}

impl PartialFile {
    fn create(directory: &Path) -> anyhow::Result<Self> {
        for _ in 0..16 {
            let path = directory.join(format!(".dashbeam-stream-{}.part", uuid::Uuid::new_v4()));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    let mut file = File::from_std(file);
                    file.set_max_buf_size(STREAM_BUFFER_BYTES);
                    return Ok(Self { file: Some(file), path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("could not allocate source stream partial file")
    }

    fn commit(&mut self, directory: &Path, name: &str) -> anyhow::Result<PathBuf> {
        validate_name(name)?;
        // No check-then-overwrite: hard_link atomically fails if any file,
        // directory or symlink already occupies the candidate destination.
        for conflict in 0..1000 {
            let candidate = if conflict == 0 {
                directory.join(name)
            } else {
                let source = Path::new(name);
                let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("received");
                let extension = source.extension().and_then(|s| s.to_str());
                let name = match extension {
                    Some(extension) => format!("{stem} ({conflict}).{extension}"),
                    None => format!("{stem} ({conflict})"),
                };
                directory.join(name)
            };
            match std::fs::hard_link(&self.path, &candidate) {
                Ok(()) => {
                    // The verified final file is now committed. Failure to
                    // unlink the extra temporary name cannot undo that result;
                    // Drop attempts cleanup again.
                    let _ = std::fs::remove_file(&self.path);
                    return Ok(candidate);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("too many source stream filename conflicts")
    }
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        self.file.take();
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => tracing::warn!("failed to remove owned source stream partial file"),
        }
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
