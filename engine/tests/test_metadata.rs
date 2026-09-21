mod common;

use common::TestFixture;
use engine::{
    download, fetch_metadata, start_share, start_share_items, FileMetadata, ReceiveOptions,
    SendOptions, TEXT_CONTENT_KIND,
};

#[tokio::test]
async fn e2e_metadata_preview() {
    let fixture = TestFixture::new();
    let content = b"preview content here";
    let source = fixture.create_file("preview_test.txt", content);

    let metadata = FileMetadata {
        file_name: "preview_test.txt".into(),
        item_count: 1,
        size: content.len() as u64,
        thumbnail: Some("data:image/png;base64,dGVzdA==".into()),
        mime_type: Some("text/plain".into()),
        items: None,
        content_kind: None,
    };

    let share = start_share(source, SendOptions::default(), None, Some(metadata.clone()))
        .await
        .expect("start_share should succeed");

    let fetched = fetch_metadata(share.ticket.clone(), ReceiveOptions::default())
        .await
        .expect("fetch_metadata should succeed");

    assert_eq!(fetched.file_name, "preview_test.txt");
    assert_eq!(fetched.size, content.len() as u64);
    assert_eq!(fetched.mime_type, Some("text/plain".into()));
    assert_eq!(
        fetched.thumbnail,
        Some("data:image/png;base64,dGVzdA==".into())
    );
    assert_eq!(fetched.item_count, 1);

    drop(share);
}

#[tokio::test]
async fn e2e_metadata_multi_item() {
    let fixture = TestFixture::new();
    let content_a = b"aaa";
    let content_b = b"bbb";
    let file_a = fixture.create_file("a.txt", content_a);
    let file_b = fixture.create_file("b.txt", content_b);

    let metadata = FileMetadata {
        file_name: "2 items".into(),
        item_count: 2,
        size: (content_a.len() + content_b.len()) as u64,
        thumbnail: None,
        mime_type: None,
        items: None,
        content_kind: None,
    };

    let share = start_share_items(
        vec![file_a, file_b],
        SendOptions::default(),
        &None,
        Some(metadata),
    )
    .await
    .expect("start_share_items should succeed");

    let fetched = fetch_metadata(share.ticket.clone(), ReceiveOptions::default())
        .await
        .expect("fetch_metadata should succeed");

    assert_eq!(fetched.file_name, "2 items");
    assert_eq!(fetched.item_count, 2);
    assert_eq!(fetched.size, (content_a.len() + content_b.len()) as u64);
    assert_eq!(
        fetched.thumbnail, None,
        "multi-item share should have no thumbnail"
    );
    assert_eq!(
        fetched.mime_type, None,
        "multi-item share should have no mime_type"
    );

    drop(share);
}

#[tokio::test]
async fn e2e_marked_text_roundtrip_preserves_markdown_and_conflict_path() {
    let fixture = TestFixture::new();
    let content = b"# Typed text\n\n**literal markdown** `stays intact`\n";
    let source = fixture.create_file("DashBeam Text.txt", content);
    let recv_dir = fixture.output_dir();
    std::fs::write(recv_dir.join("DashBeam Text.txt"), b"existing user file")
        .expect("conflicting destination");

    let metadata = FileMetadata {
        file_name: "DashBeam Text.txt".into(),
        item_count: 1,
        size: content.len() as u64,
        thumbnail: None,
        mime_type: Some("text/plain".into()),
        items: None,
        content_kind: Some(TEXT_CONTENT_KIND.into()),
    };
    let share = start_share(source, SendOptions::default(), None, Some(metadata.clone()))
        .await
        .expect("marked text share");

    let fetched = fetch_metadata(share.ticket.clone(), ReceiveOptions::default())
        .await
        .expect("marked text metadata");
    assert_eq!(fetched.content_kind.as_deref(), Some(TEXT_CONTENT_KIND));
    assert_eq!(fetched.size, content.len() as u64);

    let (_cancel_tx, cancel_rx) = common::no_cancel();
    let received = download(
        share.ticket.clone(),
        ReceiveOptions {
            output_dir: Some(recv_dir.clone()),
            ..Default::default()
        },
        None,
        cancel_rx,
    )
    .await
    .expect("marked text download");

    assert_eq!(received.exported_files.len(), 1);
    assert_eq!(
        received.exported_files[0].collection_name,
        "DashBeam Text.txt"
    );
    assert_eq!(
        received.exported_files[0].path,
        recv_dir.join("DashBeam Text (1).txt")
    );
    assert_eq!(
        std::fs::read(&received.exported_files[0].path).expect("received bytes"),
        content
    );
    assert_eq!(
        std::fs::read(recv_dir.join("DashBeam Text.txt")).expect("existing bytes"),
        b"existing user file"
    );

    drop(share);
}
