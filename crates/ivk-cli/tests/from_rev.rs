//! End-to-end tests for `ivk new --from <rev>` (plan v3 Phase B).
//!
//! The workspace is CoW-cloned from the source working tree as usual, then
//! only the paths differing between HEAD and <rev> are rewritten; files
//! ignored by git (caches, build artifacts) survive so the sharing story
//! holds for non-HEAD bases too.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

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
        "ivk-cli-from-{}-{}-{:?}",
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

fn git_capture(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {:?} failed", args);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn run_ivk(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn ivk")
}

/// Two commits + an ignored cache file in the source working tree.
fn make_two_commit_repo(root: &Path) -> PathBuf {
    let _g = INIT_LOCK.lock().unwrap();
    let _cp = CrossProcessInitLock::acquire();
    let src = root.join("src-repo");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join(".gitignore"), "cache/\n").unwrap();
    fs::write(src.join("f.txt"), "v1\n").unwrap();
    git(&src, &["init", "-q", "-b", "main", "--template="]);
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "one"]);

    fs::write(src.join("f.txt"), "v2\n").unwrap();
    fs::write(src.join("g.txt"), "new in two\n").unwrap();
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "two"]);

    // Untracked-but-ignored artifact (a stand-in for node_modules/target).
    fs::create_dir_all(src.join("cache")).unwrap();
    fs::write(src.join("cache/blob.bin"), "expensive artifact\n").unwrap();
    src
}

#[test]
fn from_rev_materializes_the_older_tree_and_keeps_ignored_files() {
    let root = temp_root();
    let src = make_two_commit_repo(&root);
    let old_sha = git_capture(&src, &["rev-parse", "HEAD~1"]);

    let out = run_ivk(&src, &["new", "old", "--from", "HEAD~1", "--json"]);
    assert!(
        out.status.success(),
        "ivk new --from failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let ws = src.join(".ivk/workspaces/old");
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).unwrap(),
        "v1\n",
        "tracked file must match the old revision"
    );
    assert!(
        !ws.join("g.txt").exists(),
        "file that does not exist at the old revision must be absent"
    );
    assert!(
        ws.join("cache/blob.bin").is_file(),
        "ignored artifacts must survive the fixup (cache sharing)"
    );
    assert_eq!(
        git_capture(&ws, &["rev-parse", "HEAD"]),
        old_sha,
        "workspace HEAD must be detached at the requested revision"
    );
    assert_eq!(
        git_capture(&ws, &["status", "--porcelain"]),
        "",
        "workspace must read clean at the old revision"
    );

    // The registry records the resolved base.
    let v: serde_json::Value =
        serde_json::from_slice(&run_ivk(&src, &["ls", "--json"]).stdout).unwrap();
    assert_eq!(v["workspaces"][0]["state"], "ready");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn from_rev_defaulting_to_head_is_unchanged() {
    let root = temp_root();
    let src = make_two_commit_repo(&root);

    let out = run_ivk(&src, &["new", "tip", "--json"]);
    assert!(out.status.success());
    let ws = src.join(".ivk/workspaces/tip");
    assert_eq!(fs::read_to_string(ws.join("f.txt")).unwrap(), "v2\n");
    assert!(ws.join("g.txt").is_file());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ws_du_reports_per_workspace_and_total_sizes() {
    let root = temp_root();
    let src = make_two_commit_repo(&root);

    assert!(run_ivk(&src, &["new", "a", "b", "--json"]).status.success());
    let v: serde_json::Value =
        serde_json::from_slice(&run_ivk(&src, &["ws", "du", "--json"]).stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["count"], 2);
    assert!(v["total_apparent_bytes"].as_u64().unwrap() > 0);
    assert!(v["total_allocated_bytes"].as_u64().unwrap() > 0);
    assert_eq!(v["workspaces"].as_array().unwrap().len(), 2);

    // Single-name form and the top-level alias.
    let v: serde_json::Value =
        serde_json::from_slice(&run_ivk(&src, &["du", "a", "--json"]).stdout).unwrap();
    assert_eq!(v["count"], 1);
    assert_eq!(v["workspaces"][0]["name"], "a");

    // Unknown name refuses.
    let out = run_ivk(&src, &["du", "nope", "--json"]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "not_found");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ws_ls_and_show_report_base_drift() {
    let root = temp_root();
    let src = make_two_commit_repo(&root);

    run_ivk(&src, &["new", "drifty"]);

    // Cut from HEAD, nothing moved yet: behind 0.
    let out = run_ivk(&src, &["ws", "ls", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("ws ls JSON");
    let row = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "drifty")
        .expect("row for drifty");
    assert_eq!(row["behind_head"], serde_json::Value::Number(0.into()));

    // The repo HEAD advances by one commit: behind 1.
    fs::write(src.join("drift-note.txt"), "moving on\n").unwrap();
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "advance"]);

    let out = run_ivk(&src, &["ws", "ls", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("ws ls JSON");
    let row = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "drifty")
        .expect("row for drifty");
    assert_eq!(row["behind_head"], serde_json::Value::Number(1.into()));

    let out = run_ivk(&src, &["ws", "show", "drifty", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("ws show JSON");
    assert_eq!(v["behind_head"], serde_json::Value::Number(1.into()));

    let _ = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["worktree", "remove", "--force"])
        .arg(src.join(".ivk/workspaces/drifty"))
        .status();
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn bad_from_rev_fails_fast_before_touching_anything() {
    let root = temp_root();
    let src = make_two_commit_repo(&root);

    let out = run_ivk(&src, &["new", "nope", "--from", "no-such-rev", "--json"]);
    assert!(!out.status.success(), "bad --from must fail");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "invalid_revision");
    assert!(
        !src.join(".ivk/workspaces/nope").exists(),
        "no workspace may be created on a bad revision"
    );

    let _ = fs::remove_dir_all(&root);
}
