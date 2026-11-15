//! Bounded native peer for installed-app acceptance. First stdin line is
//! {"relay": RelayConfigArg, "discovery": DiscoveryConfigArg}; neither it nor
//! bearer tickets are logged. Subsequent lines are JSON commands (see --help).
//! Run only in a new private task directory. Identity and stores are temporary.
use anyhow::{ensure, Context, Result};
use clap::Parser;
use engine::{
    AppHandle, Discoverability, EventEmitter, FileMetadata, NodeService,
    PairingStatus, ReceiveOptions, RegisteredStreamShare, SendOptions, SendResult,
    StreamLimits, StreamTicket, TEXT_CONTENT_KIND,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{io::AsyncBufReadExt, sync::{mpsc, oneshot}, task::JoinHandle};

#[derive(Parser)]
#[command(about = "Temporary paired peer. stdin: config then host/join/status/send/arm/cancel/stop/forget/quit JSON commands.")]
struct Args {
    /// Must not exist yet; its parent must exist. Tickets/fixtures/results live here.
    #[arg(long)]
    work_dir: PathBuf,
    #[arg(long, default_value = "DashBeam DEV temporary probe")]
    name: String,
    #[arg(long, default_value_t = 1800, value_parser = clap::value_parser!(u64).range(60..=3600))]
    ttl_secs: u64,
}

#[derive(Clone)]
struct Fixture {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

struct Event {
    name: String,
    payload: Option<String>,
}

struct ProbeEmitter(mpsc::UnboundedSender<Event>);
impl EventEmitter for ProbeEmitter {
    fn emit_event(&self, name: &str) -> Result<(), String> {
        self.emit_event_with_payload(name, "")
    }
    fn emit_event_with_payload(&self, name: &str, payload: &str) -> Result<(), String> {
        if matches!(name, "paired-invite-received" | "paired-invite-response" | "device-paired" | "device-node-network-ready"
            | "transfer-started" | "transfer-completed" | "transfer-failed" | "receive-started") {
            let _ = self.0.send(Event {
                name: name.into(),
                payload: (!payload.is_empty()).then(|| payload.into()),
            });
        }
        Ok(())
    }
}

enum Share {
    Legacy(SendResult),
    Stream(RegisteredStreamShare),
}
impl Share {
    async fn close(self) {
        match self {
            Self::Legacy(share) => {
                let _ = tokio::time::timeout(Duration::from_secs(5), share.router.shutdown()).await;
                share.router.endpoint().close().await;
            }
            Self::Stream(share) => share.shutdown().await,
        }
    }
    fn completed(&self) -> u64 {
        match self {
            Self::Legacy(share) => share.completed_peers() as u64,
            Self::Stream(share) => share.completed_transfers(),
        }
    }
}

#[derive(Clone)]
struct Armed {
    fixture: String,
    cancel_after_ms: Option<u64>,
}
struct ActiveReceive {
    cancel: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    task: JoinHandle<Value>,
}

fn report(value: Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}

fn error_kind(error: &anyhow::Error) -> &'static str {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("cancel") { "cancelled" }
    else if message.contains("timeout") || message.contains("timed out") { "timeout" }
    else if message.contains("connect") { "connection" }
    else if message.contains("verif") || message.contains("digest") { "verification" }
    else { "operation_failed" }
}

fn private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

async fn sha256(path: &Path) -> Result<String> {
    let output = tokio::process::Command::new("sha256sum").arg("--").arg(path).output().await?;
    ensure!(output.status.success(), "hash command failed");
    let result = String::from_utf8(output.stdout)?;
    let hash = result.split_whitespace().next().context("missing hash")?;
    ensure!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()), "invalid hash output");
    Ok(hash.to_owned())
}

async fn fixtures(root: &Path) -> Result<BTreeMap<String, Fixture>> {
    let dir = root.join("fixtures");
    private_dir(&dir)?;
    let mut values = BTreeMap::new();
    for (key, name, bytes) in [
        ("empty", "empty.bin", 0u64),
        ("small", "DEV 中文 spaces.bin", 32 * 1024),
        ("large", "DEV 8MiB.bin", 8 * 1024 * 1024),
        ("cancel", "DEV 64MiB cancel.bin", 64 * 1024 * 1024),
    ] {
        let path = dir.join(name);
        private_write(&path, &[])?;
        let mut file = OpenOptions::new().write(true).open(&path)?;
        let buffer: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
        let mut remaining = bytes;
        while remaining > 0 {
            let len = remaining.min(buffer.len() as u64) as usize;
            file.write_all(&buffer[..len])?;
            remaining -= len as u64;
        }
        file.sync_all()?;
        values.insert(key.into(), Fixture { sha256: sha256(&path).await?, path, bytes });
    }
    let text_path = dir.join("DashBeam Text.txt");
    private_write(&text_path, "DashBeam 临时实机验收\nplain **markdown**\nline 3\n".as_bytes())?;
    values.insert("text".into(), Fixture {
        bytes: std::fs::metadata(&text_path)?.len(),
        sha256: sha256(&text_path).await?,
        path: text_path,
    });
    Ok(values)
}

fn fixture_group(fixtures: &BTreeMap<String, Fixture>, name: &str) -> Result<Vec<Fixture>> {
    if name == "multi" {
        return Ok(vec![fixtures["small"].clone(), fixtures["text"].clone()]);
    }
    Ok(vec![fixtures.get(name).context("unknown fixture")?.clone()])
}

fn ticket_file(root: &Path, value: &Value) -> Result<String> {
    let name = value.get("ticket_file").and_then(Value::as_str).context("ticket_file required")?;
    let path = root.join(name);
    let metadata = std::fs::symlink_metadata(&path)?;
    ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "invalid ticket file");
    ensure!(path.canonicalize()?.starts_with(root), "ticket file outside task");
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        ensure!(metadata.permissions().mode() & 0o077 == 0, "ticket file must be private");
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(8193).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8192, "ticket too large");
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}

fn selected_peer(node: &NodeService, command: &Value) -> Result<String> {
    let index = command.get("peer").and_then(Value::as_u64).unwrap_or(0) as usize;
    let devices = node.list_paired()?;
    let device = devices.get(index).context("peer index unavailable")?;
    ensure!(device.pairing_status == PairingStatus::Active, "peer is not active");
    Ok(device.endpoint_id.clone())
}

fn limits() -> StreamLimits {
    StreamLimits { max_bytes: 128 * 1024 * 1024, whole_timeout: Duration::from_secs(300),
        ..Default::default() }
}

async fn receive_invite(
    node: Arc<NodeService>, invite: Value, expectation: Armed, expected: Vec<Fixture>,
    output: PathBuf, network: ReceiveOptions, emitter: AppHandle,
    cancel: Arc<Mutex<Option<oneshot::Sender<()>>>>, cancel_rx: oneshot::Receiver<()>,
) -> Value {
    let started = Instant::now();
    let ticket = invite["blob_ticket"].as_str().unwrap_or_default();
    let remote = invite["remote_endpoint_id"].as_str().unwrap_or_default();
    let stream = StreamTicket::is_stream_token(ticket);
    let timer_cancel = cancel.clone();
    let cancel_delay = expectation.cancel_after_ms.unwrap_or(300_000).min(300_000);
    let timer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(cancel_delay)).await;
        if let Some(cancel) = timer_cancel.lock().unwrap().take() { let _ = cancel.send(()); }
    });
    let mut telemetry = json!({});
    let received = async {
        node.respond_paired_invite(remote, true).await?;
        if stream {
            let ticket = StreamTicket::from_token(ticket)?;
            let mut receiver = node.stream_receiver(limits()).await?;
            let result = receiver.receive_with_emitter(&ticket, output.clone(), cancel_rx, emitter).await;
            receiver.shutdown().await;
            let result = result?;
            telemetry = json!({
                "first_payload_us": result.first_payload_elapsed.map(|d| d.as_micros() as u64),
                "committed_us": result.committed_elapsed.as_micros() as u64,
                "ack": result.completion_ack_sent,
                "initial_path": format!("{:?}", result.initial_path),
                "final_path": format!("{:?}", result.final_path),
            });
            Ok::<_, anyhow::Error>(vec![result.path])
        } else {
            let result = engine::download(ticket.into(), ReceiveOptions {
                output_dir: Some(output.clone()), ..network
            }, emitter, cancel_rx).await?;
            Ok(result.exported_files.into_iter().map(|file| file.path).collect())
        }
    }.await;
    timer.abort();
    let node_alive = node.is_network_ready();
    match received {
        Ok(paths) => {
            let mut actual = Vec::new();
            for path in &paths {
                match sha256(path).await {
                    Ok(hash) => actual.push((std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX), hash)),
                    Err(_) => return json!({"event":"receive-result","ok":false,"kind":"hash_failed"}),
                }
            }
            let mut expected_hashes: Vec<_> = expected.iter().map(|f| (f.bytes, f.sha256.clone())).collect();
            actual.sort(); expected_hashes.sort();
            json!({"event":"receive-result","ok":actual == expected_hashes,"fixture":expectation.fixture,
                "mode":if stream {"stream"} else {"legacy"},"files":paths.len(),
                "bytes":actual.iter().map(|(bytes,_)|bytes).sum::<u64>(),
                "elapsed_ms":started.elapsed().as_millis() as u64,"telemetry":telemetry,
                "node_alive":node_alive,"output_dir":output})
        }
        Err(error) => {
            let entries = std::fs::read_dir(&output).map(|entries| entries.count()).unwrap_or(usize::MAX);
            let kind = error_kind(&error);
            json!({"event":"receive-result","ok":false,"kind":kind,"fixture":expectation.fixture,
                "mode":if stream {"stream"} else {"legacy"},"cancel_expected":expectation.cancel_after_ms.is_some(),
                "output_entries":entries,"partial_clean":stream && entries == 0,
                "elapsed_ms":started.elapsed().as_millis() as u64,"node_alive":node_alive})
        }
    }
}

async fn session(args: Args) -> Result<()> {
    private_dir(&args.work_dir).context("work directory must be new")?;
    let root = args.work_dir.canonicalize()?;
    let identity = tempfile::Builder::new().prefix("identity-").tempdir_in(&root)?;
    let stores = tempfile::Builder::new().prefix("stores-").tempdir_in(&root)?;
    engine::storage::TEMP_DIR.set(stores.path().to_owned()).map_err(|_| anyhow::anyhow!("store root already set"))?;
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let config_line = tokio::time::timeout(Duration::from_secs(60), lines.next_line()).await??
        .context("first stdin line must be network JSON")?;
    ensure!(config_line.len() <= 32768, "configuration too large");
    let config: Value = serde_json::from_str(&config_line).map_err(|_| anyhow::anyhow!("invalid configuration JSON"))?;
    let relay_arg = config.get("relay").filter(|v| !v.is_null()).map(|v| serde_json::from_value(v.clone())).transpose()
        .map_err(|_| anyhow::anyhow!("invalid relay configuration"))?;
    let discovery_arg = config.get("discovery").filter(|v| !v.is_null()).map(|v| serde_json::from_value(v.clone())).transpose()
        .map_err(|_| anyhow::anyhow!("invalid discovery configuration"))?;
    let relay = engine::build_relay_mode(relay_arg).map_err(|_| anyhow::anyhow!("invalid relay configuration"))?;
    let discovery = engine::build_discovery_mode(discovery_arg).map_err(|_| anyhow::anyhow!("invalid discovery configuration"))?;
    drop(config); drop(config_line);
    let fixtures = fixtures(&root).await?;
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let emitter: AppHandle = Some(Arc::new(ProbeEmitter(events_tx)));
    let node = Arc::new(tokio::time::timeout(Duration::from_secs(60), NodeService::start(
        identity.path(), relay.clone().into(), discovery.clone(), Discoverability::PairedOnly, emitter.clone(),
    )).await??);
    node.set_device_display_name(&args.name)?;
    report(json!({"event":"ready","name":args.name,"ttl_secs":args.ttl_secs,"node_alive":node.is_network_ready(),
        "fixtures":fixtures.iter().map(|(key,f)| json!({"fixture":key,"bytes":f.bytes,"sha256":f.sha256,
            "file":f.path.strip_prefix(&root).unwrap()})).collect::<Vec<_>>()}));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(args.ttl_secs);
    let mut share: Option<Share> = None;
    let mut armed: Option<Armed> = None;
    let mut receiving: Option<ActiveReceive> = None;
    let mut serial = 0u64;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => { report(json!({"event":"deadline"})); break; }
            _ = tokio::signal::ctrl_c() => break,
            result = async { (&mut receiving.as_mut().unwrap().task).await }, if receiving.is_some() => {
                receiving = None;
                report(result.unwrap_or_else(|_| json!({"event":"receive-result","ok":false,"kind":"task_failed"})));
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                if event.name == "paired-invite-received" {
                    let invite: Value = match event.payload.as_deref().map(serde_json::from_str) {
                        Some(Ok(value)) => value, _ => continue,
                    };
                    if receiving.is_some() || armed.is_none() || invite["total_size"].as_u64().unwrap_or(u64::MAX) > 128*1024*1024 {
                        let _ = node.respond_paired_invite(invite["remote_endpoint_id"].as_str().unwrap_or_default(), false).await;
                        report(json!({"event":"invite-declined","reason":"not_armed_or_busy_or_budget"}));
                        continue;
                    }
                    let expectation = armed.take().unwrap();
                    let expected = fixture_group(&fixtures, &expectation.fixture)?;
                    serial += 1;
                    let output = root.join(format!("received-{serial:04}"));
                    private_dir(&output)?;
                    let (cancel_tx, cancel_rx) = oneshot::channel();
                    let cancel = Arc::new(Mutex::new(Some(cancel_tx)));
                    let task = tokio::spawn(receive_invite(node.clone(), invite, expectation, expected,
                        output, ReceiveOptions {relay_mode:relay.clone(), discovery_mode:discovery.clone(), ..Default::default()},
                        emitter.clone(), cancel.clone(), cancel_rx));
                    receiving = Some(ActiveReceive { cancel, task });
                    report(json!({"event":"invite-accepted"}));
                } else {
                    let payload: Value = event.payload.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
                    report(json!({"event":event.name,"duration_ms":payload.get("durationMs"),
                        "bytes":payload.get("bytes"),"response":payload.get("response")}));
                }
            }
            command = lines.next_line() => {
                let Some(command) = command? else { break };
                let command: Value = match serde_json::from_str(&command) {
                    Ok(value) => value, Err(_) => { report(json!({"event":"command-error","kind":"invalid_json"})); continue; }
                };
                let op = command["op"].as_str().unwrap_or_default();
                if op == "quit" { break; }
                let result: Result<Value> = async {
                    match op {
                        "host" => {
                            let ticket = node.start_pairing_host(Some(300)).await?;
                            serial += 1;
                            let name = format!("pairing-{serial:04}.ticket");
                            private_write(&root.join(&name), ticket.as_bytes())?;
                            Ok(json!({"event":"pairing-host","ticket_file":name,"ttl_secs":300}))
                        }
                        "join" => {
                            let ticket = ticket_file(&root, &command)?;
                            tokio::time::timeout(Duration::from_secs(45), node.join_pairing(&ticket)).await??;
                            Ok(json!({"event":"pairing-joined","ok":true}))
                        }
                        "status" => {
                            let peers = node.list_paired()?;
                            Ok(json!({"event":"status","node_alive":node.is_network_ready(),"receiving":receiving.is_some(),
                                "share_completed":share.as_ref().map(Share::completed),
                                "peers":peers.iter().enumerate().map(|(index,p)|json!({"index":index,
                                    "active":p.pairing_status == PairingStatus::Active,"online":p.online})).collect::<Vec<_>>()}))
                        }
                        "arm" => {
                            ensure!(receiving.is_none(), "already receiving");
                            let fixture = command["fixture"].as_str().context("fixture required")?;
                            fixture_group(&fixtures, fixture)?;
                            armed = Some(Armed { fixture:fixture.into(), cancel_after_ms:command["cancel_after_ms"].as_u64() });
                            Ok(json!({"event":"armed","fixture":fixture,"cancel_after_ms":command.get("cancel_after_ms")}))
                        }
                        "cancel" => {
                            if let Some(active) = &receiving {
                                if let Some(cancel) = active.cancel.lock().unwrap().take() { let _ = cancel.send(()); }
                            }
                            Ok(json!({"event":"cancel-requested"}))
                        }
                        "stop" => {
                            if let Some(previous) = share.take() { previous.close().await; }
                            Ok(json!({"event":"share-stopped","node_alive":node.is_network_ready()}))
                        }
                        "forget" => {
                            node.forget_paired(&selected_peer(&node, &command)?).await?;
                            Ok(json!({"event":"peer-forgotten"}))
                        }
                        "send" => {
                            ensure!(share.is_none(), "stop current share first");
                            let kind = command["kind"].as_str().context("kind required")?;
                            let fixture_name = command["fixture"].as_str().context("fixture required")?;
                            let selected = fixture_group(&fixtures, fixture_name)?;
                            ensure!(matches!(kind, "legacy" | "text" | "stream"), "invalid kind");
                            ensure!(kind != "stream" || selected.len() == 1, "stream requires one file");
                            ensure!(kind != "text" || fixture_name == "text", "text requires text fixture");
                            let remote = selected_peer(&node, &command)?;
                            let bytes: u64 = selected.iter().map(|f|f.bytes).sum();
                            let start = Instant::now();
                            let (next_share, ticket, zero_reads) = if kind == "stream" {
                                let result = node.start_stream_share_with_emitter(selected[0].path.clone(), limits(), emitter.clone()).await?;
                                let zero = result.source_bytes_read() == 0;
                                let ticket = result.ticket().to_token()?;
                                (Share::Stream(result), ticket, Some(zero))
                            } else {
                                let metadata = FileMetadata {
                                    file_name:selected[0].path.file_name().unwrap().to_string_lossy().into_owned(),
                                    item_count:selected.len() as u32, size:bytes, thumbnail:None, items:None,
                                    mime_type:Some(if kind == "text" {"text/plain"} else {"application/octet-stream"}.into()),
                                    content_kind:(kind == "text").then(||TEXT_CONTENT_KIND.into()),
                                };
                                let result = tokio::time::timeout(Duration::from_secs(90), engine::start_share_items(
                                    selected.iter().map(|f|f.path.clone()).collect(),
                                    SendOptions {relay_mode:relay.clone(),discovery_mode:discovery.clone(),..Default::default()},
                                    &emitter, Some(metadata))).await??;
                                let ticket = result.ticket.clone();
                                (Share::Legacy(result), ticket, None)
                            };
                            let prepare_us = start.elapsed().as_micros() as u64;
                            share = Some(next_share);
                            serial += 1;
                            let name = format!("share-{serial:04}.ticket");
                            private_write(&root.join(&name), ticket.as_bytes())?;
                            let delivered = node.invite_paired_device(&remote, &ticket, selected.len() as u32, bytes).await?;
                            Ok(json!({"event":"share-invited","kind":kind,"fixture":fixture_name,"delivered":delivered,
                                "prepare_us":prepare_us,"zero_prepare_reads":zero_reads,"ticket_file":name}))
                        }
                        _ => anyhow::bail!("unknown command"),
                    }
                }.await;
                match result { Ok(value) => report(value), Err(error) => report(json!({"event":"command-error","op":op,"kind":error_kind(&error)})) }
            }
        }
    }
    if let Some(active) = receiving {
        if let Some(cancel) = active.cancel.lock().unwrap().take() { let _ = cancel.send(()); }
        let mut task = active.task;
        match tokio::time::timeout(Duration::from_secs(10), &mut task).await {
            Ok(Ok(result)) => report(result),
            _ => task.abort(),
        }
    }
    if let Some(share) = share { share.close().await; }
    node.stop_pairing_host().await;
    node.shutdown().await?;
    drop(node);
    identity.close()?;
    stores.close()?;
    report(json!({"event":"stopped","identity_retained":false,"tickets_revoked":true}));
    Ok(())
}

fn main() {
    let args = Args::parse();
    // Set before Tokio creates any worker thread. This prevents reading or
    // synchronizing the host's real DashBeam keyring, even if it has one.
    if std::env::var_os("IROH_SECRET").is_some() {
        report(json!({"event":"fatal","kind":"existing_identity_override"}));
        std::process::exit(2);
    }
    let secret = iroh::SecretKey::generate();
    std::env::set_var("IROH_SECRET", data_encoding::HEXLOWER.encode(&secret.to_bytes()));
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    if let Err(error) = runtime.block_on(session(args)) {
        report(json!({"event":"fatal","kind":error_kind(&error)}));
        std::process::exit(1);
    }
}
