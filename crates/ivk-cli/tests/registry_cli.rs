//! End-to-end tests for the Phase B registry: creation journal, crash
//! recovery via `ivk doctor --repair`, gc reconcile, and DB-backed
//! changesets.
//!
//! "Crash" simulation: the tests write journal rows directly through
//! `ivk_core::Registry` — exactly the state a SIGKILL'd `ivk new` / `ivk ws
//! rm` leaves behind — then drive the real binary to detect and repair it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use ivk_core::Registry;

static INIT_LOCK: Mutex<()> = Mutex::new(());

struct CrossProcessInitLock(PathBuf);
impl CrossProcessInitLock {
    fn acquire() -> Self {
        let path = std::env::temp_dir().join("ivk-test-git-init.lock");
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Self(path),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
    }
}
impl Drop for CrossProcessInitLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ivk")
}

fn temp_root() -> PathBuf {
    let tid = std::thread::current().id();
    let base = std::env::temp_dir().join(format!(
        "ivk-cli-reg-{}-{}-{:?}",
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

fn git(cwd: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "user.email=t@test", "-c", "user.name=t"])
        .args(args)
        .status()
        .expect("spawn git");
    assert!(st.success(), "git {:?} failed", args);
}

fn run_ivk(cwd: &Path, args: &[&str]) -> serde_json::Value {
    let out = Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn ivk");
    assert!(
        out.status.success(),
        "ivk {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    serde_json::from_slice(&out.stdout).expect("json output")
}

fn make_src_repo(root: &Path) -> PathBuf {
    let _g = INIT_LOCK.lock().unwrap();
    let _cp = CrossProcessInitLock::acquire();
    let src = root.join("src-repo");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("hello.txt"), "hello\n").unwrap();
    git(&src, &["init", "-q", "-b", "main", "--template="]);
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "initial"]);
    src
}

fn names(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn new_records_ready_state_and_ls_reports_it() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "ws-a", "--json"]);
    assert!(
        Registry::db_path(&src).is_file(),
        "ivk new must create the registry db"
    );

    let v = run_ivk(&src, &["ls", "--json"]);
    assert_eq!(v["count"], 1);
    assert_eq!(v["workspaces"][0]["name"], "ws-a");
    assert_eq!(v["workspaces"][0]["state"], "ready");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn interrupted_create_is_reported_and_repaired() {
    let root = temp_root();
    let src = make_src_repo(&root);

    // Simulate `ivk new ghost` killed between the journal write and
    // materialization: a `creating` row with no directory.
    {
        let reg = Registry::open_at_root(&src).unwrap();
        reg.begin_create("ghost", Some("0000000000000000000000000000000000000000"))
            .unwrap();
    }

    let v = run_ivk(&src, &["doctor", "--json"]);
    let in_flight = v["registry"]["in_flight"]
        .as_array()
        .expect("in_flight array");
    assert_eq!(in_flight.len(), 1);
    assert_eq!(in_flight[0]["name"], "ghost");
    assert_eq!(in_flight[0]["state"], "creating");
    assert_eq!(v["next_command"], "ivk doctor --repair");

    let v = run_ivk(&src, &["doctor", "--repair", "--json"]);
    assert!(
        names(&v["repair"]["rolled_back"]).contains(&"ghost".to_string()),
        "repair must roll back the interrupted create: {}",
        v
    );

    let v = run_ivk(&src, &["doctor", "--json"]);
    assert!(
        v["registry"]["in_flight"].as_array().unwrap().is_empty(),
        "post-repair doctor must be clean: {}",
        v
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn interrupted_remove_is_completed_by_repair() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "doomed", "--json"]);
    let ws = src.join(".ivk/workspaces/doomed");
    assert!(ws.is_dir());

    // Simulate `ivk ws rm doomed` killed after journaling the intent.
    {
        let reg = Registry::open_at_root(&src).unwrap();
        reg.begin_remove("doomed").unwrap();
    }

    let v = run_ivk(&src, &["doctor", "--repair", "--json"]);
    assert!(
        names(&v["repair"]["completed_removals"]).contains(&"doomed".to_string()),
        "repair must complete the interrupted removal: {}",
        v
    );
    assert!(!ws.exists(), "workspace dir must be removed by repair");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gc_drops_stale_rows_for_externally_deleted_workspaces() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "vanished", "--json"]);
    // Someone rm -rf's the workspace behind ivk's back.
    fs::remove_dir_all(src.join(".ivk/workspaces/vanished")).unwrap();

    let v = run_ivk(&src, &["gc", "--json"]);
    assert!(
        names(&v["removed_registry_rows"]).contains(&"vanished".to_string()),
        "gc must drop the stale registry row: {}",
        v
    );

    let v = run_ivk(&src, &["ls", "--json"]);
    assert_eq!(v["count"], 0);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn changesets_survive_json_artifact_loss_and_export_is_stamped() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "task", "--json"]);
    fs::write(src.join(".ivk/workspaces/task/hello.txt"), "changed\n").unwrap();
    let v = run_ivk(&src, &["ch", "new", "task", "--json"]);
    let ch_id = v["id"].as_str().expect("changeset id").to_string();

    // The DB (not the JSON artifacts) is authoritative for queries.
    fs::remove_dir_all(src.join(".ivk/changesets")).unwrap();
    let v = run_ivk(&src, &["ch", "ls", "--json"]);
    assert_eq!(v["count"], 1, "ch ls must read from the registry: {}", v);
    assert_eq!(v["changesets"][0]["id"], ch_id.as_str());

    run_ivk(&src, &["export", &ch_id, "agent/task", "--json"]);
    let v = run_ivk(&src, &["ch", "show", &ch_id, "--json"]);
    assert_eq!(
        v["exported_branch"], "agent/task",
        "export must be stamped on the changeset: {}",
        v
    );

    let _ = fs::remove_dir_all(&root);
}

fn rev_parse(cwd: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("spawn git");
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn interrupted_ch_new_recovers_the_committed_changeset() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "task", "--json"]);
    let ws = src.join(".ivk/workspaces/task");
    fs::write(ws.join("hello.txt"), "changed\n").unwrap();

    // Simulate `ivk ch new task` killed between the commit and the metadata
    // write: journal the intent, land the commit, record nothing.
    let base = rev_parse(&ws);
    {
        let reg = Registry::open_at_root(&src).unwrap();
        reg.begin_op("ch-new", "task", Some(&base)).unwrap();
    }
    git(&ws, &["add", "-A"]);
    git(&ws, &["commit", "-q", "-m", "work"]);
    let head = rev_parse(&ws);
    let expected_id = format!("ch_{}", &head[..12]);

    let v = run_ivk(&src, &["doctor", "--json"]);
    let ops = v["registry"]["pending_ops"].as_array().expect("ops array");
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["kind"], "ch-new");
    assert_eq!(ops[0]["workspace_name"], "task");
    assert_eq!(v["next_command"], "ivk doctor --repair");

    let v = run_ivk(&src, &["doctor", "--repair", "--json"]);
    assert!(
        names(&v["repair"]["recovered_changesets"]).contains(&expected_id),
        "repair must reconstruct the changeset: {}",
        v
    );

    // The recovered changeset is fully usable: show, export, and the JSON
    // artifact exists again.
    let v = run_ivk(&src, &["ch", "show", &expected_id, "--json"]);
    assert_eq!(v["workspace_name"], "task");
    assert_eq!(v["result_snapshot"], head.as_str());
    assert_eq!(v["touched_paths"][0], "hello.txt");
    assert!(src
        .join(".ivk/changesets")
        .join(format!("{expected_id}.json"))
        .is_file());
    run_ivk(&src, &["export", &expected_id, "agent/task", "--json"]);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn interrupted_ch_new_before_commit_is_cleared_without_changeset() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "task", "--json"]);
    let ws = src.join(".ivk/workspaces/task");

    // Killed after journaling but before anything committed: HEAD == base.
    let base = rev_parse(&ws);
    {
        let reg = Registry::open_at_root(&src).unwrap();
        reg.begin_op("ch-new", "task", Some(&base)).unwrap();
    }

    let v = run_ivk(&src, &["doctor", "--repair", "--json"]);
    assert!(
        v["repair"]["recovered_changesets"]
            .as_array()
            .unwrap()
            .is_empty(),
        "nothing committed, nothing to recover: {}",
        v
    );
    assert_eq!(v["repair"]["cleared_ops"], 1);

    let v = run_ivk(&src, &["ch", "ls", "--json"]);
    assert_eq!(v["count"], 0);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn registry_rebuilds_from_directory_layout_after_db_loss() {
    let root = temp_root();
    let src = make_src_repo(&root);

    run_ivk(&src, &["new", "survivor", "--json"]);
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(src.join(format!(".ivk/db.sqlite{}", suffix)));
    }

    let v = run_ivk(&src, &["ls", "--json"]);
    assert_eq!(v["count"], 1);
    assert_eq!(v["workspaces"][0]["state"], "ready");
    assert!(
        Registry::db_path(&src).is_file(),
        "db must be recreated on open"
    );

    let _ = fs::remove_dir_all(&root);
}
