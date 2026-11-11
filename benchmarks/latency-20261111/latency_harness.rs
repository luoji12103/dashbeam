//! App-command orchestration simulation over the real engine, not a GUI test.
//! Copy unchanged into each revision's engine/examples/latency_harness.rs.
//! Cross-process timestamps use the same Linux kernel's CLOCK_MONOTONIC.
use anyhow::{anyhow, bail, Context, Result};
use iroh::Watcher;
use engine::{
    build_discovery_mode, download, fetch_metadata, resolve_relay_mode_with_fallback,
    start_share_items, AddrInfoOptions, AppHandle, DiscoveryConfigArg, EventEmitter, FileMetadata,
    ReceiveOptions, RelayConfigArg, RelayModeOption, SendOptions, SendResult,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn now_ns() -> u64 {
    let mut ts = nix::libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { nix::libc::clock_gettime(nix::libc::CLOCK_MONOTONIC, &mut ts) }, 0);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

struct Args {
    role: String,
    variant: String,
    shared: PathBuf,
    source: PathBuf,
    relay: String,
    stall_ms: u64,
}

fn args() -> Result<Args> {
    let mut map = BTreeMap::new();
    let mut it = std::env::args().skip(1);
    while let Some(key) = it.next() {
        if !key.starts_with("--") || map.insert(key, it.next().context("missing argument value")?).is_some() {
            bail!("invalid or duplicated argument");
        }
    }
    let required = |name: &str| -> Result<String> {
        map.get(name).cloned().with_context(|| format!("missing {name}"))
    };
    if map.len() != 6 { bail!("expected six named options"); }
    let a = Args {
        role: required("--role")?,
        variant: required("--variant")?,
        shared: required("--shared-dir")?.into(),
        source: required("--source-path")?.into(),
        relay: required("--relay")?,
        stall_ms: required("--metadata-stall-ms")?.parse()?,
    };
    if !["send", "recv"].contains(&a.role.as_str())
        || !["baseline", "candidate"].contains(&a.variant.as_str())
        || !["disabled", "http://127.0.0.1:3340", "https://relay.dashbeam.test:3341",
            "https://relay-b.dashbeam.test:3341"]
            .contains(&a.relay.as_str())
    { bail!("unsupported role, variant, or relay"); }
    Ok(a)
}

fn atomic_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}

async fn resolve(relay: &str) -> Result<(RelayModeOption, bool)> {
    let arg = RelayConfigArg {
        mode: if relay == "disabled" { "disabled" } else { "custom" }.into(),
        urls: if relay == "disabled" { vec![] } else { vec![relay.into()] },
        auth_token: None,
        fallback: Some("strict".into()),
    };
    resolve_relay_mode_with_fallback(Some(arg)).await.map_err(|_| anyhow!("relay resolution failed"))
}

fn discovery() -> Result<engine::DiscoveryModeOption> {
    build_discovery_mode(Some(DiscoveryConfigArg {
        mode: "custom".into(),
        pkarr_relay_url: Some("https://relay.dashbeam.test:3341/pkarr".into()),
        dns_origin: None,
    })).map_err(|_| anyhow!("discovery config failed"))
}

fn source_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() { bail!("symlink fixtures unsupported"); }
        if meta.is_file() { files.push(path.to_owned()); }
        else if meta.is_dir() {
            for e in fs::read_dir(path)? { visit(&e?.path(), files)?; }
        } else { bail!("fixture must contain only files and directories"); }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn metadata(source: &Path) -> Result<FileMetadata> {
    let files = source_files(source)?;
    let size = files.iter().try_fold(0u64, |total, path| -> Result<u64> {
        Ok(total.checked_add(fs::metadata(path)?.len()).context("size overflow")?)
    })?;
    Ok(FileMetadata {
        file_name: source.file_name().context("fixture name missing")?.to_string_lossy().into_owned(),
        item_count: 1,
        size,
        thumbnail: None,
        // This benchmark accepts binary files or directories, without GUI thumbnails.
        mime_type: Some(if source.is_dir() { "inode/directory" } else { "application/octet-stream" }.into()),
        items: None,
        content_kind: None,
    })
}

async fn close_share(share: SendResult) -> Result<()> {
    let dir = share.blobs_data_dir.path().to_owned();
    share.router.shutdown().await.map_err(|_| anyhow!("router shutdown failed"))?;
    share.router.endpoint().close().await;
    drop(share);
    let until = Instant::now() + Duration::from_secs(15);
    while dir.exists() {
        if Instant::now() > until { bail!("send store cleanup timeout"); }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

async fn send(a: &Args) -> Result<()> {
    let intent = now_ns();
    let meta = metadata(&a.source)?;
    let metadata_prepared = now_ns();
    let (relay_mode, fell_back) = resolve(&a.relay).await?;
    let relay_resolved = now_ns();
    let options = SendOptions {
        relay_mode,
        discovery_mode: discovery()?,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        magic_ipv4_addr: None,
        magic_ipv6_addr: None,
    };
    let recorder = Arc::new(Recorder(Mutex::new(Events::default())));
    let emitter: AppHandle = Some(recorder.clone());
    engine::benchmark_set_observer(emitter.clone());
    let share = start_share_items(vec![a.source.clone()], options, &emitter, Some(meta))
        .await.map_err(|_| anyhow!("share start failed"))?;
    let ticket_ready = now_ns();
    let connected_home_relays: Vec<String> = share.router.endpoint().home_relay_status().get()
        .into_iter().filter(|status| status.is_connected())
        .map(|status| status.url().to_string()).collect();
    atomic_private(&a.shared.join("ticket"), share.ticket.as_bytes())?;
    let stages = recorder.0.lock().map_err(|_| anyhow!("event lock"))?.stages.clone();
    atomic_private(&a.shared.join("sender.json"), serde_json::to_string(&json!({
        "variant": a.variant, "role": "send", "share_intent_ns": intent,
        "metadata_prepared_ns": metadata_prepared, "relay_resolved_ns": relay_resolved,
        "ticket_ready_ns": ticket_ready, "relay_fell_back": fell_back,
        "imported_bytes": share.size, "stages": stages,
        "connected_home_relays": connected_home_relays
    }))?.as_bytes())?;
    let until = Instant::now() + Duration::from_secs(900);
    while !a.shared.join("done").exists() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let completed = a.shared.join("done").exists();
    close_share(share).await?;
    if !completed { bail!("receiver done timeout"); }
    Ok(())
}

#[derive(Default)]
struct Events {
    first_nonzero_progress_proxy_ns: Option<u64>,
    first_payload_progress_proxy_ns: Option<u64>,
    engine_completion_event_ns: Option<u64>,
    stages: BTreeMap<String, u64>,
}
struct Recorder(Mutex<Events>);
impl EventEmitter for Recorder {
    fn emit_event(&self, name: &str) -> std::result::Result<(), String> {
        let mut e = self.0.lock().map_err(|_| "event lock")?;
        let timestamp = now_ns();
        if name.starts_with("benchmark-") {
            e.stages.entry(name.to_string()).or_insert(timestamp);
        }
        if name == "benchmark-first-payload-progress" && e.first_payload_progress_proxy_ns.is_none() {
            e.first_payload_progress_proxy_ns = Some(timestamp);
        }
        if name == "receive-completed" {
            e.engine_completion_event_ns = Some(timestamp);
        }
        Ok(())
    }
    fn emit_event_with_payload(&self, name: &str, payload: &str) -> std::result::Result<(), String> {
        let mut e = self.0.lock().map_err(|_| "event lock")?;
        let timestamp = now_ns();
        if name.starts_with("benchmark-") {
            e.stages.entry(name.to_string()).or_insert(timestamp);
        }
        if name == "receive-progress"
            && payload.split(':').next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) > 0
            && e.first_nonzero_progress_proxy_ns.is_none()
        { e.first_nonzero_progress_proxy_ns = Some(timestamp); }
        if name == "benchmark-first-payload-progress" && e.first_payload_progress_proxy_ns.is_none() {
            e.first_payload_progress_proxy_ns = Some(timestamp);
        }
        if name == "receive-completed" { e.engine_completion_event_ns = Some(timestamp); }
        Ok(())
    }
}

fn compare(a: &Path, b: &Path) -> Result<()> {
    let mut left = BufReader::new(File::open(a)?);
    let mut right = BufReader::new(File::open(b)?);
    let mut x = [0u8; 65536];
    let mut y = [0u8; 65536];
    loop {
        let n = left.read(&mut x)?;
        if n == 0 {
            if right.read(&mut y[..1])? != 0 { bail!("export longer than source"); }
            return Ok(());
        }
        right.read_exact(&mut y[..n]).context("export shorter than source")?;
        if x[..n] != y[..n] { bail!("export bytes differ"); }
    }
}

fn verify(source: &Path, result: &engine::ReceiveResult) -> Result<(usize, u64)> {
    let files = source_files(source)?;
    if files.len() != result.exported_files.len() { bail!("export file count differs"); }
    let parent = source.parent().context("fixture parent missing")?;
    let mut mapped = BTreeMap::new();
    for file in &result.exported_files {
        if mapped.insert(file.collection_name.clone(), &file.path).is_some() { bail!("duplicate export"); }
    }
    let mut bytes = 0u64;
    for path in &files {
        let name = path.strip_prefix(parent)?.to_str().context("non UTF-8 fixture")?.replace('\\', "/");
        let exported = mapped.get(&name).context("export name mismatch")?;
        compare(path, exported)?;
        bytes += fs::metadata(path)?.len();
    }
    Ok((files.len(), bytes))
}

async fn recv(a: &Args) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let ticket_path = a.shared.join("ticket");
    while !ticket_path.exists() {
        if Instant::now() >= deadline { bail!("ticket wait timeout"); }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let ticket = fs::read_to_string(ticket_path)?;
    let intent = now_ns();
    let (relay_mode, fell_back) = resolve(&a.relay).await?;
    let relay_resolved = now_ns();
    let discovery_mode = discovery()?;
    let metadata_options = ReceiveOptions {
        output_dir: None, relay_mode: relay_mode.clone(), discovery_mode: discovery_mode.clone(),
        magic_ipv4_addr: None, magic_ipv6_addr: None,
    };
    let output = a.shared.join("output");
    fs::create_dir(&output)?;
    let options = ReceiveOptions {
        output_dir: Some(output), relay_mode, discovery_mode,
        magic_ipv4_addr: None, magic_ipv6_addr: None,
    };
    let recorder = Arc::new(Recorder(Mutex::new(Events::default())));
    let emitter: AppHandle = Some(recorder.clone());
    engine::benchmark_set_observer(emitter.clone());
    let metadata_times = Arc::new(Mutex::new((None, None)));
    let times = metadata_times.clone();
    let metadata_ticket = ticket.clone();
    let stall_ms = a.stall_ms;
    let advisory = async move {
        times.lock().unwrap().0 = Some(now_ns());
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            if stall_ms > 0 {
                // Synthetic advisory delay; neither payload nor relay is throttled.
                tokio::time::sleep(Duration::from_millis(stall_ms)).await;
                None
            } else {
                fetch_metadata(metadata_ticket, metadata_options).await.ok()
            }
        }).await.ok().flatten();
        times.lock().unwrap().1 = Some(now_ns());
        result
    };
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let work = async { download(ticket, options, emitter, cancel_rx).await };
    let (result, metadata) = if a.variant == "baseline" {
        let m = advisory.await;
        (work.await, m)
    } else {
        // Equivalent to alongside_advisory for an uncancelled benchmark sample.
        tokio::pin!(work);
        tokio::pin!(advisory);
        tokio::select! {
            r = &mut work => {
                let m = if r.is_ok() { advisory.await } else { None };
                (r, m)
            }
            m = &mut advisory => (work.await, m),
        }
    };
    let return_ns = now_ns();
    let result = result.map_err(|_| anyhow!("download failed"))?;
    let (count, bytes) = verify(&a.source, &result)?;
    let verified_ns = now_ns();
    let events = recorder.0.lock().map_err(|_| anyhow!("event lock"))?;
    let times = metadata_times.lock().map_err(|_| anyhow!("metadata lock"))?;
    Ok(json!({
        "variant": a.variant, "role": "recv", "receive_intent_ns": intent,
        "relay_resolved_ns": relay_resolved, "relay_fell_back": fell_back,
        "metadata_start_ns": times.0, "metadata_end_ns": times.1,
        "metadata_received": metadata.is_some(), "synthetic_metadata_stall_ms": stall_ms,
        "first_nonzero_receive_progress_proxy_ns": events.first_nonzero_progress_proxy_ns,
        "first_payload_progress_proxy_ns": events.first_payload_progress_proxy_ns,
        "engine_completion_event_ns": events.engine_completion_event_ns,
        "stages": events.stages,
        "receive_return_ns": return_ns, "verification_return_ns": verified_ns,
        "verified_files": count, "verified_bytes": bytes, "bytes_match": true,
        "scope": "real engine with simulated app orchestration; progress is not first wire byte"
    }))
}

async fn run(a: Args) -> Result<()> {
    fs::create_dir_all(&a.shared)?;
    let store_root = a.shared.join(format!("store-{}", a.role));
    fs::create_dir(&store_root)?;
    engine::storage::TEMP_DIR.set(store_root).map_err(|_| anyhow!("store root already set"))?;
    if a.role == "send" { send(&a).await }
    else {
        let result = recv(&a).await;
        let value = match &result {
            Ok(v) => v.clone(),
            Err(_) => json!({"variant": a.variant, "role": "recv", "success": false}),
        };
        atomic_private(&a.shared.join("receiver.json"), serde_json::to_string(&value)?.as_bytes())?;
        atomic_private(&a.shared.join("done"), b"done")?;
        result.map(|_| ())
    }
}

fn main() -> Result<()> {
    // Ensure ephemeral identities are used; no account or pairing state.
    std::env::remove_var("IROH_SECRET");
    let a = args()?;
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?.block_on(run(a))
}
