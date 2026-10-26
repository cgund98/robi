use std::sync::Arc;

use robi_core::tool::{ApprovalDecision, Tool};
use serde_json::json;

use super::test_harness::{harness, run};
use super::{delete_file::DeleteFile, edit_file::EditFile, write_file::WriteFile};
use crate::agent::review::{hunks_for_session, reject, FileStatus};

#[tokio::test]
async fn edit_file_replaces_one_match_and_keeps_the_first_baseline() {
    let harness = harness().await;
    std::fs::write(harness.root.join("a.txt"), "one\ntwo\n").unwrap();
    let edit = EditFile::new(Arc::clone(&harness.ctx));
    let args = json!({"path": "a.txt", "old_string": "two\n", "new_string": "three\n"});
    assert_eq!(
        edit.requires_approval(&args).await,
        ApprovalDecision::AllowImmediately
    );
    let result = edit.execute(args, run()).await.unwrap();
    assert_eq!(result["status"], "modified");
    assert_eq!(
        std::fs::read_to_string(harness.root.join("a.txt")).unwrap(),
        "one\nthree\n"
    );

    edit.execute(
        json!({"path": "a.txt", "old_string": "three\n", "new_string": "four\n"}),
        run(),
    )
    .await
    .unwrap();
    let baseline = harness
        .ctx
        .file_changes
        .get_baseline(harness.session_id, "a.txt")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(baseline.baseline, "one\ntwo\n");
    assert!(!baseline.created);

    let hunks = hunks_for_session(
        harness.ctx.file_changes.as_ref(),
        &harness.ctx.root,
        harness.session_id,
    )
    .await
    .unwrap();
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].status, FileStatus::Modified);
    let restored = reject("one\nfour\n", &hunks[0].hunks[0]).unwrap();
    assert_eq!(restored, "one\ntwo\n");
}

#[tokio::test]
async fn an_ambiguous_edit_does_not_write() {
    let harness = harness().await;
    std::fs::write(harness.root.join("a.txt"), "same\nsame\n").unwrap();
    let edit = EditFile::new(Arc::clone(&harness.ctx));
    let error = edit
        .execute(
            json!({"path": "a.txt", "old_string": "same\n", "new_string": "other\n"}),
            run(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("matched 2 times"), "{error}");
    assert_eq!(
        std::fs::read_to_string(harness.root.join("a.txt")).unwrap(),
        "same\nsame\n"
    );
}

#[tokio::test]
async fn a_denied_path_asks_for_approval_and_then_writes() {
    let harness = harness().await;
    std::fs::write(harness.root.join(".env"), "TOKEN=1\n").unwrap();
    let edit = EditFile::new(Arc::clone(&harness.ctx));
    let args = json!({"path": ".env", "old_string": "TOKEN=1\n", "new_string": "TOKEN=2\n"});
    assert_eq!(
        edit.requires_approval(&args).await,
        ApprovalDecision::NeedsApproval
    );
    edit.execute(args, run()).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(harness.root.join(".env")).unwrap(),
        "TOKEN=2\n"
    );
    let rules = harness
        .ctx
        .sessions
        .get_chat_session(harness.session_id)
        .await
        .unwrap()
        .path_rules;
    assert!(rules.allow_write.is_empty());
}

#[tokio::test]
async fn write_file_creates_a_file_and_delete_file_drops_that_baseline() {
    let harness = harness().await;
    let write = WriteFile::new(Arc::clone(&harness.ctx));
    let created = write
        .execute(
            json!({"path": "nested/new.txt", "content": "hello\n"}),
            run(),
        )
        .await
        .unwrap();
    assert_eq!(created["status"], "added");
    assert_eq!(
        std::fs::read_to_string(harness.root.join("nested/new.txt")).unwrap(),
        "hello\n"
    );

    let delete = DeleteFile::new(Arc::clone(&harness.ctx));
    assert_eq!(
        delete
            .requires_approval(&json!({"path": "nested/new.txt"}))
            .await,
        ApprovalDecision::AllowImmediately
    );
    let removed = delete
        .execute(json!({"path": "nested/new.txt"}), run())
        .await
        .unwrap();
    assert_eq!(removed["status"], "deleted");
    assert!(!harness.root.join("nested/new.txt").exists());
    assert!(harness
        .ctx
        .file_changes
        .get_baseline(harness.session_id, "nested/new.txt")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn delete_file_of_an_existing_file_keeps_the_baseline() {
    let harness = harness().await;
    std::fs::write(harness.root.join("gone.txt"), "keep\n").unwrap();
    let delete = DeleteFile::new(Arc::clone(&harness.ctx));
    delete
        .execute(json!({"path": "gone.txt"}), run())
        .await
        .unwrap();
    let baseline = harness
        .ctx
        .file_changes
        .get_baseline(harness.session_id, "gone.txt")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(baseline.baseline, "keep\n");
    let hunks = hunks_for_session(
        harness.ctx.file_changes.as_ref(),
        &harness.ctx.root,
        harness.session_id,
    )
    .await
    .unwrap();
    assert_eq!(hunks[0].status, FileStatus::Deleted);
    assert_eq!(reject("", &hunks[0].hunks[0]).unwrap(), "keep\n");
}

#[tokio::test]
async fn delete_file_refuses_a_directory() {
    let harness = harness().await;
    std::fs::create_dir(harness.root.join("dir")).unwrap();
    let delete = DeleteFile::new(Arc::clone(&harness.ctx));
    let error = delete
        .execute(json!({"path": "dir"}), run())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("directory"), "{error}");
}

#[tokio::test]
async fn edit_and_write_refuse_a_compressed_shell_marker() {
    let harness = harness().await;
    std::fs::write(harness.root.join("pay.rs"), "fn keep() {}\n").unwrap();
    let marker = "<<<ROBI_LOG id=\"abc\" sha256=\"abcdabcdabcdabcd\" exit=0>>>";
    let edit = EditFile::new(Arc::clone(&harness.ctx));
    let error = edit
        .execute(
            json!({"path": "pay.rs", "old_string": marker, "new_string": "fn keep() { let x = 1; }\n"}),
            run(),
        )
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("compressed shell marker"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(harness.root.join("pay.rs")).unwrap(),
        "fn keep() {}\n"
    );
    let write = WriteFile::new(Arc::clone(&harness.ctx));
    let error = write
        .execute(json!({"path": "pay.rs", "content": marker}), run())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("compressed shell marker"),
        "{error}"
    );
}
