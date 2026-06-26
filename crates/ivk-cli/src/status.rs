//! `ivk status [--json] [--agent]` — repo-wide summary across all workspaces.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::output::{print_json, wants_agent, wants_json, Envelope};

#[derive(Serialize)]
struct WorkspaceSummary {
    name: String,
    status: &'static str, // "clean" | "dirty" | "unknown"
    has_changes: bool,
}

#[derive(Serialize)]
struct StatusPayload {
    repo_root: String,
    ivk_dir_present: bool,
    workspace_count: usize,
    workspaces: Vec<WorkspaceSummary>,
    strategy: &'static str,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ivk status: cannot resolve current directory: {}", e);
            return 1;
        }
    };

    let workspaces_dir = cwd.join(".ivk").join("workspaces");
    let ivk_dir_present = workspaces_dir.parent().map(|p| p.is_dir()).unwrap_or(false);

    let mut workspaces: Vec<WorkspaceSummary> = Vec::new();
    if workspaces_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&workspaces_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let (status, dirty) = workspace_status(&p);
                workspaces.push(WorkspaceSummary {
                    name,
                    status,
                    has_changes: dirty,
                });
            }
        }
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
    }

    let payload = StatusPayload {
        repo_root: cwd.display().to_string(),
        ivk_dir_present,
        workspace_count: workspaces.len(),
        workspaces,
        strategy: super::doctor::current_strategy(),
    };

    if json || agent {
        let next = if !ivk_dir_present {
            Some("ivk init".into())
        } else if payload.workspace_count == 0 {
            Some("ivk new <task-name>".into())
        } else {
            Some("ivk doctor --agent --json".into())
        };
        let steps = if agent {
            Some(if !ivk_dir_present {
                vec!["Run `ivk init` first to set up the .ivk/ skeleton.".into()]
            } else if payload.workspace_count == 0 {
                vec!["No workspaces yet. Create one with `ivk new <task-name>`.".into()]
            } else {
                let dirty: Vec<_> = payload
                    .workspaces
                    .iter()
                    .filter(|w| w.has_changes)
                    .collect();
                if dirty.is_empty() {
                    vec!["All workspaces clean. Pick one and `cd .ivk/workspaces/<name>` to work in it.".into()]
                } else {
                    vec![
                        format!("{} workspace(s) have uncommitted changes.", dirty.len()),
                        "For each, decide: record a changeset (`ivk ch new <name>`) or discard (`ivk ws rm <name>`).".into(),
                    ]
                }
            })
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "status",
            next_command: next,
            recommended_next_steps: steps,
            error: None,
            data: payload,
        };
        print_json(&env);
    } else {
        if !ivk_dir_present {
            println!("ivk status: .ivk/ not present. Run `ivk init` first.");
            return 0;
        }
        if payload.workspaces.is_empty() {
            println!("ivk status: 0 workspaces. Create one with `ivk new <task-name>`.");
            return 0;
        }
        println!("ivk status: {} workspace(s)", payload.workspaces.len());
        for w in &payload.workspaces {
            println!("  {:<24} {}", w.name, w.status);
        }
    }
    0
}

fn workspace_status(ws_path: &PathBuf) -> (&'static str, bool) {
    use std::process::Command;
    let out = Command::new("git")
        .arg("-C")
        .arg(ws_path)
        .args(["status", "--porcelain"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let dirty = !s.trim().is_empty();
            (if dirty { "dirty" } else { "clean" }, dirty)
        }
        _ => ("unknown", false),
    }
}
