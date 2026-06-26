//! End-to-end integration test for Phase 3:
//!   ivk new -> edit -> ivk ch new -> ivk export -> ivk patch
//!
//! Drives the `ivk` binary against a tiny temp git repo and asserts the
//! branch + patch artifacts land where they should.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

// Apple's git races against itself when several `git init` calls run in
// parallel against fresh dirs (template copy / config lock). The in-process
// Mutex serializes within this binary; the file lock below serializes across
// every other test binary running concurrently under `cargo test`.
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
        "ivk-cli-it-{}-{}-{:?}",
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

fn run_ivk(cwd: &Path, args: &[&str]) -> std::process::Output {
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
    out
}

fn make_src_repo(root: &Path) -> PathBuf {
    let _g = INIT_LOCK.lock().unwrap();
    let _cp = CrossProcessInitLock::acquire();
    let src = root.join("src-repo");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("hello.txt"), "hello\n").unwrap();
    // `--template=` skips template copy; Apple's git also races on config
    // locking across parallel inits, so the INIT_LOCK above keeps inits serial.
    git(&src, &["init", "-q", "-b", "main", "--template="]);
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "initial"]);
    src
}

#[test]
fn phase3_changeset_export_patch_roundtrip() {
    let root = temp_root();
    let src = make_src_repo(&root);

    // ivk new attempt-1
    run_ivk(&src, &["new", "attempt-1"]);
    let ws = src.join(".ivk/workspaces/attempt-1");
    assert!(ws.is_dir(), "workspace dir missing: {}", ws.display());

    // Modify a file inside the workspace.
    fs::write(ws.join("hello.txt"), "hello world\n").unwrap();
    fs::write(ws.join("new.txt"), "fresh\n").unwrap();

    // ivk ch new attempt-1 --json
    let out = run_ivk(&src, &["ch", "new", "attempt-1", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("ch new JSON parse failed: {} -- raw: {}", e, stdout));
    assert_eq!(
        v["ok"],
        serde_json::Value::Bool(true),
        "ch new not ok: {}",
        stdout
    );
    let ch_id = v["id"]
        .as_str()
        .expect("ch.new payload missing id")
        .to_string();
    assert!(ch_id.starts_with("ch_"), "unexpected ch id: {}", ch_id);

    // ivk export <id> agent/test --json
    let out = run_ivk(&src, &["export", &ch_id, "agent/test", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("export JSON");
    assert_eq!(v["ok"], serde_json::Value::Bool(true), "export not ok");
    assert_eq!(v["branch"], serde_json::Value::String("agent/test".into()));

    // Branch must exist in the source repo and point at the result commit.
    let branch_sha = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["rev-parse", "agent/test"])
        .output()
        .expect("git rev-parse");
    assert!(branch_sha.status.success(), "branch agent/test not created");
    let branch_sha = String::from_utf8_lossy(&branch_sha.stdout)
        .trim()
        .to_string();
    let result_sha = v["sha"].as_str().unwrap();
    assert_eq!(branch_sha, result_sha, "branch points elsewhere");

    // ivk patch <id> --json (default path: ./patches/<id>.patch)
    let out = run_ivk(&src, &["patch", &ch_id, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("patch JSON");
    assert_eq!(v["ok"], serde_json::Value::Bool(true), "patch not ok");
    let patch_path = PathBuf::from(v["output_path"].as_str().expect("output_path"));
    assert!(
        patch_path.is_file(),
        "patch file missing: {}",
        patch_path.display()
    );
    let body = fs::read_to_string(&patch_path).expect("read patch");
    assert!(
        body.contains("hello.txt"),
        "patch body missing hello.txt:\n{}",
        body
    );
    assert!(
        body.contains("hello world"),
        "patch body missing edited line:\n{}",
        body
    );
    assert!(
        body.contains("new.txt"),
        "patch body missing new file:\n{}",
        body
    );

    // ivk patch with explicit output
    let custom = root.join("out.patch");
    run_ivk(&src, &["patch", &ch_id, custom.to_str().unwrap()]);
    assert!(
        custom.is_file(),
        "custom patch path missing: {}",
        custom.display()
    );

    // ivk ch ls --json shows the changeset.
    let out = run_ivk(&src, &["ch", "ls", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("ch ls JSON");
    assert_eq!(v["count"], serde_json::Value::Number(1.into()));

    // Cleanup the worktree admin entry so the temp dir can be reaped.
    let _ = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["worktree", "remove", "--force"])
        .arg(&ws)
        .status();
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ch_new_errors_when_nothing_changed() {
    let root = temp_root();
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "noop"]);

    let out = Command::new(bin())
        .current_dir(&src)
        .args(["ch", "new", "noop", "--json"])
        .output()
        .expect("spawn ivk");
    assert!(
        !out.status.success(),
        "ch new on a clean workspace should fail"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(v["ok"], serde_json::Value::Bool(false));
    assert_eq!(
        v["error"]["code"],
        serde_json::Value::String("no_changes".into())
    );

    let _ = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["worktree", "remove", "--force"])
        .arg(src.join(".ivk/workspaces/noop"))
        .status();
    let _ = fs::remove_dir_all(&root);
}
