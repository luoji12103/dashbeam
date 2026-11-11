//! Isolated DEV probe for iroh 1.0.3. Copy to engine/native/examples/.
//! Pass exactly one JSON object on stdin: {"urls":["https://...","https://..."],"auth_token":"..."}.
//! No production endpoints, tokens, IDs, or tickets are printed. This is not a
//! DashBeam/iroh-blobs benchmark and must not be used as steady-state throughput.
//! By default the URLs are tested independently, in input order. Set
//! "cross_only":true to test URL 0 as sender home and URL 1 as receiver home.

use std::{cell::Cell, io::Read, time::{Duration, Instant}};
use iroh::{endpoint::{presets, Connection, RelayMode}, tls::CaTlsConfig, Endpoint, RelayMap, RelayUrl, SecretKey, TransportAddr, Watcher};
use n0_future::StreamExt;
use serde_json::{json, Value};
use tokio::time::timeout;

const ALPN: &[u8] = b"dashbeam-dev-relay-probe/1";
const SIZE: usize = 16 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;
const PINGS: usize = 8;

fn ms(t: Duration) -> f64 { t.as_secs_f64() * 1000.0 }

fn relay_map(url: RelayUrl, token: &str) -> RelayMode {
    let map = RelayMap::from_iter([url]).with_auth_token(token.to_owned());
    RelayMode::Custom(map)
}

async fn bind(url: RelayUrl, token: &str) -> Result<Endpoint, &'static str> {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .alpns(vec![ALPN.to_vec()])
        .ca_tls_config(CaTlsConfig::system())
        .relay_mode(relay_map(url, token))
        .clear_ip_transports()
        .bind().await.map_err(|_| "endpoint_bind_failed")
}

async fn online(ep: &Endpoint, expected: &RelayUrl) -> Result<f64, &'static str> {
    let start = Instant::now();
    ep.online().await;
    let statuses = ep.home_relay_status().get();
    if !statuses.iter().any(|s| s.is_connected() && s.url() == expected) {
        return Err("home_relay_not_connected");
    }
    Ok(ms(start.elapsed()))
}

async fn selected_relay(conn: &Connection, expected: &RelayUrl) -> Result<(), &'static str> {
    let mut stream = conn.paths_stream();
    while let Some(paths) = stream.next().await {
        if paths.iter().any(|p| p.is_selected() && matches!(p.remote_addr(), TransportAddr::Relay(url) if url == expected)) {
            return Ok(());
        }
        if paths.iter().any(|p| p.is_selected()) {
            return Err("selected_path_not_expected_relay");
        }
    }
    Err("path_stream_closed")
}

// A missing/ambiguous connected home is represented by None, not guessed.
fn home_snapshot(ep: &Endpoint, token: &str) -> (Value, Option<RelayUrl>) {
    let statuses = ep.home_relay_status().get();
    let mut connected: Vec<RelayUrl> = Vec::new();
    let values: Vec<Value> = statuses.iter().map(|status| {
        if status.is_connected() { connected.push(status.url().clone()); }
        json!({
            "domain": status.url().host_str().unwrap_or("").replace(token, "[redacted]"),
            "connected": status.is_connected()
        })
    }).collect();
    let sole = if connected.len() == 1 { connected.pop() } else { None };
    (json!(values), sole)
}

fn different_homes(a: &Option<RelayUrl>, b: &Option<RelayUrl>) -> Option<bool> {
    match (a, b) { (Some(a), Some(b)) => Some(a != b), _ => None }
}

// The sender writes two independent streams and waits for a receiver ACK after
// each. The ACK makes each duration include receipt of the whole payload.
async fn send_payloads(conn: &Connection) -> Result<(Vec<f64>, Vec<f64>), &'static str> {
    let block = vec![0x5au8; CHUNK];
    let mut durations = Vec::new();
    for _ in 0..2 {
        let started = Instant::now();
        let (mut tx, mut rx) = conn.open_bi().await.map_err(|_| "payload_open_failed")?;
        for _ in 0..SIZE / CHUNK {
            tx.write_all(&block).await.map_err(|_| "payload_send_failed")?;
        }
        tx.finish().map_err(|_| "payload_finish_failed")?;
        let mut ack = [0u8; 1];
        rx.read_exact(&mut ack).await.map_err(|_| "payload_ack_failed")?;
        if ack[0] != 1 { return Err("payload_ack_invalid"); }
        durations.push(ms(started.elapsed()));
    }
    let (mut tx, mut rx) = conn.open_bi().await.map_err(|_| "ping_open_failed")?;
    let mut rtts = Vec::new();
    for n in 0..PINGS {
        let started = Instant::now();
        tx.write_all(&[n as u8]).await.map_err(|_| "ping_send_failed")?;
        let mut pong = [0u8; 1];
        rx.read_exact(&mut pong).await.map_err(|_| "pong_receive_failed")?;
        if pong[0] != n as u8 { return Err("pong_invalid"); }
        rtts.push(ms(started.elapsed()));
    }
    tx.finish().map_err(|_| "ping_finish_failed")?;
    Ok((durations, rtts))
}

async fn receive_payloads(conn: &Connection, connect_start: Instant) -> Result<Vec<f64>, &'static str> {
    let mut first_payload = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    for _ in 0..2 {
        let (mut tx, mut rx) = conn.accept_bi().await.map_err(|_| "payload_accept_failed")?;
        rx.read_exact(&mut buf[..1]).await.map_err(|_| "payload_receive_failed")?;
        first_payload.push(ms(connect_start.elapsed()));
        if buf[0] != 0x5a { return Err("payload_invalid"); }
        rx.read_exact(&mut buf[..CHUNK - 1]).await.map_err(|_| "payload_receive_failed")?;
        if buf[..CHUNK - 1].iter().any(|b| *b != 0x5a) { return Err("payload_invalid"); }
        for _ in 1..SIZE / CHUNK {
            rx.read_exact(&mut buf).await.map_err(|_| "payload_receive_failed")?;
            if buf.iter().any(|b| *b != 0x5a) { return Err("payload_invalid"); }
        }
        tx.write_all(&[1]).await.map_err(|_| "payload_ack_send_failed")?;
        tx.finish().map_err(|_| "payload_ack_finish_failed")?;
    }
    let (mut tx, mut rx) = conn.accept_bi().await.map_err(|_| "ping_accept_failed")?;
    for _ in 0..PINGS {
        let mut ping = [0u8; 1];
        rx.read_exact(&mut ping).await.map_err(|_| "ping_receive_failed")?;
        tx.write_all(&ping).await.map_err(|_| "pong_send_failed")?;
    }
    tx.finish().map_err(|_| "pong_finish_failed")?;
    Ok(first_payload)
}

async fn run_case(label: &str, url: RelayUrl, token: &str) -> Value {
    let total = Instant::now();
    let stage = Cell::new("bind_sender");
    let mut sender: Option<Endpoint> = None;
    let mut receiver: Option<Endpoint> = None;
    // The work deadline plus bounded cleanup keeps each case below 90 seconds.
    let result = timeout(Duration::from_secs(85), async {
        let started = Instant::now();
        sender = Some(bind(url.clone(), token).await?);
        let sender_bind_ms = ms(started.elapsed());
        stage.set("bind_receiver");
        let started = Instant::now();
        receiver = Some(bind(url.clone(), token).await?);
        let receiver_bind_ms = ms(started.elapsed());
        let a = sender.as_ref().ok_or("endpoint_missing")?;
        let b = receiver.as_ref().ok_or("endpoint_missing")?;
        stage.set("wait_home_relay");
        let (sender_online_ms, receiver_online_ms) = tokio::try_join!(online(a, &url), online(b, &url))?;
        stage.set("connect");
        let connect_start = Instant::now();
        let (client, server) = tokio::try_join!(
            async { a.connect(b.addr(), ALPN).await.map_err(|_| "connect_failed") },
            async { b.accept().await.ok_or("accept_closed")?.await.map_err(|_| "accept_failed") }
        )?;
        let connect_ms = ms(connect_start.elapsed());
        stage.set("verify_selected_relay_before_payload");
        tokio::try_join!(selected_relay(&client, &url), selected_relay(&server, &url))?;
        stage.set("payload_and_ping");
        let ((durations, rtts), first_payload) = tokio::try_join!(
            send_payloads(&client), receive_payloads(&server, connect_start)
        )?;
        stage.set("verify_selected_relay_after_payload");
        tokio::try_join!(selected_relay(&client, &url), selected_relay(&server, &url))?;
        let throughputs: Vec<f64> = durations.iter().map(|d| (SIZE as f64 * 8.0) / (d * 1000.0)).collect();
        Ok::<Value, &'static str>(json!({
            "status":"ok", "sender_bind_ms":sender_bind_ms, "receiver_bind_ms":receiver_bind_ms,
            "sender_online_ms_after_both_bind":sender_online_ms, "receiver_online_ms_after_both_bind":receiver_online_ms,
            "connect_ms":connect_ms, "first_payload_ms_from_connect_start":first_payload,
            "payload_bytes_per_stream":SIZE, "stream_total_ms_including_ack":durations,
            "stream_mbps_including_ack":throughputs, "ping_pong_rtt_ms":rtts,
            "selected_relay_verified_both_ends_before_and_after":true,
            "relay_domain":url.host_str().unwrap_or("").replace(token, "[redacted]")
        }))
    }).await;
    // Never format any underlying error or relay configuration.
    let mut output = match result {
        Ok(Ok(value)) => value,
        Ok(Err(category)) => json!({"status":"error", "stage":stage.get(), "category":category}),
        Err(_) => json!({"status":"error", "stage":stage.get(), "category":"case_timeout"}),
    };
    let mut cleanup_ok = true;
    if let Some(ep) = sender { cleanup_ok &= timeout(Duration::from_secs(2), ep.close()).await.is_ok(); }
    if let Some(ep) = receiver { cleanup_ok &= timeout(Duration::from_secs(2), ep.close()).await.is_ok(); }
    output["case"] = json!(label);
    output["cleanup_completed"] = json!(cleanup_ok);
    output["case_total_ms_including_cleanup"] = json!(ms(total.elapsed()));
    output
}

// URL 0 is the sender's initial home, URL 1 the receiver's initial home.
// Only the receiver's relay map is extended; it dials the sender. The sender
// then sends application payload back over that connection.
async fn run_cross(sender_url: RelayUrl, receiver_url: RelayUrl, token: &str) -> Value {
    let total = Instant::now();
    let stage = Cell::new("bind_sender");
    let mut sender: Option<Endpoint> = None;
    let mut receiver: Option<Endpoint> = None;
    let mut initial_homes: Option<Value> = None;
    let mut after_insert_homes: Option<Value> = None;
    let mut final_homes: Option<Value> = None;
    let result = timeout(Duration::from_secs(85), async {
        let started = Instant::now();
        sender = Some(bind(sender_url.clone(), token).await?);
        let sender_bind_ms = ms(started.elapsed());
        stage.set("bind_receiver");
        let started = Instant::now();
        receiver = Some(bind(receiver_url.clone(), token).await?);
        let receiver_bind_ms = ms(started.elapsed());
        let a = sender.as_ref().ok_or("endpoint_missing")?;
        let b = receiver.as_ref().ok_or("endpoint_missing")?;
        stage.set("wait_initial_homes");
        let (sender_online_ms, receiver_online_ms) = tokio::try_join!(
            online(a, &sender_url), online(b, &receiver_url)
        )?;
        let (a_snapshot, a_home) = home_snapshot(a, token);
        let (b_snapshot, b_home) = home_snapshot(b, token);
        initial_homes = Some(json!({"sender":a_snapshot, "receiver":b_snapshot,
            "different":different_homes(&a_home, &b_home)}));
        if a_home.as_ref() != Some(&sender_url) || b_home.as_ref() != Some(&receiver_url)
            || different_homes(&a_home, &b_home) != Some(true) {
            return Err("initial_homes_not_proven_different");
        }
        stage.set("insert_sender_relay_into_receiver");
        let started = Instant::now();
        let map = RelayMap::from_iter([sender_url.clone()]).with_auth_token(token.to_owned());
        let config = map.get(&sender_url).ok_or("relay_config_missing")?;
        // None means no old entry (also returned if closed); it is not a success
        // flag. The subsequent connection/path checks verify actual operation.
        let _previous = b.insert_relay(sender_url.clone(), config).await;
        let insert_ms = ms(started.elapsed());
        let (a_snapshot, a_home) = home_snapshot(a, token);
        let (b_snapshot, b_home) = home_snapshot(b, token);
        after_insert_homes = Some(json!({"sender":a_snapshot, "receiver":b_snapshot,
            "different":different_homes(&a_home, &b_home)}));
        stage.set("connect_receiver_to_sender");
        let connect_start = Instant::now();
        let (client, server) = tokio::try_join!(
            async { b.connect(a.addr(), ALPN).await.map_err(|_| "connect_failed") },
            async { a.accept().await.ok_or("accept_closed")?.await.map_err(|_| "accept_failed") }
        )?;
        let connect_ms = ms(connect_start.elapsed());
        stage.set("verify_selected_relay_before_payload");
        tokio::try_join!(selected_relay(&client, &sender_url), selected_relay(&server, &sender_url))?;
        stage.set("payload_and_ping");
        let ((durations, rtts), first_payload) = tokio::try_join!(
            send_payloads(&server), receive_payloads(&client, connect_start)
        )?;
        stage.set("verify_selected_relay_after_payload");
        tokio::try_join!(selected_relay(&client, &sender_url), selected_relay(&server, &sender_url))?;
        let (a_snapshot, a_home) = home_snapshot(a, token);
        let (b_snapshot, b_home) = home_snapshot(b, token);
        final_homes = Some(json!({"sender":a_snapshot, "receiver":b_snapshot,
            "different":different_homes(&a_home, &b_home),
            "both_match_initial": match (&a_home, &b_home) {
                (Some(a), Some(b)) => Some(a == &sender_url && b == &receiver_url),
                _ => None,
            }}));
        let throughputs: Vec<f64> = durations.iter().map(|d| (SIZE as f64 * 8.0) / (d * 1000.0)).collect();
        Ok::<Value, &'static str>(json!({
            "status":"ok", "sender_bind_ms":sender_bind_ms, "receiver_bind_ms":receiver_bind_ms,
            "sender_online_ms_after_both_bind":sender_online_ms, "receiver_online_ms_after_both_bind":receiver_online_ms,
            "insert_ms":insert_ms, "connect_ms":connect_ms,
            "first_payload_ms_from_connect_start":first_payload, "payload_bytes_per_stream":SIZE,
            "stream_total_ms_including_ack":durations, "stream_mbps_including_ack":throughputs,
            "ping_pong_rtt_ms":rtts, "selected_sender_relay_verified_both_ends_before_and_after":true,
            "relay_domain":sender_url.host_str().unwrap_or("").replace(token, "[redacted]")
        }))
    }).await;
    let mut output = match result {
        Ok(Ok(value)) => value,
        Ok(Err(category)) => json!({"status":"error", "stage":stage.get(), "category":category}),
        Err(_) => json!({"status":"error", "stage":stage.get(), "category":"case_timeout"}),
    };
    // Preserve end-state evidence even when connect, transfer, or the deadline fails.
    if final_homes.is_none() {
        if let (Some(a), Some(b)) = (sender.as_ref(), receiver.as_ref()) {
            let (a_snapshot, a_home) = home_snapshot(a, token);
            let (b_snapshot, b_home) = home_snapshot(b, token);
            final_homes = Some(json!({"sender":a_snapshot, "receiver":b_snapshot,
                "different":different_homes(&a_home, &b_home),
                "both_match_initial": match (&a_home, &b_home) {
                    (Some(a), Some(b)) => Some(a == &sender_url && b == &receiver_url),
                    _ => None,
                }}));
        }
    }
    output["initial_homes"] = json!(initial_homes);
    output["after_insert_homes"] = json!(after_insert_homes);
    output["final_homes"] = json!(final_homes);
    let mut cleanup_ok = true;
    if let Some(ep) = sender { cleanup_ok &= timeout(Duration::from_secs(2), ep.close()).await.is_ok(); }
    if let Some(ep) = receiver { cleanup_ok &= timeout(Duration::from_secs(2), ep.close()).await.is_ok(); }
    output["case"] = json!("cross_home_url_0_sender_url_1_receiver");
    output["cleanup_completed"] = json!(cleanup_ok);
    output["case_total_ms_including_cleanup"] = json!(ms(total.elapsed()));
    output
}

#[tokio::main]
async fn main() {
    let mut input = String::new();
    let parsed = std::io::stdin().read_to_string(&mut input).ok()
        .and_then(|_| serde_json::from_str::<Value>(&input).ok());
    let config = parsed.as_ref().and_then(|v| {
        let urls = v.get("urls")?.as_array()?;
        if urls.len() != 2 { return None; }
        let first: RelayUrl = urls[0].as_str()?.parse().ok()?;
        let second: RelayUrl = urls[1].as_str()?.parse().ok()?;
        let token = v.get("auth_token")?.as_str()?;
        if token.is_empty() || first == second || first.scheme() != "https" || second.scheme() != "https"
            || !first.username().is_empty() || !second.username().is_empty()
            || first.password().is_some() || second.password().is_some() { return None; }
        let cross_only = match v.get("cross_only") { None => false, Some(value) => value.as_bool()? };
        Some((first, second, token, cross_only))
    });
    let output = if let Some((first, second, token, cross_only)) = config {
        let cases = if cross_only {
            vec![run_cross(first, second, token).await]
        } else {
            vec![run_case("url_0", first, token).await, run_case("url_1", second, token).await]
        };
        json!({"kind":"DEV_raw_iroh_quic_relay_probe", "scope":"no_blobs_no_ui_short_sample_not_steady_state", "cross_only":cross_only, "cases":cases})
    } else {
        json!({"status":"error", "stage":"input", "category":"invalid_input"})
    };
    println!("{}", output);
}
