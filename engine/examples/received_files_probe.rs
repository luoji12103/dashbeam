//! Local-network fixture sender for receive-notification acceptance.
//! Uses a random temporary identity and no relay credentials or pairing data.

use engine::{start_share_items, AddrInfoOptions, RelayModeOption, SendOptions};
use std::io::Write as _;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let names = ["DashBeam-copy-中文.md", "DashBeam-copy spaces &.txt"];
    let mut paths = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let path = source.path().join(name);
        std::fs::write(
            &path,
            format!("DashBeam received-file clipboard fixture {}\n", index + 1),
        )?;
        paths.push(path);
    }
    // Ignore an inherited IROH_SECRET without exposing it or reading an app's
    // persistent identity. The temporary sender only serves these two fixtures.
    std::env::set_var(
        "IROH_SECRET",
        data_encoding::HEXLOWER.encode(&iroh::SecretKey::generate().to_bytes()),
    );
    let options = SendOptions {
        relay_mode: RelayModeOption::Disabled,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        ..Default::default()
    };
    let share = start_share_items(paths, options, &None, None).await?;
    println!("{}", share.ticket);
    std::io::stdout().flush()?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = tokio::time::sleep(Duration::from_secs(15 * 60)) => {}
    }
    drop(share);
    Ok(())
}
