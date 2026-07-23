//! Integration tests for the Phase A seams: `GitBackend` (via
//! `GitCliBackend`) and `Materializer` (via `CopyMaterializer`).
//!
//! These exercise every trait method against a real on-disk repo. When the
//! libgit2 backend lands (Phase C), the same scenarios become the parity
//! suite: run against both backends, diff the results.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use ivk_core::{
    materialize_workspace_with, remove_workspace, CommitIdentity, CopyMaterializer, DiffTarget,
    GitBackend, GitCliBackend, MaterializeOptions, Materializer,
};

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
    let tid = std::thread::current().id();
    let base = std::env::temp_dir().join(format!(
        "ivk-core-be-{}-{}-{:?}",
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
    fs::create_dir_all(src.join("a")).unwrap();
    fs::write(src.join("README.md"), "hello\n").unwrap();
    fs::write(src.join("a/one.txt"), "alpha\n").unwrap();

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
fn resolves_revisions_and_reads_status() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let git = GitCliBackend::new();

    let full = git.resolve_revision(&src, "HEAD").expect("resolve HEAD");
    assert_eq!(full.len(), 40, "expected full sha, got: {}", full);
    let short = git
        .resolve_revision_short(&src, "HEAD")
        .expect("resolve short HEAD");
    assert!(
        full.starts_with(&short) && short.len() < full.len(),
        "short sha {} should be a proper prefix of {}",
        short,
        full
    );
    assert!(git.resolve_revision(&src, "no-such-rev").is_err());

    let clean = git.status(&src).expect("status");
    assert!(!clean.is_dirty(), "fresh repo must be clean");

    fs::write(src.join("new.txt"), "x\n").unwrap();
    let dirty = git.status(&src).expect("status");
    assert!(dirty.is_dirty());
    assert_eq!(dirty.entries.len(), 1);
    assert_eq!(dirty.entries[0].code, "??");
    assert_eq!(dirty.entries[0].path, "new.txt");
    assert_eq!(dirty.touched_paths(), vec!["new.txt".to_string()]);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn commits_diffs_and_branches_roundtrip() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let git = GitCliBackend::new();

    let base = git.resolve_revision(&src, "HEAD").unwrap();

    // One modified line + worktree diff before committing.
    fs::write(src.join("a/one.txt"), "alpha\nbeta\n").unwrap();
    let stat = git
        .diff_stat(&src, DiffTarget::WorktreeToHead)
        .expect("worktree diff stat");
    assert_eq!(stat.files_changed, 1);
    assert_eq!(stat.insertions, 1);
    assert_eq!(stat.deletions, 0);
    let patch = git
        .diff_patch(&src, DiffTarget::WorktreeToHead, false)
        .expect("worktree diff patch");
    let patch_text = String::from_utf8_lossy(&patch);
    assert!(patch_text.contains("+beta"), "patch:\n{}", patch_text);

    // The kernel's one committing operation.
    let head = git
        .stage_all_and_commit(&src, "test: change one.txt", &CommitIdentity::ivk_default())
        .expect("commit");
    assert_eq!(head.len(), 40);
    assert_ne!(head, base, "commit must advance HEAD");
    assert!(!git.status(&src).unwrap().is_dirty(), "clean after commit");

    // Commit-range diff matches what we just did.
    let range = DiffTarget::CommitRange {
        base: &base,
        head: &head,
    };
    let stat = git.diff_stat(&src, range).expect("range diff stat");
    assert_eq!(
        (stat.files_changed, stat.insertions, stat.deletions),
        (1, 1, 0)
    );
    let patch = git.diff_patch(&src, range, true).expect("range diff patch");
    assert!(String::from_utf8_lossy(&patch).contains("+beta"));

    // Branch + ref listing.
    git.create_branch(&src, "agent/test-branch", &head, false)
        .expect("create branch");
    git.create_branch(&src, "agent/test-branch", &base, true)
        .expect("force-move branch");
    let refs = git.list_refs(&src, "refs/heads/agent/").expect("list refs");
    let entry = refs
        .iter()
        .find(|r| r.name == "agent/test-branch")
        .expect("branch listed");
    assert_eq!(entry.sha, base, "force-move must repoint the branch");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn merge_check_reports_clean_and_conflicted_merges() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let git = GitCliBackend::new();

    let run = |args: &[&str]| {
        let s = Command::new("git")
            .arg("-C")
            .arg(&src)
            .args(args)
            .status()
            .unwrap();
        assert!(s.success(), "git {:?} failed", args);
    };

    let base = git.resolve_revision(&src, "HEAD").unwrap();

    // "theirs": a changeset-style commit touching a/one.txt.
    fs::write(src.join("a/one.txt"), "alpha\ntheirs\n").unwrap();
    let theirs = git
        .stage_all_and_commit(&src, "theirs", &CommitIdentity::ivk_default())
        .unwrap();

    // Drift counting piggybacks on the same commit graph.
    assert_eq!(git.commits_ahead(&src, &base, &theirs).unwrap(), 1);
    assert_eq!(git.commits_ahead(&src, &theirs, &base).unwrap(), 0);
    assert!(git.commits_ahead(&src, "no-such-rev", &base).is_err());

    // "ours" advanced the base on a different file — must merge cleanly.
    run(&["checkout", "-q", "--detach", &base]);
    fs::write(src.join("README.md"), "hello\nours\n").unwrap();
    let ours = git
        .stage_all_and_commit(&src, "ours", &CommitIdentity::ivk_default())
        .unwrap();
    let check = git
        .merge_check(&src, &base, &ours, &theirs)
        .expect("clean check");
    assert!(check.clean, "disjoint edits must merge cleanly");
    assert!(check.conflict_paths.is_empty());
    assert_eq!(
        check.merged_tree.len(),
        40,
        "tree oid: {}",
        check.merged_tree
    );

    // "ours" touching the same line of the same file — must conflict.
    run(&["checkout", "-q", "--detach", &base]);
    fs::write(src.join("a/one.txt"), "alpha\nours\n").unwrap();
    let ours2 = git
        .stage_all_and_commit(&src, "ours2", &CommitIdentity::ivk_default())
        .unwrap();
    let check = git
        .merge_check(&src, &base, &ours2, &theirs)
        .expect("conflicted check is still Ok");
    assert!(!check.clean);
    assert_eq!(check.conflict_paths, vec!["a/one.txt".to_string()]);
    assert_eq!(
        check.merged_tree.len(),
        40,
        "conflicted merge still writes a tree"
    );

    // Unresolvable input is an Err, not a conflicted result.
    assert!(git
        .merge_check(&src, &base, "no-such-rev", &theirs)
        .is_err());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn copy_materializer_clones_files_dirs_and_symlinks() {
    let root = temp_root();
    let src = root.join("tree");
    fs::create_dir_all(src.join("d1/d2")).unwrap();
    fs::write(src.join("top.txt"), "top\n").unwrap();
    fs::write(src.join("d1/d2/deep.txt"), "deep\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("top.txt", src.join("link")).unwrap();

    let dst = root.join("out");
    let m = CopyMaterializer;
    assert_eq!(m.strategy(), "std-copy");
    m.clone_path(&src, &dst).expect("copy tree");

    assert_eq!(fs::read_to_string(dst.join("top.txt")).unwrap(), "top\n");
    assert_eq!(
        fs::read_to_string(dst.join("d1/d2/deep.txt")).unwrap(),
        "deep\n"
    );
    #[cfg(unix)]
    {
        let md = fs::symlink_metadata(dst.join("link")).unwrap();
        assert!(md.file_type().is_symlink(), "symlink must stay a symlink");
        assert_eq!(
            fs::read_link(dst.join("link")).unwrap(),
            PathBuf::from("top.txt")
        );
    }

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn materialize_with_copy_backend_and_remove_workspace() {
    let root = temp_root();
    let src = make_src_repo(&root);
    let git = GitCliBackend::new();

    let dst = root.join("ws-copy");
    let report = materialize_workspace_with(
        &git,
        &CopyMaterializer,
        &MaterializeOptions {
            src: src.clone(),
            dst: dst.clone(),
            with_git: true,
            rev: None,
        },
    )
    .expect("materialize with copy backend");
    assert_eq!(report.strategy, "std-copy");
    assert!(dst.join(".git").is_file(), ".git worktree pointer expected");
    assert!(
        !git.status(&dst).expect("status in workspace").is_dirty(),
        "materialized workspace must read clean"
    );

    // Kernel-level removal: worktree admin + directory, then prune.
    remove_workspace(&git, &src, &dst).expect("remove workspace");
    assert!(!dst.exists(), "workspace dir must be gone");

    // A manually deleted worktree leaves a stale admin entry; prune reports
    // it. git's default expiry (gc.worktreePruneExpire = 3 months) protects
    // young entries, so pin it to `now` for the fixture — production `ivk gc`
    // does not rely on git's expiry (it removes stale admin dirs itself).
    let dst2 = root.join("ws-stale");
    git.add_worktree(&src, &dst2, "HEAD").expect("add worktree");
    fs::remove_dir_all(&dst2).unwrap();
    let cfg = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["config", "gc.worktreePruneExpire", "now"])
        .status()
        .unwrap();
    assert!(cfg.success());
    let pruned = git.prune_worktrees(&src).expect("prune");
    assert!(
        pruned.iter().any(|n| n == "ws-stale"),
        "stale admin entry should be pruned, got: {:?}",
        pruned
    );

    let _ = fs::remove_dir_all(&root);
}
