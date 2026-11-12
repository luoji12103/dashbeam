//! Explicit experimental single-file streaming tool. Never used by the app's default flow.
//! Ticket files are bearer secrets; send them only to an intended recipient.
use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use engine::{DiscoveryModeOption, ReceiveOptions, RelayModeOption, SendOptions};
use native::stream::{StreamLimits, StreamReceiveOptions, StreamReceiver, StreamShare, StreamTicket};
use std::{fs::{File, OpenOptions}, io::{Read, Write}, path::PathBuf};

#[derive(Parser)]
#[command(about = "EXPERIMENTAL: one regular file, no pre-hash/store, no resume; upgraded peer required")]
struct Args {
    /// Disable relays; both endpoints must be directly reachable.
    #[arg(long, conflicts_with = "relay_url")]
    direct: bool,
    /// Explicit self-hosted relay(s). Token comes only from DASHBEAM_RELAY_TOKEN.
    #[arg(long)]
    relay_url: Vec<String>,
    /// Custom pkarr address lookup; recommended with self-hosted infrastructure.
    #[arg(long)]
    pkarr: Option<String>,
    /// Explicit file-size budget; this is not a reservation of available disk space.
    #[arg(long, default_value_t = 8 * 1024 * 1024 * 1024u64)]
    max_bytes: u64,
    /// QUIC flow-control ceiling, not a full-file cache. Use 1 for a low-memory profile.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..=16))]
    transport_window_mib: u32,
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Serve until Ctrl-C. Does not silently upgrade/fallback from legacy clients.
    Send { source: PathBuf, #[arg(long)] ticket_file: PathBuf },
    /// Existing, private, non-symlink output directory required. No automatic retry.
    Receive { #[arg(long)] ticket_file: PathBuf, #[arg(long)] output: PathBuf },
}
fn network(a: &Args) -> Result<(RelayModeOption, DiscoveryModeOption)> {
    ensure!(a.direct || !a.relay_url.is_empty(), "choose explicit --direct or --relay-url with --pkarr; no implicit public infrastructure");
    ensure!(!a.direct || a.pkarr.is_none(), "--direct does not use public/custom discovery; omit --pkarr");
    let relay = if a.direct { RelayModeOption::Disabled }
        else { engine::build_relay_mode(Some(engine::RelayConfigArg {
            mode:"custom".into(), urls:a.relay_url.clone(),
            auth_token:std::env::var("DASHBEAM_RELAY_TOKEN").ok(), fallback:Some("strict".into()),
        })).map_err(|_|anyhow::anyhow!("invalid explicit relay configuration"))? };
    let discovery = match &a.pkarr {
        Some(url) => DiscoveryModeOption::Custom {
            pkarr_relay_url: protocol::discovery::parse_pkarr_relay_url(url)
                .map_err(|_|anyhow::anyhow!("invalid explicit discovery URL"))?, dns_origin: None },
        None => { ensure!(a.relay_url.is_empty(), "self-hosted relay requires explicit --pkarr (no silent public lookup)"); DiscoveryModeOption::Default }
    };
    Ok((relay, discovery))
}
fn save_private(path: &PathBuf, ticket: &StreamTicket) -> Result<()> {
    let mut o=OpenOptions::new();o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut file=o.open(path).context("create exclusive private ticket file")?;
    file.write_all(ticket.to_token()?.as_bytes())?;file.sync_all()?;Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let a=Args::parse();let (relay_mode,discovery_mode)=network(&a)?;
    ensure!(a.max_bytes>0,"--max-bytes must be positive");
    let limits=StreamLimits{max_bytes:a.max_bytes,transport_window_bytes:a.transport_window_mib * 1024 * 1024,..Default::default()};
    match a.action {
        Action::Send {source,ticket_file} => {
            let share=StreamShare::start_with_limits(source,SendOptions{relay_mode,discovery_mode,..Default::default()},limits).await?;
            if let Err(error)=save_private(&ticket_file,&share.ticket()) {share.shutdown().await;return Err(error);}
            println!("Experimental share ready. Ticket is a bearer secret in the specified file; Ctrl-C revokes it.");
            let interrupted=tokio::signal::ctrl_c().await;
            share.shutdown().await;
            // Preserve the secret file for explicit user cleanup; the endpoint/capability is now revoked.
            interrupted?;
        }
        Action::Receive {ticket_file,output} => {
            let mut bytes=Vec::new();
            File::open(&ticket_file).context("open private ticket file")?.take(8193).read_to_end(&mut bytes)?;
            ensure!(bytes.len()<=8192,"ticket exceeds size limit");
            let ticket=StreamTicket::from_token(std::str::from_utf8(&bytes)?)?;
            ensure!(!std::fs::symlink_metadata(&output)?.file_type().is_symlink(),"output itself must not be a symlink");
            // Resolve the explicitly chosen local directory once (e.g. macOS /var alias).
            // The engine still checks the resolved directory and refuses shared-write access.
            let output=output.canonicalize().context("resolve selected output directory")?;
            let mut receiver=StreamReceiver::new(StreamReceiveOptions{network:ReceiveOptions{relay_mode,discovery_mode,..Default::default()},limits}).await?;
            let (tx,rx)=tokio::sync::oneshot::channel();
            let interrupt=tokio::spawn(async move {if tokio::signal::ctrl_c().await.is_ok(){let _=tx.send(());}});
            let result=receiver.receive(&ticket,output,rx).await;
            interrupt.abort();receiver.shutdown().await;
            let result=result?;
            println!("Verified {} bytes; saved to {}. Completion receipt transport-delivered: {}",result.bytes,result.path.display(),result.completion_ack_sent);
            if !result.completion_ack_sent {println!("The file is already committed. Do not automatically retry.");}
        }
    }
    Ok(())
}
