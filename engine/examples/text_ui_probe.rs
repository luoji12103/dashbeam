use engine::{
    download, start_share, AddrInfoOptions, FileMetadata, ReceiveOptions, RelayModeOption,
    SendOptions, TEXT_CONTENT_KIND,
};
use std::io::Write as _;
use std::time::Duration;

const FILE_NAME: &str = "DashBeam Text.txt";
const FIXTURE: &str = "# DashBeam 文字测试\n\n**Markdown** 原样保留。\n";

fn usage() -> ! {
    eprintln!("usage: text_ui_probe serve | text_ui_probe receive <ticket>");
    std::process::exit(2);
}

fn send_options() -> SendOptions {
    SendOptions {
        relay_mode: RelayModeOption::Disabled,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        ..Default::default()
    }
}

fn receive_options(output_dir: std::path::PathBuf) -> ReceiveOptions {
    ReceiveOptions {
        output_dir: Some(output_dir),
        relay_mode: RelayModeOption::Disabled,
        ..Default::default()
    }
}

async fn serve() -> anyhow::Result<()> {
    let source_dir = tempfile::tempdir()?;
    let source_path = source_dir.path().join(FILE_NAME);
    std::fs::write(&source_path, FIXTURE.as_bytes())?;

    let metadata = FileMetadata {
        file_name: FILE_NAME.to_string(),
        item_count: 1,
        size: FIXTURE.len() as u64,
        thumbnail: None,
        mime_type: Some("text/plain".to_string()),
        items: None,
        content_kind: Some(TEXT_CONTENT_KIND.to_string()),
    };
    let share = start_share(source_path, send_options(), None, Some(metadata)).await?;

    println!("{}", share.ticket);
    std::io::stdout().flush()?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = tokio::time::sleep(Duration::from_secs(10 * 60)) => {}
    }

    drop(share);
    drop(source_dir);
    Ok(())
}

async fn receive(ticket: String) -> anyhow::Result<()> {
    let output_dir = tempfile::tempdir()?;
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let result = download(
        ticket,
        receive_options(output_dir.path().to_path_buf()),
        None,
        cancel_rx,
    )
    .await?;

    let received = result
        .exported_files
        .iter()
        .find(|file| file.collection_name == FILE_NAME)
        .ok_or_else(|| anyhow::anyhow!("fixture file missing from received manifest"))?;
    let bytes = std::fs::read(&received.path)?;
    println!(
        "bytes={} matches_fixture={}",
        bytes.len(),
        bytes == FIXTURE.as_bytes()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Force this probe to use a fresh process-local endpoint identity instead
    // of consulting any persisted device/node identity.
    std::env::set_var(
        "IROH_SECRET",
        data_encoding::HEXLOWER.encode(&iroh::SecretKey::generate().to_bytes()),
    );

    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("serve"), None, None) => serve().await,
        (Some("receive"), Some(ticket), None) => receive(ticket).await,
        _ => usage(),
    }
}
