use super::*;
use std::{
    pin::Pin,
    sync::atomic::AtomicUsize,
    task::{Context as TaskContext, Poll},
};
use protocol::{AddrInfoOptions, RelayModeOption};
use tempfile::TempDir;
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};

fn discovery() -> DiscoveryModeOption {
    DiscoveryModeOption::Custom {
        pkarr_relay_url: "http://127.0.0.1:9/pkarr".parse().unwrap(),
        dns_origin: None,
    }
}

fn send_options() -> SendOptions {
    SendOptions {
        relay_mode: RelayModeOption::Disabled,
        discovery_mode: discovery(),
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        magic_ipv4_addr: Some("127.0.0.1:0".parse().unwrap()),
        magic_ipv6_addr: None,
    }
}

fn limits() -> StreamLimits {
    StreamLimits {
        request_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
        whole_timeout: Duration::from_secs(30),
        max_bytes: 32 * 1024 * 1024,
        // Keep cancellation and mutable-source fixtures below their known
        // source-read bounds independently of the public throughput default.
        transport_window_bytes: 1024 * 1024,
    }
}

fn receive_options() -> StreamReceiveOptions {
    StreamReceiveOptions {
        network: ReceiveOptions {
            relay_mode: RelayModeOption::Disabled,
            discovery_mode: discovery(),
            magic_ipv4_addr: Some("127.0.0.1:0".parse().unwrap()),
            magic_ipv6_addr: None,
            output_dir: None,
        },
        limits: limits(),
    }
}

fn part_count(directory: &Path) -> usize {
    std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
        .count()
}

fn canonical(directory: &TempDir) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            directory.path(),
            std::fs::Permissions::from_mode(0o700),
        ).unwrap();
    }
    directory.path().canonicalize().unwrap()
}

#[test]
fn unsafe_and_cross_platform_paths_are_rejected() {
    for name in [
        "", ".", "..", "../outside", "/absolute", "a/b", "a\\b", "C:\\outside",
        "a:b", "a\0b", "a\nb", "name.", "name ", "CON", "NUL.bin", "com1.txt",
    ] {
        assert!(validate_name(name).is_err(), "unsafe name accepted");
    }
    assert!(validate_name(&"x".repeat(codec::MAX_NAME_BYTES + 1)).is_err());
    validate_name("课程 notes.bin").unwrap();
}

#[test]
fn ticket_is_explicit_bounded_and_debug_redacted() {
    let ticket = StreamTicket::from_parts(
        EndpointAddr::from(SecretKey::generate().public()),
        [173u8; 32],
    );
    let token = ticket.to_token().unwrap();
    let restored = StreamTicket::from_token(&token).unwrap();
    assert_eq!(restored.capability, ticket.capability);
    assert_eq!(restored.addr, ticket.addr);
    assert!(format!("{ticket:?}").contains("[REDACTED]"));
    assert!(!format!("{ticket:?}").contains(&format!("{:?}", ticket.capability)));
    assert!(StreamTicket::from_token("blob-ticket-not-stream").is_err());
    assert!(StreamTicket::from_token(&"x".repeat(MAX_TICKET_BYTES + 1)).is_err());
}

#[test]
fn transport_window_has_a_finite_default_and_validates_both_boundaries() {
    assert_eq!(StreamLimits::default().transport_window_bytes, 8 * 1024 * 1024);
    for window in [0, MIN_TRANSPORT_WINDOW - 1, MAX_TRANSPORT_WINDOW + 1, u32::MAX] {
        let mut options = limits();
        options.transport_window_bytes = window;
        assert!(options.validate().is_err(), "out-of-range window accepted");
    }
    for window in [
        MIN_TRANSPORT_WINDOW,
        1024 * 1024,
        DEFAULT_TRANSPORT_WINDOW,
        MAX_TRANSPORT_WINDOW,
    ] {
        let mut options = limits();
        options.transport_window_bytes = window;
        options.validate().unwrap();
        transport_config(&options).unwrap();
    }
}

#[tokio::test]
async fn empty_and_frame_boundaries_repeat_without_preparation_reads() {
    for size in [0usize, 1, 65_535, 65_536, 65_537] {
        let source_dir = TempDir::new().unwrap();
        let output = TempDir::new().unwrap();
        let content: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let source = source_dir.path().join("payload.bin");
        tokio::fs::write(&source, &content).await.unwrap();
        let share = StreamShare::start_with_limits(source.clone(), send_options(), limits())
            .await.unwrap();
        assert_eq!(share.source_bytes_read(), 0);
        let ticket = share.ticket();
        let mut receiver = StreamReceiver::new(receive_options()).await.unwrap();
        for repetition in 1..=2 {
            let (_cancel, cancelled) = oneshot::channel();
            let result = receiver.receive(&ticket, canonical(&output), cancelled).await.unwrap();
            assert_eq!(result.bytes, size as u64);
            assert_eq!(result.blake3, blake3::hash(&content).to_hex().to_string());
            assert_eq!(result.first_payload_elapsed.is_some(), size > 0);
            assert!(result.completion_ack_sent);
            assert_eq!(tokio::fs::read(&result.path).await.unwrap(), content);
            assert_eq!(share.source_bytes_read(), (size * repetition) as u64);
            assert_eq!(part_count(output.path()), 0);
        }
        assert_eq!(tokio::fs::read(&source).await.unwrap(), content);
        receiver.shutdown().await;
        share.shutdown().await;
    }
}

#[tokio::test]
async fn an_open_regular_file_does_not_require_a_source_path() {
    let source_dir = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let original = source_dir.path().join("original.bin");
    std::fs::write(&original, b"opened source").unwrap();
    let file = std::fs::File::open(&original).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&original).unwrap();
    let share = StreamShare::start_open_file_with_limits(
        file, "display.bin".into(), send_options(), limits(),
    ).await.unwrap();
    assert_eq!(share.source_bytes_read(), 0);
    let (_cancel, cancelled) = oneshot::channel();
    let result = receive(
        &share.ticket(), canonical(&output), receive_options(), cancelled,
    ).await.unwrap();
    assert_eq!(std::fs::read(result.path).unwrap(), b"opened source");
    share.shutdown().await;
}

#[tokio::test]
async fn invalid_capability_cannot_read_source_or_create_a_partial() {
    let source = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let path = source.path().join("private.bin");
    std::fs::write(&path, b"private content").unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let mut ticket = share.ticket();
    ticket.capability[0] ^= 1;
    let (_cancel, cancelled) = oneshot::channel();
    assert!(receive(&ticket, canonical(&output), receive_options(), cancelled).await.is_err());
    assert_eq!(share.source_bytes_read(), 0);
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    share.shutdown().await;
}

#[tokio::test]
async fn over_budget_request_is_rejected_before_partial_creation_or_source_read() {
    let source = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let path = source.path().join("payload.bin");
    std::fs::write(&path, b"over budget").unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let mut options = receive_options();
    options.limits.max_bytes = 3;
    let (_cancel, cancelled) = oneshot::channel();
    assert!(receive(
        &share.ticket(), canonical(&output), options, cancelled,
    ).await.is_err());
    assert_eq!(share.source_bytes_read(), 0);
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    share.shutdown().await;
}

#[test]
fn request_requires_explicit_budget_and_rejects_old_or_unknown_fields() {
    let capability = vec![0u8; 32];
    let missing = serde_json::json!({ "version": VERSION, "capability": capability });
    assert!(serde_json::from_value::<Request>(missing).is_err());
    let unknown = serde_json::json!({
        "version": VERSION, "capability": vec![0u8; 32], "max_bytes": 1,
        "accept_later": true,
    });
    assert!(serde_json::from_value::<Request>(unknown).is_err());
    let zero_budget = serde_json::json!({
        "version": VERSION, "capability": vec![0u8; 32], "max_bytes": 0,
    });
    assert_eq!(serde_json::from_value::<Request>(zero_budget).unwrap().max_bytes, 0);
}

#[tokio::test]
async fn request_alone_starts_the_first_frame_without_an_accept_message() {
    let source = TempDir::new().unwrap();
    let path = source.path().join("payload.bin");
    let bytes = vec![79u8; STREAM_BUFFER_BYTES + 1];
    std::fs::write(&path, &bytes).unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let receiver = StreamReceiver::new(receive_options()).await.unwrap();
    let ticket = share.ticket();
    let connection = receiver.endpoint.0.connect(ticket.addr.clone(), STREAM_ALPN).await.unwrap();
    let (mut tx, mut rx) = connection.open_bi().await.unwrap();
    write_control(
        &mut tx,
        &Request {
            version: VERSION,
            capability: ticket.capability,
            max_bytes: bytes.len() as u64,
        },
        limits().idle_timeout,
    ).await.unwrap();
    let offer: Offer = read_control(&mut rx, limits().idle_timeout).await.unwrap();
    assert_eq!(offer.bytes, bytes.len() as u64);
    // Send nothing after Request: the first frame must arrive independently.
    let len = read_frame_length(&mut rx, offer.bytes, limits().idle_timeout).await.unwrap();
    let mut first_frame = vec![0u8; len];
    read_exact(&mut rx, &mut first_frame, limits().idle_timeout).await.unwrap();
    assert_eq!(first_frame, bytes[..len]);
    assert!(share.source_bytes_read() >= len as u64);
    connection.close(VarInt::from_u32(0), b"test finished after first frame");
    receiver.shutdown().await;
    share.shutdown().await;
}

#[tokio::test]
async fn rejection_after_offer_keeps_source_read_ahead_bounded() {
    rejected_offer_read_ahead(limits().transport_window_bytes, 4 * 1024 * 1024).await;
}

#[tokio::test]
async fn larger_transport_window_preserves_an_explicit_read_ahead_budget() {
    rejected_offer_read_ahead(DEFAULT_TRANSPORT_WINDOW, 24 * 1024 * 1024).await;
}

async fn rejected_offer_read_ahead(window: u32, size: usize) {
    let source = TempDir::new().unwrap();
    let path = source.path().join("payload.bin");
    // Sparse fixture: the test needs a source larger than its transport
    // budget, without allocating another complete payload in test memory.
    std::fs::File::create(&path).unwrap().set_len(size as u64).unwrap();
    let mut sender_options = limits();
    sender_options.transport_window_bytes = window;
    let share = StreamShare::start_with_limits(path, send_options(), sender_options).await.unwrap();
    assert_eq!(share.source_bytes_read(), 0);
    let mut receiver_options = receive_options();
    receiver_options.limits.transport_window_bytes = window;
    let receiver = StreamReceiver::new(receiver_options).await.unwrap();
    let ticket = share.ticket();
    let connection = receiver.endpoint.0.connect(ticket.addr.clone(), STREAM_ALPN).await.unwrap();
    let (mut tx, mut rx) = connection.open_bi().await.unwrap();
    write_control(
        &mut tx,
        &Request {
            version: VERSION,
            capability: ticket.capability,
            max_bytes: size as u64,
        },
        limits().idle_timeout,
    ).await.unwrap();
    let _: Offer = read_control(&mut rx, limits().idle_timeout).await.unwrap();
    // Stop application reads after Offer. The peer may fill the advertised
    // receive window and its own send window, plus one application frame.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let maximum_read_ahead =
        2 * u64::from(window) + STREAM_BUFFER_BYTES as u64 + codec::MAX_CONTROL_BYTES as u64;
    assert!(share.source_bytes_read() <= maximum_read_ahead);
    connection.close(VarInt::from_u32(0), b"test rejected offer");
    // Acquiring the cursor after closing observes the producer's final count,
    // not a transient sample taken while it still owns an in-flight file read.
    let cursor = timeout(limits().request_timeout, share.source.file.lock()).await.unwrap();
    assert!(share.source_bytes_read() <= maximum_read_ahead);
    assert!(share.source_bytes_read() < size as u64);
    drop(cursor);
    receiver.shutdown().await;
    share.shutdown().await;
}

#[tokio::test]
async fn cancelling_an_active_receive_cleans_partial_and_bounds_further_source_reads() {
    let source = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let path = source.path().join("payload.bin");
    let size = 8 * 1024 * 1024;
    std::fs::write(&path, vec![37u8; size]).unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let ticket = share.ticket();
    let output_path = canonical(&output);
    let (cancel, cancelled) = oneshot::channel();
    let transfer = tokio::spawn(async move {
        receive(&ticket, output_path, receive_options(), cancelled).await
    });
    timeout(Duration::from_secs(5), async {
        while part_count(output.path()) == 0 && !transfer.is_finished() {
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    assert!(part_count(output.path()) > 0, "receive finished before cancellation point");
    let read_at_cancel = share.source_bytes_read();
    cancel.send(()).unwrap();
    let error = timeout(Duration::from_secs(10), transfer).await.unwrap().unwrap().unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    let cursor = timeout(limits().request_timeout, share.source.file.lock()).await.unwrap();
    let maximum_remaining_read_ahead =
        2 * u64::from(limits().transport_window_bytes)
            + STREAM_BUFFER_BYTES as u64
            + codec::MAX_CONTROL_BYTES as u64;
    assert!(share.source_bytes_read() <= read_at_cancel + maximum_remaining_read_ahead);
    drop(cursor);
    assert_eq!(part_count(output.path()), 0);
    assert!(!output.path().join("payload.bin").exists());
    share.shutdown().await;
}

#[tokio::test]
async fn concurrent_receivers_do_not_share_a_mutable_source_cursor() {
    let source = TempDir::new().unwrap();
    let output_a = TempDir::new().unwrap();
    let output_b = TempDir::new().unwrap();
    let path = source.path().join("payload.bin");
    let bytes: Vec<_> = (0..(3 * STREAM_BUFFER_BYTES + 1)).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let ticket = share.ticket();
    let (_cancel_a, cancelled_a) = oneshot::channel();
    let (_cancel_b, cancelled_b) = oneshot::channel();
    let (a, b) = tokio::join!(
        receive(&ticket, canonical(&output_a), receive_options(), cancelled_a),
        receive(&ticket, canonical(&output_b), receive_options(), cancelled_b),
    );
    assert_eq!(std::fs::read(a.unwrap().path).unwrap(), bytes);
    assert_eq!(std::fs::read(b.unwrap().path).unwrap(), bytes);
    assert_eq!(share.source_bytes_read(), (2 * bytes.len()) as u64);
    share.shutdown().await;
}

#[tokio::test]
async fn metadata_changes_or_path_replacement_cannot_complete() {
    for replace in [false, true] {
        let source_dir = TempDir::new().unwrap();
        let output = TempDir::new().unwrap();
        let path = source_dir.path().join("payload.bin");
        std::fs::write(&path, b"original").unwrap();
        let share = StreamShare::start_with_limits(path.clone(), send_options(), limits())
            .await.unwrap();
        if replace {
            let replacement = source_dir.path().join("replacement.bin");
            std::fs::write(&replacement, b"original").unwrap();
            std::fs::rename(replacement, &path).unwrap();
        } else {
            std::fs::write(&path, b"truncated").unwrap();
        }
        let (_cancel, cancelled) = oneshot::channel();
        assert!(receive(
            &share.ticket(), canonical(&output), receive_options(), cancelled,
        ).await.is_err());
        assert_eq!(share.source_bytes_read(), 0);
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
        share.shutdown().await;
    }
}

#[tokio::test]
async fn conflict_commit_preserves_existing_file_and_symlink() {
    let source_dir = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let path = source_dir.path().join("payload.bin");
    std::fs::write(&path, b"new contents").unwrap();
    std::fs::write(output.path().join("payload.bin"), b"keep original").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("payload.bin", output.path().join("payload (1).bin")).unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let (_cancel, cancelled) = oneshot::channel();
    let received = receive(
        &share.ticket(), canonical(&output), receive_options(), cancelled,
    ).await.unwrap();
    assert_eq!(std::fs::read(&received.path).unwrap(), b"new contents");
    assert_eq!(std::fs::read(output.path().join("payload.bin")).unwrap(), b"keep original");
    assert_ne!(received.path.file_name().unwrap(), "payload.bin");
    assert_eq!(part_count(output.path()), 0);
    share.shutdown().await;
}

#[cfg(unix)]
#[test]
fn output_directory_symlinks_and_shared_write_access_are_rejected() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = TempDir::new().unwrap();
    let target = root.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let link = root.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(private_output_directory(&link).is_err());
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(private_output_directory(&target).is_err());
}

#[tokio::test]
async fn cancelled_before_start_keeps_existing_files_and_reads_nothing() {
    let source_dir = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let path = source_dir.path().join("payload.bin");
    std::fs::write(&path, b"content").unwrap();
    std::fs::write(output.path().join("keep.bin"), b"keep").unwrap();
    let share = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    let (cancel, cancelled) = oneshot::channel();
    cancel.send(()).unwrap();
    assert!(receive(
        &share.ticket(), canonical(&output), receive_options(), cancelled,
    ).await.is_err());
    assert_eq!(share.source_bytes_read(), 0);
    assert_eq!(part_count(output.path()), 0);
    assert_eq!(std::fs::read(output.path().join("keep.bin")).unwrap(), b"keep");
    share.shutdown().await;
}

#[tokio::test]
async fn malformed_streams_never_commit_and_cleanup_owned_partials() {
    for case in ["hash", "truncated", "short", "oversized", "trailing", "missing_manifest"] {
        let output = TempDir::new().unwrap();
        let (mut tx, mut rx) = tokio::io::duplex(128);
        let writer = tokio::spawn(async move {
            let idle = Duration::from_secs(2);
            let payload = b"test payload";
            let len = if case == "oversized" { STREAM_BUFFER_BYTES + 1 } else { payload.len() };
            let _ = write_all(&mut tx, &(len as u32).to_be_bytes(), idle).await;
            if case == "oversized" { return; }
            let amount = if case == "truncated" { 1 } else { payload.len() };
            let _ = write_all(&mut tx, &payload[..amount], idle).await;
            if case == "truncated" { return; }
            let _ = write_all(&mut tx, &0u32.to_be_bytes(), idle).await;
            if case == "missing_manifest" { return; }
            let digest = if case == "hash" { [0u8; 32] } else { *blake3::hash(payload).as_bytes() };
            let _ = write_control(
                &mut tx,
                &Manifest { bytes: payload.len() as u64, blake3: digest },
                idle,
            ).await;
            if case == "trailing" { let _ = tx.write_all(&[123]).await; }
        });
        {
            let mut partial = PartialFile::create(output.path()).unwrap();
            let offer = Offer {
                version: VERSION,
                name: "payload.bin".into(),
                bytes: if case == "short" { 30 } else { 12 },
            };
            assert!(receive_payload(
                &mut rx, &mut partial, &offer, &limits(), Instant::now(),
            ).await.is_err(), "malformed stream accepted");
        }
        writer.await.unwrap();
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn control_lengths_are_bounded_before_allocating_or_deserializing() {
    for length in [0u32, codec::MAX_CONTROL_BYTES as u32 + 1, u32::MAX] {
        let mut bytes = &length.to_be_bytes()[..];
        assert!(read_control::<_, Request>(&mut bytes, Duration::from_secs(1)).await.is_err());
    }
}

struct MeteredRead<R> {
    inner: R,
    largest_request: Arc<AtomicUsize>,
}

impl<R: AsyncRead + Unpin> AsyncRead for MeteredRead<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.largest_request.fetch_max(buffer.remaining(), Ordering::Relaxed);
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

#[tokio::test]
async fn large_payload_uses_fixed_frame_reads_and_small_backpressure_channel() {
    const SIZE: usize = 16 * 1024 * 1024 + 1;
    let output = TempDir::new().unwrap();
    // A producer cannot put an entire payload into this channel. It can only
    // progress as receive_payload consumes bytes and writes them to disk.
    let (mut tx, rx) = tokio::io::duplex(127);
    let largest_request = Arc::new(AtomicUsize::new(0));
    let mut rx = MeteredRead { inner: rx, largest_request: largest_request.clone() };
    let writer = tokio::spawn(async move {
        let buffer = vec![91u8; STREAM_BUFFER_BYTES];
        let mut hasher = blake3::Hasher::new();
        let mut remaining = SIZE;
        while remaining > 0 {
            let len = remaining.min(buffer.len());
            write_all(&mut tx, &(len as u32).to_be_bytes(), Duration::from_secs(5)).await.unwrap();
            write_all(&mut tx, &buffer[..len], Duration::from_secs(5)).await.unwrap();
            hasher.update(&buffer[..len]);
            remaining -= len;
        }
        write_all(&mut tx, &0u32.to_be_bytes(), Duration::from_secs(5)).await.unwrap();
        write_control(
            &mut tx,
            &Manifest { bytes: SIZE as u64, blake3: *hasher.finalize().as_bytes() },
            Duration::from_secs(5),
        ).await.unwrap();
    });
    let mut partial = PartialFile::create(output.path()).unwrap();
    let offer = Offer { version: VERSION, name: "large.bin".into(), bytes: SIZE as u64 };
    let verified = receive_payload(
        &mut rx, &mut partial, &offer, &limits(), Instant::now(),
    ).await.unwrap();
    writer.await.unwrap();
    assert_eq!(verified.bytes, SIZE as u64);
    assert!(largest_request.load(Ordering::Relaxed) <= STREAM_BUFFER_BYTES);
    let final_path = partial.commit(output.path(), &offer.name).unwrap();
    assert_eq!(std::fs::metadata(final_path).unwrap().len(), SIZE as u64);
    drop(partial);
    assert_eq!(part_count(output.path()), 0);
}

#[tokio::test]
async fn dropping_a_cancelled_payload_cleans_its_partial() {
    let output = TempDir::new().unwrap();
    let (_tx, mut rx) = tokio::io::duplex(64);
    let transfer = async {
        let mut partial = PartialFile::create(output.path()).unwrap();
        let offer = Offer { version: VERSION, name: "payload.bin".into(), bytes: 1 };
        receive_payload(&mut rx, &mut partial, &offer, &limits(), Instant::now()).await
    };
    assert!(timeout(Duration::from_millis(25), transfer).await.is_err());
    assert_eq!(part_count(output.path()), 0);
}

#[tokio::test]
async fn idle_timeout_is_bounded_and_removes_partial() {
    let output = TempDir::new().unwrap();
    let (_tx, mut rx) = tokio::io::duplex(64);
    {
        let mut partial = PartialFile::create(output.path()).unwrap();
        let offer = Offer { version: VERSION, name: "payload.bin".into(), bytes: 1 };
        let mut options = limits();
        options.idle_timeout = Duration::from_millis(25);
        assert!(receive_payload(
            &mut rx, &mut partial, &offer, &options, Instant::now(),
        ).await.unwrap_err().to_string().contains("timeout"));
    }
    assert_eq!(part_count(output.path()), 0);
}

#[tokio::test]
async fn cancellation_after_verification_prevents_commit() {
    let output = TempDir::new().unwrap();
    let (cancel, mut cancelled) = oneshot::channel();
    let mut partial = PartialFile::create(output.path()).unwrap();
    // Model the exact state immediately after payload verification and sync:
    // the temporary file is closed, but no final pathname exists.
    partial.file.take();
    cancel.send(()).unwrap();
    assert!(commit_verified(
        &mut partial, output.path(), "verified.bin", &mut cancelled,
    ).is_err());
    assert!(!output.path().join("verified.bin").exists());
    drop(partial);
    assert_eq!(part_count(output.path()), 0);
}

#[tokio::test]
async fn append_or_truncate_after_first_frame_cannot_produce_a_verified_commit() {
    for truncate in [false, true] {
        source_changed_after_first_frame(truncate).await;
    }
}

async fn source_changed_after_first_frame(truncate: bool) {
    use std::io::Write;
    let source_dir = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let source_path = source_dir.path().join("payload.bin");
    let size = 4 * 1024 * 1024;
    std::fs::write(&source_path, vec![7u8; size]).unwrap();
    let share = StreamShare::start_with_limits(source_path.clone(), send_options(), limits())
        .await.unwrap();
    let receiver = StreamReceiver::new(receive_options()).await.unwrap();
    let ticket = share.ticket();
    let connection = receiver.endpoint.0.connect(ticket.addr.clone(), STREAM_ALPN).await.unwrap();
    let (mut tx, mut rx) = connection.open_bi().await.unwrap();
    write_control(
        &mut tx,
        &Request {
            version: VERSION,
            capability: ticket.capability,
            max_bytes: limits().max_bytes,
        },
        limits().idle_timeout,
    ).await.unwrap();
    let offer: Offer = read_control(&mut rx, limits().idle_timeout).await.unwrap();
    let len = read_frame_length(&mut rx, offer.bytes, limits().idle_timeout).await.unwrap();
    let mut frame = vec![0u8; len];
    read_exact(&mut rx, &mut frame, limits().idle_timeout).await.unwrap();
    // Both QUIC windows are 1 MiB, so a 4 MiB source cannot have completed while
    // this peer has consumed only its first 64 KiB frame.
    let mut writer = std::fs::OpenOptions::new().write(true).open(&source_path).unwrap();
    if truncate {
        writer.set_len((STREAM_BUFFER_BYTES + 1) as u64).unwrap();
    } else {
        writer.seek(SeekFrom::End(0)).unwrap();
        writer.write_all(b"changed during transfer").unwrap();
    }
    writer.sync_all().unwrap();
    drop(writer);
    let mut partial = PartialFile::create(output.path()).unwrap();
    let tail = Offer {
        version: VERSION,
        name: offer.name,
        bytes: offer.bytes - len as u64,
    };
    // We deliberately omit the consumed prefix from this test partial: the
    // sender must fail before it can send any final manifest at all.
    let error = receive_payload(
        &mut rx, &mut partial, &tail, &limits(), Instant::now(),
    ).await.unwrap_err();
    assert!(
        !error.to_string().contains("length or digest mismatch"),
        "sender incorrectly emitted a manifest for the changed source"
    );
    drop(partial);
    assert!(!output.path().join("payload.bin").exists());
    assert_eq!(part_count(output.path()), 0);
    receiver.shutdown().await;
    share.shutdown().await;
}

#[tokio::test]
async fn a_lost_completion_ack_preserves_the_verified_final_file() {
    let output = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .alpns(vec![STREAM_ALPN.to_vec()])
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddrV4>().unwrap()).unwrap()
        .bind().await.unwrap();
    let socket = endpoint.bound_sockets().into_iter().find(|addr| addr.is_ipv4()).unwrap();
    let ticket = StreamTicket::from_parts(
        EndpointAddr::from(endpoint.id()).with_ip_addr(socket), [42u8; 32],
    );
    let server = endpoint.clone();
    let fake_peer = tokio::spawn(async move {
        let connection = server.accept().await.unwrap().await.unwrap();
        let (mut tx, mut rx) = connection.accept_bi().await.unwrap();
        let _: Request = read_control(&mut rx, Duration::from_secs(5)).await.unwrap();
        let content = b"verified before ack";
        write_control(
            &mut tx,
            &Offer { version: VERSION, name: "verified.bin".into(), bytes: content.len() as u64 },
            Duration::from_secs(5),
        ).await.unwrap();
        // Stop only the client's outgoing direction. The offered payload can
        // still be received, verified and committed, but its final ACK fails.
        rx.stop(VarInt::from_u32(13)).unwrap();
        write_all(&mut tx, &(content.len() as u32).to_be_bytes(), Duration::from_secs(5)).await.unwrap();
        write_all(&mut tx, content, Duration::from_secs(5)).await.unwrap();
        write_all(&mut tx, &0u32.to_be_bytes(), Duration::from_secs(5)).await.unwrap();
        write_control(
            &mut tx,
            &Manifest {
                bytes: content.len() as u64,
                blake3: *blake3::hash(content).as_bytes(),
            },
            Duration::from_secs(5),
        ).await.unwrap();
        tx.finish().unwrap();
        let _ = timeout(Duration::from_secs(5), tx.stopped()).await;
    });
    let (_cancel, cancelled) = oneshot::channel();
    let result = receive(&ticket, canonical(&output), receive_options(), cancelled).await.unwrap();
    assert!(!result.completion_ack_sent);
    assert_eq!(std::fs::read(&result.path).unwrap(), b"verified before ack");
    assert_eq!(part_count(output.path()), 0);
    fake_peer.await.unwrap();
    close_endpoint(&endpoint).await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_open_non_regular_descriptor_is_rejected_before_binding() {
    let (socket, _other) = std::os::unix::net::UnixStream::pair().unwrap();
    let descriptor: std::os::fd::OwnedFd = socket.into();
    let file = std::fs::File::from(descriptor);
    let error = StreamShare::start_open_file(file, "pipe.bin".into(), send_options())
        .await.err().expect("non-regular descriptor must be rejected");
    assert!(error.to_string().contains("regular file"));
}

#[cfg(unix)]
#[test]
fn temporary_file_is_private_and_commit_is_no_clobber() {
    use std::os::unix::fs::PermissionsExt;
    let output = TempDir::new().unwrap();
    let mut partial = PartialFile::create(output.path()).unwrap();
    let mode = std::fs::metadata(&partial.path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    partial.file.take();
    std::fs::write(output.path().join("file.bin"), b"preserve").unwrap();
    let committed = partial.commit(output.path(), "file.bin").unwrap();
    assert_ne!(committed, output.path().join("file.bin"));
    assert_eq!(std::fs::read(output.path().join("file.bin")).unwrap(), b"preserve");
}

#[tokio::test]
async fn shutdown_revokes_ticket_and_does_not_affect_a_new_share() {
    let source_dir = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let path = source_dir.path().join("payload.bin");
    std::fs::write(&path, b"payload").unwrap();
    let first = StreamShare::start_with_limits(path.clone(), send_options(), limits()).await.unwrap();
    let old_ticket = first.ticket();
    first.shutdown().await;
    let second = StreamShare::start_with_limits(path, send_options(), limits()).await.unwrap();
    assert_ne!(old_ticket.addr.id, second.ticket().addr.id);
    let mut options = receive_options();
    options.limits.request_timeout = Duration::from_millis(100);
    let (_cancel, cancelled) = oneshot::channel();
    assert!(receive(&old_ticket, canonical(&output), options, cancelled).await.is_err());
    let (_cancel, cancelled) = oneshot::channel();
    assert!(receive(
        &second.ticket(), canonical(&output), receive_options(), cancelled,
    ).await.is_ok());
    second.shutdown().await;
}
