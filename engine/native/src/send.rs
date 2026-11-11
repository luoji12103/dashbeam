use std::path::PathBuf;

use protocol::{
    run_share_session, AppHandle, DiscoveryModeOption, FileMetadata, SendOptions, METADATA_ALPN,
};
use iroh::endpoint::presets;
use iroh::{address_lookup::pkarr::PkarrPublisher, endpoint::RelayMode, Endpoint};
use iroh_blobs::{
    provider::events::{ConnectMode, EventMask, EventSender, RequestMode},
    BlobsProtocol,
};
use tokio::sync::mpsc;

use crate::import::canonicalize_input_paths;
use crate::storage;
use crate::types::{AutoCleanupDir, SendResult};

const SHARE_ONLINE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

async fn prepare_share_content(
    endpoint: &Endpoint,
    store: &iroh_blobs::store::fs::FsStore,
    paths: Vec<PathBuf>,
    relay_mode: &RelayMode,
    online_timeout: std::time::Duration,
) -> anyhow::Result<(
    iroh_blobs::api::TempTag,
    u64,
    iroh_blobs::format::collection::Collection,
)> {
    let online = async {
        if !matches!(relay_mode, RelayMode::Disabled) {
            tokio::time::timeout(online_timeout, endpoint.online())
                .await
                .map_err(|_| anyhow::anyhow!("timed out waiting for share relay connection"))?;
        }
        anyhow::Ok(())
    };
    // The legacy ticket still needs the completed collection hash. Connecting
    // to the configured relay can happen while that import/hash is in progress.
    let prepared = tokio::try_join!(online, crate::import::import_paths(paths, store.as_ref()));
    match prepared {
        Ok(((), imported)) => Ok(imported),
        Err(error) => {
            endpoint.close().await;
            if let Err(cleanup_error) = store.shutdown().await {
                tracing::warn!(error = %cleanup_error, "failed to shut down failed share store");
            }
            Err(error)
        }
    }
}

/// Deprecated: prefer [`start_share_items`].
pub async fn start_share(
    path: PathBuf,
    options: SendOptions,
    app_handle: AppHandle,
    metadata: Option<FileMetadata>,
) -> anyhow::Result<SendResult> {
    start_share_items(vec![path], options, &app_handle, metadata).await
}

/// Starts sharing the provided paths (files or directories).
pub async fn start_share_items(
    paths: Vec<PathBuf>,
    options: SendOptions,
    app_handle: &AppHandle,
    metadata: Option<FileMetadata>,
) -> anyhow::Result<SendResult> {
    use protocol::{get_or_create_secret, AddrInfoOptions};
    use anyhow::ensure;

    ensure!(!paths.is_empty(), "no paths provided for sharing");

    // Ephemeral endpoint: must not compete with the online control endpoint for
    // the same relay identity.
    let secret_key = get_or_create_secret()?;
    let relay_mode: RelayMode = options.relay_mode.clone().into();
    let builder = match &options.discovery_mode {
        DiscoveryModeOption::Custom { .. } => Endpoint::builder(presets::Minimal),
        DiscoveryModeOption::Default => Endpoint::builder(presets::N0),
    };
    let custom_infra =
        protocol::uses_custom_infra(&options.discovery_mode, &options.relay_mode);
    let mut builder = protocol::with_system_ca_if_custom(builder, custom_infra)
        .alpns(vec![iroh_blobs::ALPN.to_vec(), METADATA_ALPN.to_vec()])
        .secret_key(secret_key)
        .relay_mode(relay_mode.clone());

    match &options.discovery_mode {
        // Self-hosted discovery: publish our home relay to the custom pkarr relay so
        // peers can resolve us by endpoint id without touching n0's servers.
        DiscoveryModeOption::Custom { pkarr_relay_url, .. } => {
            builder = builder.address_lookup(PkarrPublisher::builder(pkarr_relay_url.clone()));
        }
        DiscoveryModeOption::Default => {
            if options.ticket_type == AddrInfoOptions::Id {
                builder = builder.address_lookup(PkarrPublisher::n0_dns());
            }
        }
    }
    if let Some(addr) = options.magic_ipv4_addr {
        builder = builder.bind_addr(addr)?;
    }
    if let Some(addr) = options.magic_ipv6_addr {
        builder = builder.bind_addr(addr)?;
    }

    let blobs_data_dir = storage::new_send_blobs_dir();
    let canonical_paths = canonicalize_input_paths(paths)?;
    let (progress_tx, progress_rx) = mpsc::channel(64);
    let app_handle_clone = app_handle.clone();
    let is_collection = canonical_paths.len() > 1;
    let entry_type_for_progress = if is_collection {
        "collection".to_string()
    } else if canonical_paths[0].is_dir() {
        "directory".to_string()
    } else {
        "file".to_string()
    };
    let ticket_type = options.ticket_type;

    let endpoint = builder.bind().await?;
    let cleanup_dir = AutoCleanupDir::new(blobs_data_dir.clone());
    let store = match storage::create_send_store(&blobs_data_dir).await {
        Ok(store) => store,
        Err(error) => {
            endpoint.close().await;
            return Err(error);
        }
    };

    let blobs = BlobsProtocol::new(
        &store,
        Some(EventSender::new(
            progress_tx,
            EventMask {
                connected: ConnectMode::Notify,
                get: RequestMode::NotifyLog,
                ..EventMask::DEFAULT
            },
        )),
    );

    let (temp_tag, size, _collection) = prepare_share_content(
        &endpoint,
        &store,
        canonical_paths,
        &relay_mode,
        SHARE_ONLINE_TIMEOUT,
    )
    .await?;

    let outcome = run_share_session(
        endpoint.clone(),
        store.clone(),
        blobs,
        temp_tag,
        size,
        metadata,
        ticket_type,
        &app_handle_clone,
        entry_type_for_progress,
        relay_mode,
        Some(blobs_data_dir),
        progress_rx,
    )
    .await;

    match outcome {
        Ok(outcome) => Ok(SendResult {
            ticket: outcome.ticket,
            hash: outcome.hash,
            size: outcome.size,
            entry_type: outcome.entry_type,
            router: outcome.router.expect("ephemeral share router"),
            temp_tag: outcome.temp_tag,
            blobs_data_dir: cleanup_dir,
            _progress_handle: outcome.progress_handle,
            _store: outcome.store,
            completed_peers: outcome.completed_peers,
        }),
        Err(error) => {
            endpoint.close().await;
            if let Err(cleanup_error) = store.shutdown().await {
                tracing::warn!(error = %cleanup_error, "failed to shut down failed share store");
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn offline_endpoint() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .expect("offline endpoint should bind")
    }

    async fn assert_closed(endpoint: &Endpoint, store: &iroh_blobs::store::fs::FsStore) {
        assert!(endpoint.is_closed(), "failed share must close its endpoint");
        assert!(
            store.tags().list().await.is_err(),
            "failed share must shut down its store"
        );
    }

    #[tokio::test]
    async fn share_online_timeout_closes_endpoint_and_store() {
        let source = TempDir::new().unwrap();
        let path = source.path().join("synthetic.bin");
        tokio::fs::write(&path, b"synthetic share content").await.unwrap();
        let data = TempDir::new().unwrap();
        let store = storage::create_send_store(data.path()).await.unwrap();
        let endpoint = offline_endpoint().await;

        let error = prepare_share_content(
            &endpoint,
            &store,
            vec![path],
            &RelayMode::Default,
            Duration::from_millis(25),
        )
        .await
        .expect_err("an offline endpoint must time out");

        assert!(error.to_string().contains("share relay connection"));
        assert_closed(&endpoint, &store).await;
    }

    #[tokio::test]
    async fn share_import_failure_closes_endpoint_and_store() {
        let empty_source = TempDir::new().unwrap();
        let data = TempDir::new().unwrap();
        let store = storage::create_send_store(data.path()).await.unwrap();
        let endpoint = offline_endpoint().await;

        let error = prepare_share_content(
            &endpoint,
            &store,
            vec![empty_source.path().to_path_buf()],
            &RelayMode::Disabled,
            Duration::from_secs(1),
        )
        .await
        .expect_err("empty collection cannot produce a legacy ticket");

        assert!(error.to_string().contains("no valid files"));
        assert_closed(&endpoint, &store).await;
    }
}
