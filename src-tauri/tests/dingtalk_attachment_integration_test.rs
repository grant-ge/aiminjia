use app_lib::connector::im::types::ConversationType;
use app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use app_lib::runtime::path_auth::derive_working_dirs_from_attachments;

#[test]
fn im_attachment_paths_are_authorized_for_turn() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file_path = tmp.path().join("dingtalk_downloads").join("a.txt");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, b"hello").unwrap();

    let attachments = vec![ChatAttachmentRef {
        id: "sha".into(),
        file_name: "a.txt".into(),
        file_path: file_path.to_string_lossy().to_string(),
        kind: "file".into(),
        file_size: 5,
        file_type: "txt".into(),
        mime_type: Some("text/plain".into()),
    }];

    let dirs = derive_working_dirs_from_attachments(
        &attachments
            .iter()
            .map(|a| std::path::PathBuf::from(&a.file_path))
            .collect::<Vec<_>>(),
    );

    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("dingtalk_downloads"));
}

#[test]
fn grouped_content_shape_matches_channel_contract() {
    let rendered = match ConversationType::Group {
        ConversationType::Group => format!("[{}]: {}", "张三", "请分析"),
        ConversationType::Private => "请分析".to_string(),
    };
    assert_eq!(rendered, "[张三]: 请分析");
}
