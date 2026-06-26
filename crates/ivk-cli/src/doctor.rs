//! `ivk doctor [--agent] [--json]` — the "git status" for ivk.
//!
//! Reports:
//!   - is the cwd inside a git repo? (looks for .git)
//!   - is the cwd inside an ivk workspace? (looks for .git pointer file whose
//!     gitdir points into a workspace admin entry)
//!   - is .ivk/ initialized?
//!   - if inside a workspace, what's its name and git status?
//!
//! Output shape mirrors the convention in `output.rs`. `--agent --json`
//! produces the form the MVP plan §7 specifies.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::output::{print_json, wants_agent, wants_json, Envelope};

#[derive(Serialize, Default)]
struct DoctorReport {
    repo_initialized: bool,
    inside_ivk_workspace: bool,
    ivk_dir_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_status: Option<&'static str>, // "clean" | "dirty" | "unknown"
    has_changes: bool,
    repo_root: String,
    strategy: &'static str,
}

pub fn run(args: &[&str]) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ivk: cannot resolve current directory: {}", e);
            return 1;
        }
    };

    let dot_git = cwd.join(".git");

    // Workspace detection: a `.git` *file* (not directory) with `gitdir: <path>`
    // inside indicates this dir is a worktree. If that path looks like
    // `.../<src>/.git/worktrees/<name>`, we treat <name> as the workspace name.
    let mut inside = false;
    let mut ws_name: Option<String> = None;
    let mut ws_status: Option<&'static str> = None;
    let mut dirty = false;
    if dot_git.is_file() {
        if let Ok(s) = fs::read_to_string(&dot_git) {
            if let Some(line) = s.lines().next() {
                if let Some(rest) = line.strip_prefix("gitdir:") {
                    let admin = PathBuf::from(rest.trim());
                    if let Some(name) = workspace_name_from_admin(&admin) {
                        inside = true;
                        ws_name = Some(name);
                        let (status, d) = git_status_in(&cwd);
                        dirty = d;
                        ws_status = Some(if dirty { "dirty" } else { "clean" });
                        let _ = status; // reserved for future detail
                    }
                }
            }
        }
    }

    let rep = DoctorReport {
        repo_initialized: dot_git.exists(),
        inside_ivk_workspace: inside,
        ivk_dir_present: cwd.join(".ivk").is_dir(),
        workspace_name: ws_name,
        workspace_status: ws_status,
        has_changes: dirty,
        repo_root: cwd.display().to_string(),
        strategy: current_strategy(),
    };

    let json = wants_json(args);
    let agent = wants_agent(args);

    if json || agent {
        let next = next_command_hint(&rep);
        let steps = recommended_steps(&rep);
        let env = Envelope {
            ok: true,
            command: "doctor",
            next_command: next,
            recommended_next_steps: if agent { Some(steps) } else { None },
            error: None,
            data: rep,
        };
        print_json(&env);
        return 0;
    }

    // Human-friendly output.
    println!("ivk doctor");
    println!("  repo_initialized:     {}", rep.repo_initialized);
    println!("  ivk_dir_present:      {}", rep.ivk_dir_present);
    println!("  inside_ivk_workspace: {}", rep.inside_ivk_workspace);
    if let Some(n) = rep.workspace_name {
        println!("  workspace_name:       {}", n);
        println!(
            "  workspace_status:     {}",
            rep.workspace_status.unwrap_or("unknown")
        );
    }
    println!("  strategy:             {}", rep.strategy);
    if !rep.repo_initialized {
        println!("\nNo git repo here. Initialize with `git init` first.");
    } else if !rep.ivk_dir_present && !rep.inside_ivk_workspace {
        println!("\nTip: run `ivk new <task-name>` to create your first workspace.");
    }
    0
}

fn workspace_name_from_admin(admin: &Path) -> Option<String> {
    // Looking for ".../.git/worktrees/<name>"
    let mut comps = admin.components().rev();
    let name = comps.next()?.as_os_str().to_str()?.to_owned();
    let worktrees = comps.next()?.as_os_str();
    let dot_git = comps.next()?.as_os_str();
    if worktrees == std::ffi::OsStr::new("worktrees") && dot_git == std::ffi::OsStr::new(".git") {
        Some(name)
    } else {
        None
    }
}

fn git_status_in(cwd: &Path) -> (String, bool) {
    use std::process::Command;
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["status", "--porcelain"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).into_owned();
            let dirty = !s.trim().is_empty();
            (s, dirty)
        }
        _ => (String::new(), false),
    }
}

#[cfg(target_os = "macos")]
pub fn current_strategy() -> &'static str {
    "apfs-clonefile"
}
#[cfg(target_os = "linux")]
pub fn current_strategy() -> &'static str {
    "linux-reflink-via-cp"
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn current_strategy() -> &'static str {
    "unsupported"
}

fn next_command_hint(r: &DoctorReport) -> Option<String> {
    if !r.repo_initialized {
        return Some("git init".into());
    }
    if r.inside_ivk_workspace {
        if r.has_changes {
            return Some(format!(
                "ivk ch new {} — once tests pass, record this as a changeset",
                r.workspace_name.as_deref().unwrap_or("<this>")
            ));
        }
        return Some(
            "# you are in a clean workspace; make edits then re-run `ivk doctor --agent --json`"
                .into(),
        );
    }
    if !r.ivk_dir_present {
        return Some("ivk new <task-name>".into());
    }
    Some("ivk new <task-name>".into())
}

fn recommended_steps(r: &DoctorReport) -> Vec<String> {
    if !r.repo_initialized {
        return vec!["Initialize a git repo here first: `git init`".into()];
    }
    if r.inside_ivk_workspace {
        let name = r.workspace_name.clone().unwrap_or_else(|| "<this>".into());
        if r.has_changes {
            return vec![
                "Run project tests inside this workspace.".into(),
                format!("If tests pass, record a changeset: `ivk ch new {}`.", name),
                format!(
                    "If the attempt failed, discard the workspace: `ivk ws rm {}`.",
                    name
                ),
            ];
        }
        return vec![
            "You are inside a clean ivk workspace. Make edits, run tests, then re-run doctor."
                .into(),
        ];
    }
    vec![
        "You are not inside an ivk workspace yet.".into(),
        "Create one for the current task: `ivk new <task-name>`.".into(),
        "Then `cd .ivk/workspaces/<task-name>` and re-run `ivk doctor --agent --json`.".into(),
    ]
}
