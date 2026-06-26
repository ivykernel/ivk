//! Phase 4 integration tests: `ivk gc`, `ivk ws rm --all`, `ivk ws rm --exported`,
//! plus the deferred-flag refusals.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

// Apple's git races on parallel template copy / config lock when multiple
// `git init` calls fire concurrently. INIT_LOCK serializes within this
// binary; CrossProcessInitLock serializes across every concurrent test binary.
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

fn temp_root(tag: &str) -> PathBuf {
    let tid = std::thread::current().id();
    let base = std::env::temp_dir().join(format!(
        "ivk-gc-it-{}-{}-{}-{:?}",
        tag,
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

fn run_ivk_allow_fail(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn ivk")
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

fn cleanup(root: &Path, src: &Path) {
    let _ = Command::new("git")
        .arg("-C")
        .arg(src)
        .args(["worktree", "prune"])
        .status();
    let _ = fs::remove_dir_all(root);
}

fn parse_json(bytes: &[u8]) -> serde_json::Value {
    let s = String::from_utf8_lossy(bytes);
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("invalid JSON: {}\n---\n{}", e, s))
}

// ---------- gc tests ----------

#[test]
fn gc_reclaims_orphan_workspace() {
    let root = temp_root("orphan");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "spike"]);

    // Break the admin entry so the workspace is an orphan.
    let admin = src.join(".git/worktrees/spike");
    fs::remove_dir_all(&admin).unwrap();

    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    let removed: Vec<String> = v["removed_workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        removed.contains(&"spike".to_string()),
        "removed: {:?}",
        removed
    );
    assert!(
        !src.join(".ivk/workspaces/spike").exists(),
        "workspace dir should be gone"
    );
    cleanup(&root, &src);
}

#[test]
fn gc_dry_run_changes_nothing() {
    let root = temp_root("dryrun");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "spike"]);
    fs::remove_dir_all(src.join(".git/worktrees/spike")).unwrap();

    let out = run_ivk(&src, &["gc", "--dry-run", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["dry_run"], true);
    let removed: Vec<String> = v["removed_workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(removed.contains(&"spike".to_string()));
    assert!(
        src.join(".ivk/workspaces/spike").exists(),
        "dry run must not touch disk"
    );

    // Real gc then removes it.
    run_ivk(&src, &["gc", "--json"]);
    assert!(!src.join(".ivk/workspaces/spike").exists());
    cleanup(&root, &src);
}

#[test]
fn gc_preserves_live_workspace_with_intact_admin() {
    let root = temp_root("livekeep");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "keep"]);
    run_ivk(&src, &["new", "gone"]);
    fs::remove_dir_all(src.join(".git/worktrees/gone")).unwrap();

    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    let removed: Vec<String> = v["removed_workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert_eq!(removed, vec!["gone".to_string()]);
    assert!(src.join(".ivk/workspaces/keep").exists());
    cleanup(&root, &src);
}

#[test]
fn gc_skips_locked_admin() {
    let root = temp_root("locked");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "spike"]);
    // Lock the worktree.
    fs::write(src.join(".git/worktrees/spike/locked"), "manual hold\n").unwrap();

    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    let locked: Vec<String> = v["skipped_locked"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect();
    // Locked workspace itself is live; its admin entry stays.
    // Now break the workspace, leaving only the locked admin → must still skip.
    fs::remove_dir_all(src.join(".ivk/workspaces/spike")).unwrap();
    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    let locked2: Vec<String> = v["skipped_locked"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        locked2.contains(&"spike".to_string()),
        "locked admin must be skipped, got skipped_locked={:?} (first run: {:?})",
        locked2,
        locked
    );
    assert!(
        src.join(".git/worktrees/spike").exists(),
        "locked admin must not be removed"
    );
    cleanup(&root, &src);
}

#[test]
fn gc_warns_on_orphaned_changeset_ref() {
    let root = temp_root("orphanch");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "ws-a"]);
    // Make a change and a changeset.
    fs::write(src.join(".ivk/workspaces/ws-a/hello.txt"), "edited\n").unwrap();
    run_ivk(&src, &["ch", "new", "ws-a"]);

    // Break the admin → ws-a becomes an orphan.
    fs::remove_dir_all(src.join(".git/worktrees/ws-a")).unwrap();
    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    let orphans = v["orphaned_changeset_refs"].as_array().unwrap();
    assert!(
        !orphans.is_empty(),
        "expected an orphaned changeset ref, got: {}",
        v
    );
    let first = &orphans[0];
    assert_eq!(first["workspace_name"], "ws-a");
    let id = first["id"].as_str().unwrap();
    // Changeset metadata must remain on disk.
    assert!(
        src.join(format!(".ivk/changesets/{}.json", id)).is_file(),
        "changeset metadata must not be deleted"
    );
    cleanup(&root, &src);
}

#[test]
fn gc_concurrent_invocation_errors() {
    let root = temp_root("concurrent");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "x"]);
    let lock = src.join(".ivk/.gc.lock");
    fs::write(&lock, "").unwrap();

    let out = run_ivk_allow_fail(&src, &["gc", "--json"]);
    assert!(!out.status.success(), "expected gc to refuse");
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "gc_in_progress");

    let _ = fs::remove_file(&lock);
    cleanup(&root, &src);
}

#[test]
fn gc_not_a_repo_errors() {
    let root = temp_root("norepo");
    let dir = root.join("not-a-repo");
    fs::create_dir_all(&dir).unwrap();
    let out = run_ivk_allow_fail(&dir, &["gc", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "not_a_repo");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gc_usage_error_on_positional() {
    let root = temp_root("usage");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["gc", "extra-arg", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "usage_error");
    cleanup(&root, &src);
}

// ---------- ws rm --all / --exported tests ----------

#[test]
fn rm_all_removes_many_workspaces() {
    // Phase 4 exit criterion: "100 temporary workspaces can be removed cleanly."
    // We use 30 here to keep the test runtime reasonable while still exercising
    // the batch path; the same code runs for 100. The product benchmark covers
    // the 100-workspace scenario.
    let root = temp_root("bulk");
    let src = make_src_repo(&root);
    let n = 30usize;
    for i in 0..n {
        run_ivk(&src, &["new", &format!("ws-{:03}", i)]);
    }
    assert_eq!(
        fs::read_dir(src.join(".ivk/workspaces")).unwrap().count(),
        n
    );

    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["selector"], "all");
    let removed = v["removed"].as_array().unwrap();
    assert_eq!(removed.len(), n, "expected {} removed", n);
    // Bulk rm must report bytes accounting to match gc's envelope shape.
    assert!(v["bytes_before"].is_u64(), "rm-bulk bytes_before missing");
    assert!(v["bytes_after"].is_u64(), "rm-bulk bytes_after missing");
    assert!(
        v["bytes_reclaimed"].is_u64(),
        "rm-bulk bytes_reclaimed missing"
    );
    assert!(
        v["bytes_reclaimed_human"].is_string(),
        "rm-bulk bytes_reclaimed_human missing"
    );
    assert_eq!(
        fs::read_dir(src.join(".ivk/workspaces")).unwrap().count(),
        0
    );

    // gc after should report bytes_reclaimed as an integer field (likely 0 now).
    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    assert!(v["bytes_before"].is_u64(), "bytes_before must be present");
    assert!(v["bytes_after"].is_u64(), "bytes_after must be present");
    assert!(
        v["bytes_reclaimed"].is_u64(),
        "bytes_reclaimed must be present"
    );
    cleanup(&root, &src);
}

#[test]
fn rm_all_without_yes_refuses() {
    let root = temp_root("noyes");
    let src = make_src_repo(&root);
    for i in 0..3 {
        run_ivk(&src, &["new", &format!("w{}", i)]);
    }
    let out = run_ivk_allow_fail(&src, &["ws", "rm", "--all", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "confirmation_required");
    assert_eq!(v["next_command"], "ivk ws rm --all --yes");
    assert_eq!(
        fs::read_dir(src.join(".ivk/workspaces")).unwrap().count(),
        3,
        "no workspace should be removed"
    );
    cleanup(&root, &src);
}

#[test]
fn rm_all_skips_dirty_without_force() {
    let root = temp_root("dirty");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "clean"]);
    run_ivk(&src, &["new", "dirty"]);
    fs::write(src.join(".ivk/workspaces/dirty/hello.txt"), "changed\n").unwrap();

    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--json"]);
    let v = parse_json(&out.stdout);
    let removed: Vec<String> = v["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    let skipped: Vec<String> = v["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        removed.contains(&"clean".to_string()),
        "removed: {:?}",
        removed
    );
    assert!(
        skipped.contains(&"dirty".to_string()),
        "skipped: {:?}",
        skipped
    );
    assert!(src.join(".ivk/workspaces/dirty").exists());
    cleanup(&root, &src);
}

#[test]
fn rm_all_force_removes_dirty() {
    let root = temp_root("force");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "a"]);
    run_ivk(&src, &["new", "b"]);
    fs::write(src.join(".ivk/workspaces/b/hello.txt"), "changed\n").unwrap();

    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--force", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["removed"].as_array().unwrap().len(), 2);
    assert_eq!(v["skipped"].as_array().unwrap().len(), 0);
    cleanup(&root, &src);
}

#[test]
fn rm_force_does_not_bypass_locked_guard() {
    // Critical safety contract: --force overrides the dirty guard, NEVER the
    // locked-worktree guard. Locked worktrees are held by an external process.
    let root = temp_root("forcelock");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "held"]);
    fs::write(src.join(".git/worktrees/held/locked"), "manual\n").unwrap();

    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--force", "--json"]);
    let v = parse_json(&out.stdout);
    let removed: Vec<String> = v["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    let skipped: Vec<String> = v["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !removed.contains(&"held".to_string()),
        "locked workspace must NEVER be removed, even with --force; got removed={:?}",
        removed
    );
    assert!(
        skipped.contains(&"held".to_string()),
        "locked workspace must be in skipped, got: {:?}",
        skipped
    );
    assert!(src.join(".ivk/workspaces/held").exists());
    cleanup(&root, &src);
}

#[test]
fn rm_skips_unknown_status_without_force() {
    // If git_status_in fails (corrupt .git pointer), bulk rm must NOT silently
    // remove the workspace. The unknown-status guard mirrors the dirty guard.
    let root = temp_root("unknown");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "broken"]);
    // Break the .git pointer file so `git status` fails inside the workspace.
    fs::write(
        src.join(".ivk/workspaces/broken/.git"),
        "gitdir: /nonexistent\n",
    )
    .unwrap();

    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--json"]);
    let v = parse_json(&out.stdout);
    let skipped: Vec<(String, String)> = v["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["name"].as_str().unwrap().to_string(),
                x["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(
        skipped
            .iter()
            .any(|(n, r)| n == "broken" && r.contains("unknown")),
        "broken workspace must be skipped with unknown-status reason, got: {:?}",
        skipped
    );
    assert!(src.join(".ivk/workspaces/broken").exists());
    cleanup(&root, &src);
}

#[test]
fn rm_single_name_agent_payload_has_next_steps() {
    // Agent-readability fix: single-name rm must include recommended_next_steps
    // when --agent is set, mirroring bulk rm.
    let root = temp_root("singleagent");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "t"]);
    let out = run_ivk(&src, &["rm", "t", "--agent", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["next_command"], "ivk gc");
    let steps = v["recommended_next_steps"].as_array().expect("steps");
    assert!(
        !steps.is_empty(),
        "recommended_next_steps must be non-empty"
    );
    cleanup(&root, &src);
}

#[test]
fn rm_single_name_failure_points_to_doctor() {
    let root = temp_root("singlefail");
    let src = make_src_repo(&root);
    // No such workspace.
    let out = Command::new(bin())
        .current_dir(&src)
        .args(["rm", "nope", "--json"])
        .output()
        .expect("spawn");
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["next_command"], "ivk doctor");
    cleanup(&root, &src);
}

#[test]
fn rm_skip_only_next_command_suggests_force() {
    // When every candidate is skipped, next_command must point at the retry
    // command (--force) so a calling agent has an actionable next step.
    let root = temp_root("skipOnly");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "x"]);
    fs::write(src.join(".ivk/workspaces/x/hello.txt"), "dirty\n").unwrap();

    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["removed"].as_array().unwrap().len(), 0);
    assert!(!v["skipped"].as_array().unwrap().is_empty());
    assert_eq!(v["next_command"], "ivk ws rm --all --yes --force");
    cleanup(&root, &src);
}

#[test]
fn gc_stale_lock_is_recovered() {
    // A .gc.lock older than STALE_LOCK_SECS (300s) must be taken over so
    // crashed prior runs don't permanently block recovery.
    let root = temp_root("stalelock");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "z"]);
    let lock = src.join(".ivk/.gc.lock");
    fs::write(&lock, "").unwrap();
    // Backdate the lock by 1 hour.
    let one_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    filetime::set_file_mtime(&lock, filetime::FileTime::from_system_time(one_hour_ago))
        .expect("set mtime");
    // gc must succeed (no gc_in_progress) by taking over the stale lock.
    let out = run_ivk(&src, &["gc", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true, "stale lock should not block gc: {}", v);
    cleanup(&root, &src);
}

#[test]
fn rm_exported_selects_only_branch_matched() {
    let root = temp_root("exported");
    let src = make_src_repo(&root);
    for n in ["a", "b", "c"] {
        run_ivk(&src, &["new", n]);
    }
    // Make `a` exported: edit + ch + export.
    fs::write(src.join(".ivk/workspaces/a/hello.txt"), "from a\n").unwrap();
    let out = run_ivk(&src, &["ch", "new", "a", "--json"]);
    let v = parse_json(&out.stdout);
    let ch_id = v["id"].as_str().unwrap().to_string();
    run_ivk(&src, &["export", &ch_id, "agent/a", "--json"]);

    let out = run_ivk(&src, &["ws", "rm", "--exported", "--yes", "--json"]);
    let v = parse_json(&out.stdout);
    let removed: Vec<String> = v["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert_eq!(removed, vec!["a".to_string()]);
    assert!(!src.join(".ivk/workspaces/a").exists());
    assert!(src.join(".ivk/workspaces/b").exists());
    assert!(src.join(".ivk/workspaces/c").exists());
    cleanup(&root, &src);
}

#[test]
fn rm_exported_skips_when_branch_advanced() {
    let root = temp_root("advanced");
    let src = make_src_repo(&root);
    run_ivk(&src, &["new", "a"]);
    fs::write(src.join(".ivk/workspaces/a/hello.txt"), "from a\n").unwrap();
    let out = run_ivk(&src, &["ch", "new", "a", "--json"]);
    let ch_id = parse_json(&out.stdout)["id"].as_str().unwrap().to_string();
    run_ivk(&src, &["export", &ch_id, "agent/a", "--json"]);

    // Advance the branch beyond the workspace's HEAD with a new commit.
    git(&src, &["checkout", "agent/a"]);
    fs::write(src.join("hello.txt"), "from main repo\n").unwrap();
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "advance"]);
    git(&src, &["checkout", "main"]);

    let out = run_ivk(&src, &["ws", "rm", "--exported", "--yes", "--json"]);
    let v = parse_json(&out.stdout);
    let removed = v["removed"].as_array().unwrap();
    assert_eq!(
        removed.len(),
        0,
        "branch advanced past HEAD; must not match"
    );
    assert!(src.join(".ivk/workspaces/a").exists());
    cleanup(&root, &src);
}

#[test]
fn rm_dry_run_does_not_touch_disk() {
    let root = temp_root("rmdry");
    let src = make_src_repo(&root);
    for n in ["a", "b"] {
        run_ivk(&src, &["new", n]);
    }
    let out = run_ivk(&src, &["ws", "rm", "--all", "--yes", "--dry-run", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["removed"].as_array().unwrap().len(), 2);
    assert!(src.join(".ivk/workspaces/a").exists());
    assert!(src.join(".ivk/workspaces/b").exists());
    cleanup(&root, &src);
}

#[test]
fn rm_failed_returns_unsupported_flag() {
    let root = temp_root("failed");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["ws", "rm", "--failed", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "unsupported_flag");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("test-result tracking"),
        "unexpected message: {}",
        msg
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn rm_all_discarded_returns_unsupported_flag() {
    let root = temp_root("allDisc");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["ws", "rm", "--all-discarded", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "unsupported_flag");
    assert!(
        v["next_command"].as_str().unwrap().contains("--exported"),
        "next_command should point at --exported, got: {}",
        v["next_command"]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn rm_conflicting_flags_errors() {
    let root = temp_root("conflict");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["ws", "rm", "--all", "--exported", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "conflicting_args");
    let _ = fs::remove_dir_all(&root);
}
