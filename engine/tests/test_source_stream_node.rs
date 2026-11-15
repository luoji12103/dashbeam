//! Offline product-node integration; no production identities or relays.
use engine::{Discoverability, DiscoveryModeOption, NodeService, StreamLimits};
use iroh::endpoint::RelayMode;
use std::time::{Duration, Instant};

fn private_dir() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory
}

fn discovery(port: u16) -> DiscoveryModeOption {
    DiscoveryModeOption::Custom {
        pkarr_relay_url: format!("http://127.0.0.1:{port}/pkarr").parse().unwrap(),
        dns_origin: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistent_node_streaming_pairing_revocation_and_rebuild() {
    let a_dir = private_dir(); let b_dir = private_dir();
    let source = private_dir(); let output = private_dir();
    let a = NodeService::start(a_dir.path(), RelayMode::Disabled, discovery(9), Discoverability::Off, None).await.unwrap();
    let b = NodeService::start(b_dir.path(), RelayMode::Disabled, discovery(9), Discoverability::Off, None).await.unwrap();
    let limits = StreamLimits { request_timeout: Duration::from_secs(3), idle_timeout: Duration::from_secs(3),
        whole_timeout: Duration::from_secs(30), ..Default::default() };
    let source_path = source.path().join("node 中文 spaces.bin");
    let fixture: Vec<u8> = (0..1024 * 1024).map(|index| (index % 251) as u8).collect();
    tokio::fs::write(&source_path, &fixture).await.unwrap();
    let mut receiver = b.stream_receiver(limits.clone()).await.unwrap();
    let preparation = Instant::now();
    let share = a.start_stream_share(source_path.clone(), limits.clone()).await.unwrap();
    let prepared = preparation.elapsed();
    assert_eq!(share.source_bytes_read(), 0);
    assert_eq!(share.ticket().addr().id.to_string(), a.device_info().endpoint_id);
    let (_cancel, rx) = tokio::sync::oneshot::channel();
    assert!(receiver.receive(&share.ticket(), output.path().canonicalize().unwrap(), rx).await.is_err());
    assert_eq!(share.source_bytes_read(), 0, "unpaired peer must not read source");
    a.remember_paired_device_for_tests(&b.device_info().endpoint_id).await.unwrap();
    b.remember_paired_device_for_tests(&a.device_info().endpoint_id).await.unwrap();
    let mut measurements = Vec::new();
    for _ in 0..3 {
        let (_cancel, rx) = tokio::sync::oneshot::channel();
        let result = receiver.receive(&share.ticket(), output.path().canonicalize().unwrap(), rx).await.unwrap();
        assert_eq!(tokio::fs::read(&result.path).await.unwrap(), fixture);
        assert!(result.completion_ack_sent);
        measurements.push(result.first_payload_elapsed.unwrap().as_micros());
    }
    a.forget_paired_silently_for_tests(&b.device_info().endpoint_id).await.unwrap();
    let read = share.source_bytes_read();
    let (_cancel, rx) = tokio::sync::oneshot::channel();
    assert!(receiver.receive(&share.ticket(), output.path().canonicalize().unwrap(), rx).await.is_err());
    assert_eq!(share.source_bytes_read(), read, "reused connection must recheck pairing");
    a.remember_paired_device_for_tests(&b.device_info().endpoint_id).await.unwrap();
    a.reconfigure_network(RelayMode::Disabled, discovery(10)).await.unwrap();
    let (_cancel, rx) = tokio::sync::oneshot::channel();
    assert!(receiver.receive(&share.ticket(), output.path().canonicalize().unwrap(), rx).await.is_err());
    assert_eq!(share.source_bytes_read(), read);
    share.shutdown().await;
    let fresh = a.start_stream_share(source_path, limits).await.unwrap();
    let (_cancel, rx) = tokio::sync::oneshot::channel();
    let result = receiver.receive(&fresh.ticket(), output.path().canonicalize().unwrap(), rx).await.unwrap();
    assert_eq!(tokio::fs::read(result.path).await.unwrap(), fixture);
    receiver.shutdown().await; fresh.shutdown().await;
    assert!(a.is_network_ready() && b.is_network_ready());
    println!("persistent_node: prepare_us={} zero_prepare_reads=true first_payload_us={measurements:?} paired_revoke=true rebuild=true", prepared.as_micros());
    a.shutdown().await.unwrap(); b.shutdown().await.unwrap();
}
