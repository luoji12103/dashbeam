//! Isolated engine-only diagnostic. Copy to engine/examples/share_start_probe.rs.
//! This does not exercise the Tauri commands, UI, metadata, SAF, or relay.
//! Relay is disabled; the engine's default endpoint/discovery behavior is unchanged.
//! No ticket, hash, endpoint identity, secret, or fixture contents are printed.

use anyhow::{anyhow, bail, Context, Result};
use engine::{
    download, start_share_items, AddrInfoOptions, AppHandle, EventEmitter, ReceiveOptions,
    RelayModeOption, SendOptions, SendResult,
};
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MIB: u64 = 1024 * 1024;
const BUFFER_SIZE: usize = 64 * 1024;

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

// Every byte is written, rather than using set_len or seeking. The deterministic
// stream avoids a large allocation and makes exported-byte verification possible.
// Filesystem compression/dedup and OS cache state remain environment-dependent.
fn write_fixture(path: &Path, size: u64, seed: u64) -> Result<()> {
    let mut file = File::create(path)?;
    let mut buf = [0u8; BUFFER_SIZE];
    let mut state = seed;
    let mut left = size;
    while left > 0 {
        let len = left.min(BUFFER_SIZE as u64) as usize;
        for byte in &mut buf[..len] {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = (state >> 24) as u8;
        }
        file.write_all(&buf[..len])?;
        left -= len as u64;
    }
    file.sync_all()?;
    Ok(())
}

fn send_options() -> SendOptions {
    SendOptions {
        relay_mode: RelayModeOption::Disabled,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        ..Default::default()
    }
}

// The engine uses an asynchronous cleanup guard. Explicitly shut down first,
// then wait for the guard to remove this share's store before the next sample.
async fn close_share(share: SendResult) -> Result<()> {
    let store_dir = share.blobs_data_dir.path().to_owned();
    share.router.shutdown().await.context("router shutdown")?;
    share.router.endpoint().close().await;
    // Router shutdown invokes the blobs protocol's store shutdown. Sending a
    // second shutdown over its closed local actor channel is not an error in
    // the transfer under test, so do not issue a duplicate request here.
    drop(share);
    let deadline = Instant::now() + Duration::from_secs(15);
    while store_dir.exists() {
        if Instant::now() >= deadline {
            bail!("send store cleanup did not finish within 15 seconds");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

async fn share_sample(root: &Path, bytes: u64, many_small: bool) -> Result<Value> {
    let fixture_dir = tempfile::Builder::new().prefix("fixture-").tempdir_in(root)?;
    let start = Instant::now();
    let paths = if many_small {
        let dir = fixture_dir.path().join("many-small");
        fs::create_dir(&dir)?;
        for n in 0..256u64 {
            write_fixture(&dir.join(format!("{n:04}.bin")), 4096, n + 1)?;
        }
        vec![dir]
    } else {
        let path = fixture_dir.path().join("payload.bin");
        write_fixture(&path, bytes, 0x1234_5678_9abc_def1)?;
        vec![path]
    };
    let fixture_generation_ms = ms(start);
    let start = Instant::now();
    let share = start_share_items(paths, send_options(), &None, None).await
        .map_err(|_| anyhow!("start_share_items failed"))?;
    let share_prepare_ms = ms(start);
    let actual_bytes = share.size;
    let expected = if many_small { 256 * 4096 } else { bytes };
    if actual_bytes != expected {
        close_share(share).await?;
        bail!("unexpected imported byte count");
    }
    close_share(share).await?;
    Ok(json!({"fixture_generation_ms": fixture_generation_ms,
              "share_prepare_ms": share_prepare_ms, "imported_bytes": actual_bytes}))
}

#[derive(Default)]
struct Events {
    start: Option<Instant>,
    first_progress_ms: Option<f64>,
    first_nonzero_progress_ms: Option<f64>,
    completion_ms: Option<f64>,
    transport_duration_ms: Option<u64>,
    export_duration_ms: Option<u64>,
}
struct Recorder(Mutex<Events>);
impl EventEmitter for Recorder {
    fn emit_event(&self, _name: &str) -> std::result::Result<(), String> { Ok(()) }
    fn emit_event_with_payload(&self, name: &str, payload: &str) -> std::result::Result<(), String> {
        let mut e = self.0.lock().map_err(|_| "event lock poisoned".to_string())?;
        let elapsed = e.start.map(ms);
        let v: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        if name == "receive-progress" {
            if e.first_progress_ms.is_none() { e.first_progress_ms = elapsed; }
            // The engine sends an initial zero-byte progress event before execute_get.
            if payload.split(':').next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) > 0
                && e.first_nonzero_progress_ms.is_none() {
                e.first_nonzero_progress_ms = elapsed;
            }
        } else if name == "receive-completed" {
            e.completion_ms = elapsed;
            e.transport_duration_ms = v.get("durationMs").and_then(Value::as_u64);
            e.export_duration_ms = v.get("exportMs").and_then(Value::as_u64);
        }
        Ok(())
    }
}

fn compare_files(source: &Path, target: &Path) -> Result<()> {
    let mut a = BufReader::new(File::open(source)?);
    let mut b = BufReader::new(File::open(target)?);
    let mut left = [0u8; BUFFER_SIZE];
    let mut right = [0u8; BUFFER_SIZE];
    loop {
        let n = a.read(&mut left)?;
        if n == 0 {
            if b.read(&mut right[..1])? != 0 { bail!("export longer than source"); }
            break;
        }
        b.read_exact(&mut right[..n]).context("export shorter than source")?;
        if left[..n] != right[..n] { bail!("exported bytes differ"); }
    }
    Ok(())
}

async fn e2e(root: &Path) -> Result<Value> {
    let fixture_dir = tempfile::Builder::new().prefix("e2e-").tempdir_in(root)?;
    let source = fixture_dir.path().join("payload.bin");
    let output = fixture_dir.path().join("output");
    fs::create_dir(&output)?;
    let start = Instant::now();
    write_fixture(&source, 64 * MIB, 0x1234_5678_9abc_def1)?;
    let fixture_generation_ms = ms(start);
    let total_start = Instant::now();
    let share = start_share_items(vec![source.clone()], send_options(), &None, None).await
        .map_err(|_| anyhow!("e2e start_share_items failed"))?;
    let share_prepare_ms = ms(total_start);
    let recorder = Arc::new(Recorder(Mutex::new(Events { start: Some(total_start), ..Default::default() })));
    let emitter: AppHandle = Some(recorder.clone());
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let receive_start = Instant::now();
    let received = download(
        share.ticket.clone(),
        ReceiveOptions { output_dir: Some(output.clone()), relay_mode: RelayModeOption::Disabled, ..Default::default() },
        emitter, cancel_rx,
    ).await;
    let receive_call_ms = ms(receive_start);
    drop(cancel_tx);
    let result = match received {
        Ok(result) => result,
        Err(_) => { close_share(share).await?; bail!("e2e download failed"); }
    };
    if result.exported_files.len() != 1 { close_share(share).await?; bail!("unexpected export count"); }
    let verify_start = Instant::now();
    let comparison = compare_files(&source, &output.join("payload.bin"));
    let verification_ms = ms(verify_start);
    close_share(share).await?;
    comparison?;
    let events = recorder.0.lock().map_err(|_| anyhow!("event lock poisoned"))?;
    Ok(json!({"fixture_generation_ms": fixture_generation_ms,
              "share_prepare_ms": share_prepare_ms, "receive_call_ms": receive_call_ms,
              "first_progress_event_from_share_start_ms": events.first_progress_ms,
              "first_nonzero_progress_event_from_share_start_ms": events.first_nonzero_progress_ms,
              "completed_event_from_share_start_ms": events.completion_ms,
              "engine_reported_payload_get_duration_ms": events.transport_duration_ms,
              "engine_reported_export_duration_ms": events.export_duration_ms,
              "verification_ms": verification_ms, "exported_bytes_match": true,
              "progress_event_caveat": "event proxy only; initial event may be zero, neither is first wire payload"}))
}

async fn run() -> Result<()> {
    let root = tempfile::Builder::new().prefix("dashbeam-share-probe-").tempdir()?;
    engine::storage::TEMP_DIR.set(PathBuf::from(root.path()))
        .map_err(|_| anyhow!("engine storage root was already initialized"))?;
    for (name, bytes, small) in [("1MiB", MIB, false), ("64MiB", 64 * MIB, false),
                                  ("256MiB", 256 * MIB, false), ("256x4KiB", MIB, true)] {
        let mut samples = Vec::new();
        for _ in 0..3 { samples.push(share_sample(root.path(), bytes, small).await?); }
        println!("{}", json!({"case": name, "samples": samples, "relay": "disabled"}));
    }
    println!("{}", json!({"case": "e2e-direct-64MiB", "samples": [e2e(root.path()).await?],
                          "relay": "disabled", "scope": "engine only; no UI, SAF, or relay"}));
    Ok(())
}

fn main() -> Result<()> {
    // Do this before creating any runtime threads: each native get_or_create_secret
    // call then generates a fresh identity, including concurrent send/receive.
    std::env::remove_var("IROH_SECRET");
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}
