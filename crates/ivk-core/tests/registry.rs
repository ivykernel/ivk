//! Integration tests for the SQLite registry (plan v3 Phase B): schema
//! bootstrap, the create/remove journal, changeset records, and the
//! backfill path that keeps v0.0.x directory-layout repos working.

use std::fs;
use std::path::{Path, PathBuf};

use ivk_core::{BeginCreate, ChangesetRecord, Registry, WorkspaceState};

fn temp_root() -> PathBuf {
    let tid = std::thread::current().id();
    let base = std::env::temp_dir().join(format!(
        "ivk-core-reg-{}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        tid,
    ));
    fs::create_dir_all(&base).expect("create temp root");
    base
}

fn changeset(id: &str, ws: &str) -> ChangesetRecord {
    ChangesetRecord {
        id: id.into(),
        workspace_name: ws.into(),
        base_snapshot: "a".repeat(40),
        result_snapshot: "b".repeat(40),
        touched_paths: vec!["src/x.rs".into(), "src/y.rs".into()],
        created_at_unix: 1_000,
        exported_branch: None,
        exported_at_unix: None,
    }
}

#[test]
fn open_is_idempotent_and_gated_on_ivk_dir() {
    let root = temp_root();

    // No .ivk yet: read path must not initialize anything.
    assert!(Registry::open_if_present(&root).unwrap().is_none());
    assert!(!root.join(".ivk").exists());

    // Write path creates .ivk + db; reopening both ways works.
    let _reg = Registry::open_at_root(&root).expect("open");
    assert!(Registry::db_path(&root).is_file());
    drop(_reg);
    let _again = Registry::open_at_root(&root).expect("reopen");
    assert!(Registry::open_if_present(&root).unwrap().is_some());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn create_remove_journal_transitions() {
    let root = temp_root();
    let reg = Registry::open_at_root(&root).unwrap();

    assert_eq!(
        reg.begin_create("ws-a", Some("deadbeef")).unwrap(),
        BeginCreate::Started
    );
    let rec = reg.workspace("ws-a").unwrap().expect("row exists");
    assert_eq!(rec.state, WorkspaceState::Creating);
    assert_eq!(rec.base_snapshot.as_deref(), Some("deadbeef"));

    // A second begin_create must not steal ownership of the row.
    assert_eq!(
        reg.begin_create("ws-a", None).unwrap(),
        BeginCreate::AlreadyTracked
    );
    assert_eq!(
        reg.workspace("ws-a")
            .unwrap()
            .unwrap()
            .base_snapshot
            .as_deref(),
        Some("deadbeef"),
        "conflicting begin_create must not overwrite the existing row"
    );

    reg.mark_ready("ws-a").unwrap();
    assert_eq!(
        reg.workspace("ws-a").unwrap().unwrap().state,
        WorkspaceState::Ready
    );

    reg.begin_remove("ws-a").unwrap();
    assert_eq!(
        reg.workspace("ws-a").unwrap().unwrap().state,
        WorkspaceState::Removing
    );

    reg.finish_remove("ws-a").unwrap();
    assert!(reg.workspace("ws-a").unwrap().is_none());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn changeset_roundtrip_and_export_stamp() {
    let root = temp_root();
    let reg = Registry::open_at_root(&root).unwrap();

    reg.record_changeset(&changeset("ch_one", "ws-a")).unwrap();
    let mut newer = changeset("ch_two", "ws-b");
    newer.created_at_unix = 2_000;
    reg.record_changeset(&newer).unwrap();

    let all = reg.changesets().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "ch_two", "newest first");
    assert_eq!(all[1].touched_paths, vec!["src/x.rs", "src/y.rs"]);

    reg.mark_exported("ch_one", "agent/ws-a").unwrap();
    let one = reg.changeset("ch_one").unwrap().unwrap();
    assert_eq!(one.exported_branch.as_deref(), Some("agent/ws-a"));
    assert!(one.exported_at_unix.is_some());
    let two = reg.changeset("ch_two").unwrap().unwrap();
    assert!(two.exported_branch.is_none());

    let _ = fs::remove_dir_all(&root);
}

fn write_json_changeset(root: &Path, id: &str, ws: &str) {
    let dir = root.join(".ivk").join("changesets");
    fs::create_dir_all(&dir).unwrap();
    let body = format!(
        r#"{{"id":"{id}","workspace_name":"{ws}","base_snapshot":"{b}","result_snapshot":"{r}","touched_paths":["f.txt"],"created_at_unix":123}}"#,
        b = "c".repeat(40),
        r = "d".repeat(40),
    );
    fs::write(dir.join(format!("{id}.json")), body).unwrap();
}

#[test]
fn sync_backfills_v00x_layout_without_clobbering_journal() {
    let root = temp_root();
    // Pre-DB layout: two workspace dirs + one changeset JSON.
    fs::create_dir_all(root.join(".ivk/workspaces/old-a")).unwrap();
    fs::create_dir_all(root.join(".ivk/workspaces/old-b")).unwrap();
    write_json_changeset(&root, "ch_json", "old-a");

    let reg = Registry::open_at_root(&root).unwrap();
    // An in-flight row that sync must not touch.
    reg.begin_create("old-b", Some("feedface")).unwrap();

    let report = reg.sync_from_disk(&root).unwrap();
    assert_eq!(report.imported_workspaces, 1, "only old-a is new");
    assert_eq!(report.imported_changesets, 1);

    let a = reg.workspace("old-a").unwrap().unwrap();
    assert_eq!(a.state, WorkspaceState::Ready);
    assert!(a.base_snapshot.is_none(), "backfill does not invent a base");
    let b = reg.workspace("old-b").unwrap().unwrap();
    assert_eq!(
        b.state,
        WorkspaceState::Creating,
        "sync must not clobber the journal"
    );

    let ch = reg.changeset("ch_json").unwrap().unwrap();
    assert_eq!(ch.workspace_name, "old-a");
    assert_eq!(ch.touched_paths, vec!["f.txt"]);
    assert_eq!(ch.created_at_unix, 123);

    // Idempotent: second sync imports nothing.
    let report = reg.sync_from_disk(&root).unwrap();
    assert_eq!(report.imported_workspaces, 0);
    assert_eq!(report.imported_changesets, 0);

    let _ = fs::remove_dir_all(&root);
}
