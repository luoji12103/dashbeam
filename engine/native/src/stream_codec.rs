//! Bounded wire format for the opt-in source stream protocol.

use std::time::Duration;

use anyhow::{ensure, Context};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

// This experimental v1 has not been released. max_bytes is required, and both
// peers reject unknown fields: pre-budget/Accept prototype peers fail closed.
pub(super) const VERSION: u8 = 1;
pub(super) const MAX_CONTROL_BYTES: usize = 4096;
pub(super) const MAX_NAME_BYTES: usize = 240;
pub const STREAM_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub version: u8,
    pub capability: [u8; 32],
    /// Sending this request explicitly authorizes receiving up to this size.
    /// Required: older pre-release requests fail closed instead of obtaining
    /// an implicit unlimited budget.
    pub max_bytes: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Offer {
    pub version: u8,
    pub name: String,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub bytes: u64,
    pub blake3: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Complete {
    pub version: u8,
    pub bytes: u64,
    pub blake3: [u8; 32],
}

pub(super) async fn timed<T>(
    duration: Duration,
    operation: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    timeout(duration, operation)
        .await
        .map_err(|_| anyhow::anyhow!("source stream idle timeout"))?
}

pub(super) async fn read_exact<R: AsyncRead + Unpin>(
    rx: &mut R,
    buf: &mut [u8],
    idle: Duration,
) -> anyhow::Result<()> {
    timed(idle, async {
        rx.read_exact(buf).await.context("source stream truncated")?;
        Ok(())
    })
    .await
}

pub(super) async fn write_all<W: AsyncWrite + Unpin>(
    tx: &mut W,
    buf: &[u8],
    idle: Duration,
) -> anyhow::Result<()> {
    timed(idle, async {
        tx.write_all(buf).await.context("source stream write")?;
        Ok(())
    })
    .await
}

pub(super) async fn read_control<R: AsyncRead + Unpin, T: DeserializeOwned>(
    rx: &mut R,
    idle: Duration,
) -> anyhow::Result<T> {
    let mut len = [0u8; 4];
    read_exact(rx, &mut len, idle).await?;
    let len = u32::from_be_bytes(len) as usize;
    ensure!(
        len > 0 && len <= MAX_CONTROL_BYTES,
        "invalid source stream control length"
    );
    // Never allocate based on an unauthenticated peer's claimed length.
    let mut bytes = [0u8; MAX_CONTROL_BYTES];
    read_exact(rx, &mut bytes[..len], idle).await?;
    serde_json::from_slice(&bytes[..len])
        .map_err(|_| anyhow::anyhow!("invalid source stream control message"))
}

pub(super) async fn write_control<W: AsyncWrite + Unpin, T: Serialize>(
    tx: &mut W,
    value: &T,
    idle: Duration,
) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_CONTROL_BYTES,
        "source stream control message too large"
    );
    write_all(tx, &(bytes.len() as u32).to_be_bytes(), idle).await?;
    write_all(tx, &bytes, idle).await
}

pub(super) async fn read_frame_length<R: AsyncRead + Unpin>(
    rx: &mut R,
    remaining: u64,
    idle: Duration,
) -> anyhow::Result<usize> {
    let mut len = [0u8; 4];
    read_exact(rx, &mut len, idle).await?;
    let len = u32::from_be_bytes(len) as usize;
    ensure!(
        len <= STREAM_BUFFER_BYTES && len as u64 <= remaining,
        "invalid source stream frame length"
    );
    Ok(len)
}

pub(super) async fn require_eof<R: AsyncRead + Unpin>(
    rx: &mut R,
    idle: Duration,
) -> anyhow::Result<()> {
    timed(idle, async {
        let mut trailing = [0u8; 1];
        ensure!(
            rx.read(&mut trailing).await? == 0,
            "trailing source stream data"
        );
        Ok(())
    })
    .await
}

pub(super) fn validate_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= MAX_NAME_BYTES
            && name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name.chars().any(|c| {
                c.is_control()
                    || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
            }),
        "source stream name must be a safe single filename"
    );
    let base = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'));
    ensure!(!reserved, "reserved source stream filename");
    Ok(())
}
