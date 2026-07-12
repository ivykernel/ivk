//! End-to-end integration tests for `ivk-core::materialize_workspace`.
//!
//! These tests build a tiny on-disk git repo in a `tempdir`-style location,
//! materialize a workspace from it, and verify:
//!   - the destination exists
//!   - the working tree files match the source byte-for-byte
//!   - `git status` is clean inside the workspace
//!   - `.git` is NOT cloned (it's the worktree pointer file, not the dir)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use ivk_core::{materialize_workspace, MaterializeOptions};

// Apple's git races on parallel `git init` template copy + config locks. Hold
// this mutex around every `git init` / `git add -A` block in these tests.
// CrossProcessInitLock serializes across other test binaries running under
// the same `cargo test` invocation.
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

fn temp_root() -> PathBuf {
    // Nanosecond timestamp + thread id (low-res clock collisions on macOS can
    // otherwise let two concurrent tests pick the same path).
    let tid = std::thread::current().id();
    let base = std::env::temp_dir().join(format!(
        "ivk-core-it-{}-{}-{:?}",
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

fn make_src_repo(root: &Path) -> PathBuf {
    let _guard = INIT_LOCK.lock().unwrap();
    let _cp = CrossProcessInitLock::acquire();
    let src = root.join("src-repo");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(src.join("a")).unwrap();
    fs::create_dir_all(src.join("b/c")).unwrap();
    fs::write(src.join("README.md"), "hello\n").unwrap();
    fs::write(src.join("a/one.txt"), "alpha\n").unwrap();
    fs::write(src.join("b/c/two.txt"), "beta\n").unwrap();

    let git = |args: &[&str]| {
        let s = Command::new("git")
            .arg("-C")
            .arg(&src)
            .args(args)
            .status()
            .unwrap();
        assert!(s.success(), "git {:?} failed", args);
    };
    git(&["init", "-q", "-b", "main", "--template="]);
    git(&["-c", "user.email=t@test", "-c", "user.name=t", "add", "-A"]);
    git(&[
        "-c",
        "user.email=t@test",
        "-c",
        "user.name=t",
        "commit",
        "-q",
        "-m",
        "initial",
    ]);
    src
}

#[test]
fn materializes_workspace_and_keeps_status_clean() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let dst = root.join("ws-1");

    let report = materialize_workspace(&MaterializeOptions {
        src: src.clone(),
        dst: dst.clone(),
        with_git: true,
        rev: None,
    })
    .expect("materialize");

    // Working-tree files present and unchanged.
    assert!(dst.join("README.md").is_file(), "README.md missing");
    assert!(dst.join("a/one.txt").is_file(), "a/one.txt missing");
    assert!(dst.join("b/c/two.txt").is_file(), "b/c/two.txt missing");
    assert_eq!(
        fs::read_to_string(dst.join("a/one.txt")).unwrap(),
        "alpha\n"
    );

    // `.git` is a *file* (worktree pointer), not a directory.
    let dot_git = dst.join(".git");
    assert!(
        dot_git.exists() && dot_git.is_file(),
        ".git must exist as a worktree pointer file, got is_dir={}, is_file={}",
        dot_git.is_dir(),
        dot_git.is_file()
    );

    // git status inside dst must be clean.
    let out = Command::new("git")
        .arg("-C")
        .arg(&dst)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(out.status.success(), "git status failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim().is_empty(),
        "expected clean git status in workspace, got:\n{}",
        stdout
    );

    // Report sanity.
    assert!(report.cloned_entries >= 3, "expected ≥3 cloned entries");
    assert_eq!(
        report.skipped_entries, 1,
        ".git should be the only skipped entry"
    );

    // Cleanup the worktree admin entry to avoid polluting the source repo.
    let _ = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["worktree", "remove", "--force"])
        .arg(&dst)
        .status();
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn rejects_existing_destination() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let dst = root.join("ws-2");
    fs::create_dir_all(&dst).unwrap();

    let err = materialize_workspace(&MaterializeOptions {
        src: src.clone(),
        dst: dst.clone(),
        with_git: true,
        rev: None,
    })
    .expect_err("must fail because dst exists");

    let msg = err.to_string();
    assert!(msg.contains("already exists"), "unexpected error: {}", msg);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn no_git_mode_skips_git_setup() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let dst = root.join("ws-3");

    let report = materialize_workspace(&MaterializeOptions {
        src: src.clone(),
        dst: dst.clone(),
        with_git: false,
        rev: None,
    })
    .expect("materialize without git");

    // Files cloned, but no `.git` pointer.
    assert!(dst.join("README.md").is_file());
    assert!(
        !dst.join(".git").exists(),
        "with_git=false must not create .git"
    );
    assert!(report.git_worktree_add.is_none());
    assert!(report.git_read_tree.is_none());

    let _ = fs::remove_dir_all(&root);
}
