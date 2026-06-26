//! Phase 5 integration tests for `ivk bench *`.

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

fn temp_root(tag: &str) -> PathBuf {
    let tid = std::thread::current().id();
    let base = std::env::temp_dir().join(format!(
        "ivk-bench-it-{}-{}-{}-{:?}",
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
    fs::write(src.join("README.md"), "readme\n").unwrap();
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("sub/a.txt"), "a\n").unwrap();
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

// ---------- bench help / dispatch ----------

#[test]
fn bench_help_lists_subcommands() {
    let root = temp_root("help");
    let src = make_src_repo(&root);
    let out = run_ivk(&src, &["bench", "help"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("spawn"));
    assert!(s.contains("compare-git-worktree"));
    assert!(s.contains("disk"));
    cleanup(&root, &src);
}

#[test]
fn bench_unknown_subcommand_errors() {
    let root = temp_root("badsub");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["bench", "wat", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "usage_error");
    cleanup(&root, &src);
}

// ---------- spawn ----------

#[test]
fn bench_spawn_count_3_emits_three_and_cleans_up() {
    let root = temp_root("spawn3");
    let src = make_src_repo(&root);
    let out = run_ivk(&src, &["bench", "spawn", "--count", "3", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["workspaces"]["created"], 3);
    assert_eq!(v["params"]["count"], 3);
    assert!(v["timings_ms"]["total_wall_ms"].as_f64().unwrap() >= 0.0);
    // Bench dir cleaned up.
    let bench_dir = src.join(".ivk/bench");
    if bench_dir.exists() {
        let entries: Vec<_> = fs::read_dir(&bench_dir).unwrap().flatten().collect();
        assert!(entries.is_empty(), "bench dir not cleaned: {:?}", entries);
    }
    cleanup(&root, &src);
}

#[test]
fn bench_spawn_refuses_outside_repo() {
    let root = temp_root("noregit");
    let dir = root.join("not-a-repo");
    fs::create_dir_all(&dir).unwrap();
    let out = run_ivk_allow_fail(&dir, &["bench", "spawn", "--count", "1", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "not_a_git_repo");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn bench_spawn_refuses_no_commits() {
    let root = temp_root("nocommit");
    let src = root.join("fresh");
    fs::create_dir_all(&src).unwrap();
    let _l = INIT_LOCK.lock().unwrap();
    git(&src, &["init", "-q", "-b", "main", "--template="]);
    drop(_l);
    let out = run_ivk_allow_fail(&src, &["bench", "spawn", "--count", "1", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "no_commits");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn bench_spawn_percentiles_monotone() {
    let root = temp_root("pmono");
    let src = make_src_repo(&root);
    let out = run_ivk(&src, &["bench", "spawn", "--count", "5", "--json"]);
    let v = parse_json(&out.stdout);
    let p50 = v["timings_ms"]["per_workspace"]["p50_ms"].as_f64().unwrap();
    let p90 = v["timings_ms"]["per_workspace"]["p90_ms"].as_f64().unwrap();
    let p99 = v["timings_ms"]["per_workspace"]["p99_ms"].as_f64().unwrap();
    assert!(p50 <= p90, "p50 {} > p90 {}", p50, p90);
    assert!(p90 <= p99, "p90 {} > p99 {}", p90, p99);
    cleanup(&root, &src);
}

#[test]
fn bench_spawn_bad_count_rejected() {
    let root = temp_root("badcnt");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["bench", "spawn", "--count", "0", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "usage_error");
    cleanup(&root, &src);
}

// ---------- compare-git-worktree ----------

#[test]
fn bench_compare_emits_both_arms_and_lp_blurb() {
    let root = temp_root("cmp");
    let src = make_src_repo(&root);
    let out = run_ivk(
        &src,
        &["bench", "compare-git-worktree", "--count", "3", "--json"],
    );
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert!(v["arms"]["ivk"].is_object());
    assert!(v["arms"]["git_worktree"].is_object());
    let lp = v["comparison"]["lp_blurb"].as_str().unwrap();
    assert!(lp.contains("ivk spawned"), "lp_blurb: {}", lp);
    assert!(lp.contains("3 workspaces"), "lp_blurb: {}", lp);
    let order = v["params"]["execution_order"].as_array().unwrap();
    assert_eq!(order.len(), 2);
    let bench_dir = src.join(".ivk/bench");
    if bench_dir.exists() {
        let entries: Vec<_> = fs::read_dir(&bench_dir).unwrap().flatten().collect();
        assert!(
            entries.is_empty(),
            "compare did not clean up: {:?}",
            entries
        );
    }
    cleanup(&root, &src);
}

#[test]
fn bench_compare_cleans_up_git_worktree_admin() {
    let root = temp_root("cmpadmin");
    let src = make_src_repo(&root);
    run_ivk(
        &src,
        &["bench", "compare-git-worktree", "--count", "2", "--json"],
    );
    let out = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let body = String::from_utf8_lossy(&out.stdout);
    // Only the main worktree should remain; no admin entries with bench prefix.
    assert!(
        !body.contains("b-compare-"),
        "stale worktree admin: {}",
        body
    );
    cleanup(&root, &src);
}

// ---------- disk ----------

#[test]
fn bench_disk_emits_triad_and_lp_blurb() {
    let root = temp_root("disk");
    let src = make_src_repo(&root);
    let out = run_ivk(&src, &["bench", "disk", "--count", "4", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert!(v["workspaces"]["apparent_kb"].is_u64());
    assert!(v["workspaces"]["actual_kb_du_blocks"].is_u64());
    assert!(v["workspaces"]["real_kb_df_delta"].is_u64());
    let notes = v["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 3);
    let blurb = v["ratios"]["lp_blurb"].as_str().unwrap();
    assert!(blurb.contains("4 workspaces"), "blurb: {}", blurb);
    cleanup(&root, &src);
}

// ---------- gc ----------

#[test]
fn bench_gc_removes_orphan_half_and_preserves_user_workspaces() {
    let root = temp_root("gcbench");
    let src = make_src_repo(&root);
    // Pre-existing user workspace must survive bench gc.
    run_ivk(&src, &["new", "keep-me"]);
    let out = run_ivk(&src, &["bench", "gc", "--count", "6", "--json"]);
    let v = parse_json(&out.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["setup"]["materialized"], 6);
    assert_eq!(v["setup"]["broken_for_orphan_test"], 3);
    // gc should have removed at least the broken half.
    let removed = v["execute"]["removed_workspaces"].as_u64().unwrap();
    assert!(removed >= 3, "removed: {}", removed);
    assert!(v["execute"]["gc_total_ms"].as_f64().unwrap() >= 0.0);
    assert_eq!(v["sanity"]["leftover_admin_entries"], 0);
    // User workspace preserved.
    assert!(src.join(".ivk/workspaces/keep-me").exists());
    cleanup(&root, &src);
}

#[test]
fn bench_compare_count_1_warns_about_sample_size() {
    // Phase 5 review fix: warnings array must flag n=1 runs as statistically
    // meaningless.
    let root = temp_root("cmp1");
    let src = make_src_repo(&root);
    let out = run_ivk(
        &src,
        &["bench", "compare-git-worktree", "--count", "1", "--json"],
    );
    let v = parse_json(&out.stdout);
    let w: Vec<String> = v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        w.iter().any(|s| s.contains("n=1")),
        "expected n=1 warning, got: {:?}",
        w
    );
    // speedup_total must be finite — INFINITY breaks JSON.
    let st = v["comparison"]["speedup_total"].as_f64().unwrap();
    assert!(st.is_finite(), "speedup_total not finite: {}", st);
    cleanup(&root, &src);
}

#[test]
fn bench_compare_lp_blurb_never_says_inf() {
    // Even when timings round to zero, lp_blurb must use fmt_speedup which
    // emits ">100x" instead of the bare floating-point inf.
    let root = temp_root("cmpinf");
    let src = make_src_repo(&root);
    let out = run_ivk(
        &src,
        &["bench", "compare-git-worktree", "--count", "1", "--json"],
    );
    let v = parse_json(&out.stdout);
    let blurb = v["comparison"]["lp_blurb"].as_str().unwrap();
    assert!(!blurb.contains("inf"), "lp_blurb leaked 'inf': {}", blurb);
    assert!(
        !blurb.to_lowercase().contains("nan"),
        "lp_blurb leaked NaN: {}",
        blurb
    );
    cleanup(&root, &src);
}

#[test]
fn bench_disk_zero_df_delta_does_not_emit_infinity() {
    // df-delta is often 0 on test-sized repos; the lp_blurb must read sensibly
    // and savings_ratio must be JSON-finite.
    let root = temp_root("disk0");
    let src = make_src_repo(&root);
    let out = run_ivk(&src, &["bench", "disk", "--count", "2", "--json"]);
    let v = parse_json(&out.stdout);
    let blurb = v["ratios"]["lp_blurb"].as_str().unwrap();
    assert!(!blurb.contains("inf"), "lp_blurb leaked 'inf': {}", blurb);
    let sr = v["ratios"]["savings_ratio"].as_f64().unwrap();
    assert!(sr.is_finite(), "savings_ratio not finite: {}", sr);
    cleanup(&root, &src);
}

#[test]
fn bench_gc_count_too_small_rejected() {
    let root = temp_root("gcsmall");
    let src = make_src_repo(&root);
    let out = run_ivk_allow_fail(&src, &["bench", "gc", "--count", "1", "--json"]);
    assert!(!out.status.success());
    let v = parse_json(&out.stdout);
    assert_eq!(v["error"]["code"], "usage_error");
    cleanup(&root, &src);
}
