//! Isolated, bounded real-Iroh request prototype; NOT product code.
//! Exact dependency iroh=1.3.0. See streaming-request-Cargo.toml and SIMULATION.md.
//! JSON stdin: {"mode":"direct"|"relay", "relay_url":"https://...",
//! "auth_token":"...", "repetitions":3, "large_bytes":67108864,
//! "first_frame_bytes":65536|8192}.
//! Tokens are used only in memory; no raw errors, IDs, tokens or tickets printed.
//! Same-process endpoints share Instant: first payload timing is valid locally.
//! First-frame ACK is the conservative sender-clock bound usable across hosts.

use std::{cell::Cell, io::Read, path::{Path, PathBuf}, time::{Duration, Instant}};
use iroh::{endpoint::{presets, Connection, RelayMode}, tls::CaTlsConfig,
    Endpoint, RelayMap, RelayUrl, SecretKey, TransportAddr, Watcher};
use n0_future::StreamExt;
use serde_json::{Value, json};
use tokio::{fs::File, io::{AsyncReadExt, AsyncWriteExt}, time::timeout};

const ALPN: &[u8] = b"dashbeam-streaming-request-probe/1";
const FRAME: usize = 64 * 1024;
type Res<T> = Result<T, &'static str>;
fn ms(d: Duration) -> f64 { d.as_secs_f64() * 1000.0 }
fn emit(v: Value) { println!("{v}"); }

async fn write_json(tx: &mut iroh::endpoint::SendStream, v: &Value) -> Res<()> {
    let b = serde_json::to_vec(v).map_err(|_| "json_encode")?;
    if b.len() > 2048 { return Err("json_too_large"); }
    tx.write_all(&(b.len() as u32).to_be_bytes()).await.map_err(|_| "json_length_send")?;
    tx.write_all(&b).await.map_err(|_| "json_send")?;
    Ok(())
}
async fn read_json(rx: &mut iroh::endpoint::RecvStream) -> Res<Value> {
    let mut n = [0;4];
    rx.read_exact(&mut n).await.map_err(|_| "json_length_receive")?;
    let n = u32::from_be_bytes(n) as usize;
    if n == 0 || n > 2048 { return Err("json_length_invalid"); }
    let mut b = vec![0;n];
    rx.read_exact(&mut b).await.map_err(|_| "json_receive")?;
    serde_json::from_slice(&b).map_err(|_| "json_invalid")
}

async fn bind(relay: Option<&RelayUrl>, token: &str) -> Res<Endpoint> {
    let builder = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate()).alpns(vec![ALPN.to_vec()]);
    if let Some(url) = relay {
        builder.ca_tls_config(CaTlsConfig::system())
            .relay_mode(RelayMode::Custom(RelayMap::from_iter([url.clone()])
                .with_auth_token(token.to_owned())))
            .clear_ip_transports().bind().await.map_err(|_| "relay_bind")
    } else {
        builder.relay_mode(RelayMode::Disabled).bind().await.map_err(|_| "direct_bind")
    }
}
async fn online(ep: &Endpoint, url: &RelayUrl) -> Res<()> {
    ep.online().await;
    if !ep.home_relay_status().get().iter().any(|s| s.is_connected() && s.url() == url) {
        return Err("home_relay_not_connected");
    }
    Ok(())
}
async fn path(conn: &Connection, relay: Option<&RelayUrl>) -> Res<&'static str> {
    let mut updates = conn.paths_stream();
    while let Some(paths) = updates.next().await {
        if let Some(selected) = paths.iter().find(|p| p.is_selected()) {
            return match (selected.remote_addr(), relay) {
                (TransportAddr::Ip(_), None) => Ok("ip"),
                (TransportAddr::Relay(url), Some(expected)) if url == expected => Ok("relay"),
                _ => Err("selected_path_mismatch"),
            };
        }
    }
    Err("path_stream_closed")
}

async fn send(conn: &Connection, source: &Path, size: u64, transfer: u64,
    intent: Instant, first_frame: usize) -> Res<Value> {
    let open_start = Instant::now();
    let mut f = File::open(source).await.map_err(|_| "source_open")?;
    if f.metadata().await.map_err(|_| "source_stat")?.len() != size {
        return Err("source_size_changed");
    }
    let file_open_stat_ms = ms(open_start.elapsed());
    let stream_start = Instant::now();
    let (mut tx, mut rx) = conn.open_bi().await.map_err(|_| "open_bi")?;
    // First write opens the stream at the remote endpoint, with useful metadata.
    write_json(&mut tx, &json!({"type":"OPEN","version":1,
        "transfer_id":transfer,"source_size":size,"frame_max":FRAME,
        "first_frame_bytes":first_frame})).await?;
    let offer_written_ms = ms(intent.elapsed());
    let accept = read_json(&mut rx).await?;
    if accept["type"] != "ACCEPT" || accept["transfer_id"] != transfer {
        return Err("accept_invalid");
    }
    let accept_received_ms = ms(intent.elapsed());
    // Read ACKs concurrently: sender does not stop after first data frame.
    let ack_task = async {
        let first = read_json(&mut rx).await?;
        if first["type"] != "FIRST_FRAME" || first["transfer_id"] != transfer
            || first["bytes"] != size.min(first_frame as u64) {
            return Err("first_frame_ack_invalid");
        }
        let first_frame_ack_ms = ms(intent.elapsed());
        let final_ack = read_json(&mut rx).await?;
        if final_ack["type"] != "COMPLETE" || final_ack["transfer_id"] != transfer
            || final_ack["bytes"] != size || final_ack["verified"] != true {
            return Err("final_ack_invalid");
        }
        let complete_ack_ms = ms(intent.elapsed());
        let mut trailing = [0u8;1];
        if rx.read(&mut trailing).await.map_err(|_| "ack_eof")?.is_some() {
            return Err("ack_trailing_bytes");
        }
        Ok::<_, &'static str>((first_frame_ack_ms, complete_ack_ms, final_ack))
    };
    let data_task = async {
        let mut b = vec![0;FRAME];
        let mut count = 0u64;
        let mut hash = blake3::Hasher::new();
        let mut first_disk_read_ms = None;
        let mut disk_read_total_ms = 0.0;
        let mut first_data_write_ms = None;
        while count < size {
            let max_frame = if count == 0 { first_frame } else { FRAME };
            let n = ((size - count).min(max_frame as u64)) as usize;
            let read_start = Instant::now();
            f.read_exact(&mut b[..n]).await.map_err(|_| "source_read_or_truncated")?;
            let read_ms = ms(read_start.elapsed());
            disk_read_total_ms += read_ms;
            if first_disk_read_ms.is_none() { first_disk_read_ms = Some(read_ms); }
            hash.update(&b[..n]);
            tx.write_all(&(n as u32).to_be_bytes()).await.map_err(|_| "frame_length_send")?;
            tx.write_all(&b[..n]).await.map_err(|_| "frame_send")?;
            if first_data_write_ms.is_none() { first_data_write_ms = Some(ms(intent.elapsed())); }
            count += n as u64;
        }
        if f.read(&mut b[..1]).await.map_err(|_| "source_eof")? != 0 {
            return Err("source_grew");
        }
        if count != size { return Err("source_truncated"); }
        let digest = hash.finalize().to_hex().to_string();
        tx.write_all(&0u32.to_be_bytes()).await.map_err(|_| "end_marker_send")?;
        write_json(&mut tx, &json!({"type":"MANIFEST","transfer_id":transfer,
            "bytes":count,"blake3":digest})).await?;
        tx.finish().map_err(|_| "send_finish")?;
        Ok::<_, &'static str>((digest, disk_read_total_ms, first_disk_read_ms, first_data_write_ms))
    };
    let ((first_frame_ack_ms, complete_ack_ms, final_ack),
        (digest, read_total_ms, first_read_ms, first_write_ms)) = tokio::try_join!(ack_task, data_task)?;
    if final_ack["blake3"] != digest { return Err("final_ack_digest_mismatch"); }
    Ok(json!({"file_open_stat_ms":file_open_stat_ms,
        "offer_written_from_send_intent_ms":offer_written_ms,
        "accept_received_from_send_intent_ms":accept_received_ms,
        "first_frame_write_returned_from_send_intent_ms":first_write_ms,
        "first_frame_ack_from_send_intent_ms":first_frame_ack_ms,
        "complete_ack_from_send_intent_ms":complete_ack_ms,
        "stream_total_ms_including_final_ack":ms(stream_start.elapsed()),
        "source_first_read_ms":first_read_ms,"source_read_total_ms":read_total_ms,
        "source_blake3":digest,"sender_bytes":size}))
}

async fn receive(conn: &Connection, dest: &Path, size: u64, transfer: u64,
    expected_hash: &str, intent: Instant, first_frame: usize) -> Res<Value> {
    let (mut tx, mut rx) = conn.accept_bi().await.map_err(|_| "accept_bi")?;
    let open = read_json(&mut rx).await?;
    if open["type"] != "OPEN" || open["version"] != 1
        || open["transfer_id"] != transfer || open["source_size"] != size
        || open["frame_max"] != FRAME || open["first_frame_bytes"] != first_frame {
        return Err("offer_invalid");
    }
    let offer_received = Instant::now();
    let mut f = File::create(dest).await.map_err(|_| "destination_create")?;
    write_json(&mut tx, &json!({"type":"ACCEPT","transfer_id":transfer})).await?;
    let mut b = vec![0u8;FRAME];
    let mut count = 0u64;
    let mut hash = blake3::Hasher::new();
    let mut first_payload_ms = None;
    let mut first_payload_from_offer_ms = None;
    let mut first_frame_verified_ms = None;
    loop {
        let mut n = [0;4];
        rx.read_exact(&mut n).await.map_err(|_| "frame_length_receive")?;
        let n = u32::from_be_bytes(n) as usize;
        if n == 0 { break; }
        if n > FRAME || count + n as u64 > size { return Err("frame_size_invalid"); }
        if count == 0 {
            if n as u64 != size.min(first_frame as u64) { return Err("first_frame_size_invalid"); }
            // This byte is actual file content, after OPEN and the length header.
            rx.read_exact(&mut b[..1]).await.map_err(|_| "first_payload_receive")?;
            first_payload_ms = Some(ms(intent.elapsed()));
            first_payload_from_offer_ms = Some(ms(offer_received.elapsed()));
            rx.read_exact(&mut b[1..n]).await.map_err(|_| "first_frame_receive")?;
        } else {
            rx.read_exact(&mut b[..n]).await.map_err(|_| "frame_receive")?;
        }
        hash.update(&b[..n]);
        f.write_all(&b[..n]).await.map_err(|_| "destination_write")?;
        count += n as u64;
        if first_frame_verified_ms.is_none() {
            first_frame_verified_ms = Some(ms(intent.elapsed()));
            write_json(&mut tx, &json!({"type":"FIRST_FRAME","transfer_id":transfer,
                "bytes":count})).await?;
        }
    }
    let manifest = read_json(&mut rx).await?;
    let mut trailing = [0u8;1];
    if rx.read(&mut trailing).await.map_err(|_| "data_eof")?.is_some() {
        return Err("data_trailing_bytes");
    }
    let digest = hash.finalize().to_hex().to_string();
    if count != size || manifest["type"] != "MANIFEST"
        || manifest["transfer_id"] != transfer || manifest["bytes"] != count
        || manifest["blake3"] != digest || digest != expected_hash {
        return Err("manifest_or_hash_mismatch");
    }
    f.flush().await.map_err(|_| "destination_flush")?;
    if f.metadata().await.map_err(|_| "destination_stat")?.len() != size {
        return Err("destination_size_mismatch");
    }
    write_json(&mut tx, &json!({"type":"COMPLETE","transfer_id":transfer,
        "bytes":count,"blake3":digest,"verified":true})).await?;
    tx.finish().map_err(|_| "ack_finish")?;
    Ok(json!({"first_actual_file_byte_from_shared_send_intent_ms":first_payload_ms,
        "first_actual_file_byte_from_receiver_offer_ms":first_payload_from_offer_ms,
        "first_full_frame_hash_updated_and_file_write_returned_ms":first_frame_verified_ms,
        "receiver_bytes":count,"receiver_blake3":digest,"manifest_verified":true,
        "fixture_hash_verified":true,"data_eof_verified":true,"durable_fsync":false}))
}

async fn transfer(client: &Connection, server: &Connection, source: &Path,
    dest: &Path, size: u64, id: u64, expected_hash: &str, intent: Instant,
    first_frame: usize) -> Res<Value> {
    let (sender, receiver) = tokio::try_join!(
        send(client, source, size, id, intent, first_frame),
        receive(server, dest, size, id, expected_hash, intent, first_frame))?;
    // Separate output reread, outside both send-intent completion measurements.
    let reread_start = Instant::now();
    let mut f = File::open(dest).await.map_err(|_| "output_reread_open")?;
    let mut hash = blake3::Hasher::new(); let mut b = vec![0;FRAME]; let mut n_total=0;
    loop { let n=f.read(&mut b).await.map_err(|_| "output_reread")?;
        if n==0 {break} hash.update(&b[..n]); n_total+=n; }
    if n_total as u64 != size || hash.finalize().to_hex().as_str() != expected_hash {
        return Err("output_reread_mismatch");
    }
    Ok(json!({"sender":sender,"receiver":receiver,"output_reread_verified":true,
        "output_reread_ms_excluded_from_completion":ms(reread_start.elapsed())}))
}

async fn pair(label: &str, source: &Path, dest: &Path, size: u64,
    expected_hash: &str, repetition: u64, relay: Option<&RelayUrl>, token: &str,
    first_frame: usize) {
    let intent=Instant::now(); let stage=Cell::new("bind");
    let mut endpoints=None;
    let result=timeout(Duration::from_secs(100), async {
        let (a,b)=tokio::try_join!(bind(relay,token),bind(relay,token))?;
        let bind_ms=ms(intent.elapsed()); endpoints=Some((a,b));
        let (a,b)=endpoints.as_ref().ok_or("missing_endpoints")?;
        stage.set("online");
        let online_start=Instant::now();
        if let Some(url)=relay {tokio::try_join!(online(a,url),online(b,url))?;}
        let online_ms=ms(online_start.elapsed());
        stage.set("connect"); let connect_start=Instant::now();
        let (client,server)=tokio::try_join!(
            async {a.connect(b.addr(),ALPN).await.map_err(|_|"connect")},
            async {b.accept().await.ok_or("accept_closed")?.await.map_err(|_|"accept")})?;
        if client.remote_id()!=b.id() || server.remote_id()!=a.id() {
            return Err("peer_identity_mismatch");
        }
        let connect_ms=ms(connect_start.elapsed()); stage.set("verify_path");
        let (sender_path,receiver_path)=tokio::try_join!(path(&client,relay),path(&server,relay))?;
        stage.set("cold_transfer");
        let cold=transfer(&client,&server,source,dest,size,repetition*2,expected_hash,intent,first_frame).await?;
        emit(json!({"kind":"transfer","mode":label,"condition":"cold_new_endpoint_pair",
            "repetition":repetition,"bytes":size,"first_frame_bytes":first_frame,
            "status":"ok","bind_pair_ms":bind_ms,
            "online_pair_ms":online_ms,"connect_pair_ms":connect_ms,
            "sender_selected_path":sender_path,"receiver_selected_path":receiver_path,
            "peer_identity_verified_both_ends":true,"result":cold}));
        stage.set("warm_transfer"); let warm_intent=Instant::now();
        let warm=transfer(&client,&server,source,dest,size,repetition*2+1,expected_hash,warm_intent,first_frame).await?;
        let (sender_path,receiver_path)=tokio::try_join!(path(&client,relay),path(&server,relay))?;
        emit(json!({"kind":"transfer","mode":label,"condition":"warm_same_connection",
            "repetition":repetition,"bytes":size,"first_frame_bytes":first_frame,"status":"ok",
            "sender_selected_path":sender_path,"receiver_selected_path":receiver_path,
            "peer_identity_verified_both_ends":true,"result":warm}));
        Ok::<_,&'static str>(())
    }).await;
    match result {
        Ok(Ok(()))=>{},
        Ok(Err(category))=>emit(json!({"kind":"pair_failure","mode":label,"repetition":repetition,
            "bytes":size,"status":"error","stage":stage.get(),"category":category})),
        Err(_)=>emit(json!({"kind":"pair_failure","mode":label,"repetition":repetition,
            "bytes":size,"status":"error","stage":stage.get(),"category":"pair_timeout"})),
    }
    let mut cleanup=true;
    if let Some((a,b))=endpoints {
        let (ca,cb)=tokio::join!(timeout(Duration::from_secs(2),a.close()),timeout(Duration::from_secs(2),b.close()));
        cleanup=ca.is_ok() && cb.is_ok();
    }
    let _=tokio::fs::remove_file(dest).await;
    emit(json!({"kind":"pair_cleanup","mode":label,"repetition":repetition,
        "bytes":size,"completed":cleanup,"pair_total_ms":ms(intent.elapsed())}));
}

fn fixture(path:&Path,size:u64)->Res<String> {
    use std::io::Write;
    let mut f=std::fs::File::create(path).map_err(|_|"fixture_create")?;
    let mut hash=blake3::Hasher::new(); let mut state=0x9e3779b97f4a7c15u64;
    let mut remaining=size; let mut b=vec![0u8;FRAME];
    while remaining>0 {
        let n=remaining.min(FRAME as u64) as usize;
        for byte in &mut b[..n] {state^=state<<13;state^=state>>7;state^=state<<17;*byte=state as u8;}
        f.write_all(&b[..n]).map_err(|_|"fixture_write")?;
        hash.update(&b[..n]); remaining-=n as u64;
    }
    f.sync_all().map_err(|_|"fixture_sync")?;
    Ok(hash.finalize().to_hex().to_string())
}

#[tokio::main(flavor="multi_thread",worker_threads=2)]
async fn main() {
    let mut raw=String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {emit(json!({"status":"input_error"}));return;}
    let Ok(input)=serde_json::from_str::<Value>(&raw) else {emit(json!({"status":"input_error"}));return;};
    let mode=input["mode"].as_str().unwrap_or("direct");
    let token=input["auth_token"].as_str().unwrap_or("");
    let relay=if mode=="relay" { match input["relay_url"].as_str().and_then(|s|s.parse::<RelayUrl>().ok()) {
        Some(v)=>Some(v),None=>{emit(json!({"status":"relay_input_error"}));return;}
    }}else{None};
    let repetitions=input["repetitions"].as_u64().unwrap_or(3).clamp(1,3);
    let first_frame=match input["first_frame_bytes"].as_u64().unwrap_or(FRAME as u64) {
        8192=>8192,65536=>65536,_=>{emit(json!({"status":"first_frame_input_error"}));return;}
    };
    let large=input["large_bytes"].as_u64().unwrap_or(64*1024*1024).clamp(1024*1024,64*1024*1024);
    let root=PathBuf::from(format!("fixture-{}",std::process::id()));
    if std::fs::create_dir(&root).is_err() {emit(json!({"status":"fixture_dir_error"}));return;}
    emit(json!({"kind":"run_metadata","iroh":"1.3.0","protocol":"custom_alpn_no_blobs",
        "mode":mode,"profile":"dev_opt0_debug0_incremental_false","worker_threads":2,
        "frame_max_bytes":FRAME,"first_frame_bytes":first_frame,
        "repetitions":repetitions,"automatic_accept":true,
        "same_process_shared_monotonic_clock":true,"application_aead":false,
        "transport_quic_identity_encryption":true,"full_prehash_before_offer":false,
        "fixture_expected_hash_computed_during_generation_outside_send_intent":true,
        "fixture_page_cache":"recently_written_not_cold_disk","planned_transfers":repetitions*4}));
    for (i,size) in [1024*1024,large].into_iter().enumerate() {
        let source=root.join("source.bin"); let dest=root.join("received.bin"); let started=Instant::now();
        let expected=match fixture(&source,size) {Ok(v)=>v,Err(category)=>{
            emit(json!({"kind":"fixture_failure","status":"error","category":category}));continue;}};
        emit(json!({"kind":"fixture","bytes":size,"generation_and_fsync_ms":ms(started.elapsed()),"blake3":expected}));
        for rep in 0..repetitions {pair(mode,&source,&dest,size,&expected,10*i as u64+rep,relay.as_ref(),token,first_frame).await;}
        let _=std::fs::remove_file(source);
    }
    let _=std::fs::remove_dir(root);
}
