//! `GitBackend` implemented by shelling out to the `git` binary.
//!
//! This is the desktop default and the compatibility reference: whatever the
//! user's installed git does is by definition correct. Output parsing sticks
//! to plumbing-ish, stable formats (`--porcelain`, `--numstat`,
//! `for-each-ref --format`).

use std::path::Path;
use std::process::Command;

use super::{
    CommitIdentity, DiffStat, DiffTarget, GitBackend, GitError, MergeCheck, RefEntry, StatusEntry,
    StatusSummary,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct GitCliBackend;

impl GitCliBackend {
    pub fn new() -> Self {
        Self
    }
}

fn git_in(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir);
    c
}

/// Run to completion, capturing output. Non-zero exit becomes a `GitError`
/// carrying the trimmed stderr.
fn capture(mut cmd: Command, op: &'static str) -> Result<Vec<u8>, GitError> {
    let out = cmd.output().map_err(|e| GitError {
        op,
        message: format!("could not launch git: {e}"),
    })?;
    if !out.status.success() {
        return Err(GitError {
            op,
            message: format!(
                "exited with {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        });
    }
    Ok(out.stdout)
}

fn capture_string(cmd: Command, op: &'static str) -> Result<String, GitError> {
    capture(cmd, op).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn single_line(cmd: Command, op: &'static str) -> Result<String, GitError> {
    let s = capture_string(cmd, op)?;
    let line = s.trim();
    if line.is_empty() {
        return Err(GitError {
            op,
            message: "empty output".into(),
        });
    }
    Ok(line.to_string())
}

impl GitBackend for GitCliBackend {
    fn name(&self) -> &'static str {
        "git-cli"
    }

    fn resolve_revision(&self, repo: &Path, rev: &str) -> Result<String, GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["rev-parse", rev]);
        single_line(cmd, "rev-parse")
    }

    fn resolve_revision_short(&self, repo: &Path, rev: &str) -> Result<String, GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["rev-parse", "--short", rev]);
        single_line(cmd, "rev-parse")
    }

    fn status(&self, worktree: &Path) -> Result<StatusSummary, GitError> {
        let mut cmd = git_in(worktree);
        cmd.args(["status", "--porcelain"]);
        let out = capture_string(cmd, "status")?;
        let entries = out
            .lines()
            .filter(|l| l.len() >= 4)
            .map(|l| StatusEntry {
                code: l[..2].to_string(),
                path: l[3..].to_string(),
            })
            .collect();
        Ok(StatusSummary { entries })
    }

    fn diff_stat(&self, repo: &Path, target: DiffTarget) -> Result<DiffStat, GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["diff", "--numstat"]);
        apply_target(&mut cmd, target);
        let out = capture_string(cmd, "diff")?;
        let mut stat = DiffStat::default();
        for line in out.lines() {
            let mut cols = line.split('\t');
            let ins = cols.next().unwrap_or("");
            let del = cols.next().unwrap_or("");
            if cols.next().is_none() {
                continue; // not a numstat row
            }
            stat.files_changed += 1;
            // Binary files show "-" — count the file, add zero lines.
            stat.insertions += ins.parse::<u32>().unwrap_or(0);
            stat.deletions += del.parse::<u32>().unwrap_or(0);
        }
        Ok(stat)
    }

    fn diff_patch(
        &self,
        repo: &Path,
        target: DiffTarget,
        binary: bool,
    ) -> Result<Vec<u8>, GitError> {
        let mut cmd = git_in(repo);
        cmd.arg("diff");
        if binary {
            cmd.arg("--binary");
        }
        apply_target(&mut cmd, target);
        capture(cmd, "diff")
    }

    fn changed_paths(&self, repo: &Path, target: DiffTarget) -> Result<Vec<String>, GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["diff", "--name-only"]);
        apply_target(&mut cmd, target);
        let out = capture_string(cmd, "diff")?;
        Ok(out.lines().map(str::to_string).collect())
    }

    fn restore_worktree(&self, worktree: &Path) -> Result<(), GitError> {
        let mut cmd = git_in(worktree);
        cmd.args(["checkout", "-q", "--", "."]);
        capture(cmd, "checkout").map(|_| ())
    }

    fn clean_untracked(&self, worktree: &Path, keep_ignored: bool) -> Result<(), GitError> {
        let mut cmd = git_in(worktree);
        cmd.args(["clean", "-qfd"]);
        if !keep_ignored {
            cmd.arg("-x");
        }
        capture(cmd, "clean").map(|_| ())
    }

    fn add_worktree(&self, repo: &Path, dst: &Path, rev: &str) -> Result<(), GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["worktree", "add", "-q", "--no-checkout", "--detach"])
            .arg(dst)
            .arg(rev);
        capture(cmd, "worktree-add").map(|_| ())
    }

    fn populate_index(&self, worktree: &Path, rev: &str) -> Result<(), GitError> {
        let mut cmd = git_in(worktree);
        cmd.args(["read-tree", rev]);
        capture(cmd, "read-tree").map(|_| ())
    }

    fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["worktree", "remove", "--force"]).arg(worktree);
        capture(cmd, "worktree-remove").map(|_| ())
    }

    fn prune_worktrees(&self, repo: &Path) -> Result<Vec<String>, GitError> {
        let mut cmd = git_in(repo);
        cmd.args(["worktree", "prune", "--verbose"]);
        let out = cmd.output().map_err(|e| GitError {
            op: "worktree-prune",
            message: format!("could not launch git: {e}"),
        })?;
        if !out.status.success() {
            return Err(GitError {
                op: "worktree-prune",
                message: format!(
                    "exited with {}: {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            });
        }
        // "Removing worktrees/<name>: <reason>" lines land on stderr in
        // current git; scan both streams so a future move to stdout keeps
        // working.
        let mut pruned = Vec::new();
        for stream in [&out.stdout, &out.stderr] {
            for line in String::from_utf8_lossy(stream).lines() {
                if let Some(rest) = line.trim().strip_prefix("Removing worktrees/") {
                    if let Some(idx) = rest.find(':') {
                        pruned.push(rest[..idx].to_string());
                    }
                }
            }
        }
        Ok(pruned)
    }

    fn stage_all_and_commit(
        &self,
        worktree: &Path,
        message: &str,
        identity: &CommitIdentity,
    ) -> Result<String, GitError> {
        let mut add = git_in(worktree);
        add.args(["add", "-A"]);
        capture(add, "add")?;

        // `-c` overrides all config levels, so the commit succeeds even where
        // no global identity is configured.
        let mut commit = git_in(worktree);
        commit
            .arg("-c")
            .arg(format!("user.email={}", identity.email))
            .arg("-c")
            .arg(format!("user.name={}", identity.name))
            .args(["commit", "-q", "-m", message]);
        capture(commit, "commit")?;

        self.resolve_revision(worktree, "HEAD")
    }

    fn create_branch(
        &self,
        repo: &Path,
        branch: &str,
        sha: &str,
        force: bool,
    ) -> Result<(), GitError> {
        let mut cmd = git_in(repo);
        cmd.arg("branch");
        if force {
            cmd.arg("--force");
        }
        cmd.arg(branch).arg(sha);
        capture(cmd, "branch").map(|_| ())
    }

    fn merge_check(
        &self,
        repo: &Path,
        base: &str,
        ours: &str,
        theirs: &str,
    ) -> Result<MergeCheck, GitError> {
        // `merge-tree --write-tree` (git >= 2.38; `--merge-base` >= 2.40)
        // merges in memory. Exit 0 = clean, 1 = conflicted — both are valid
        // results, so `capture` (which treats non-zero as failure) is out.
        let mut cmd = git_in(repo);
        cmd.args(["merge-tree", "--write-tree", "--name-only"])
            .arg(format!("--merge-base={base}"))
            .arg(ours)
            .arg(theirs);
        let out = cmd.output().map_err(|e| GitError {
            op: "merge-tree",
            message: format!("could not launch git: {e}"),
        })?;
        let clean = match out.status.code() {
            Some(0) => true,
            Some(1) => false,
            _ => {
                return Err(GitError {
                    op: "merge-tree",
                    message: format!(
                        "exited with {}: {}",
                        out.status,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                })
            }
        };
        // Output: merged tree oid, then (when conflicted) one path per line
        // until a blank line; informational messages follow the blank line.
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut lines = stdout.lines();
        let merged_tree = lines
            .next()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .ok_or_else(|| GitError {
                op: "merge-tree",
                message: "no tree oid in output".into(),
            })?
            .to_string();
        let conflict_paths = if clean {
            Vec::new()
        } else {
            lines
                .take_while(|l| !l.trim().is_empty())
                .map(str::to_string)
                .collect()
        };
        Ok(MergeCheck {
            clean,
            merged_tree,
            conflict_paths,
        })
    }

    fn list_refs(&self, repo: &Path, prefix: &str) -> Result<Vec<RefEntry>, GitError> {
        let mut cmd = git_in(repo);
        cmd.args([
            "for-each-ref",
            "--format=%(refname:short) %(objectname)",
            prefix,
        ]);
        let out = capture_string(cmd, "for-each-ref")?;
        let mut refs = Vec::new();
        for line in out.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(name), Some(sha)) = (parts.next(), parts.next()) {
                refs.push(RefEntry {
                    name: name.to_string(),
                    sha: sha.to_string(),
                });
            }
        }
        Ok(refs)
    }
}

fn apply_target(cmd: &mut Command, target: DiffTarget) {
    match target {
        DiffTarget::WorktreeToHead => {
            cmd.arg("HEAD");
        }
        DiffTarget::CommitRange { base, head } => {
            cmd.arg(format!("{base}..{head}"));
        }
    }
}
