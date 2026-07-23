//! `ivk status [--json] [--agent]` — repo-wide summary across all workspaces.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;

use ivk_core::{GitBackend, GitCliBackend};

use crate::output::{print_json, wants_agent, wants_json, Envelope};

#[derive(Serialize)]
struct WorkspaceSummary {
    name: String,
    status: &'static str, // "clean" | "dirty" | "unknown"
    has_changes: bool,
}

/// A path touched by more than one in-flight line of work — a predicted
/// merge conflict. `parties` are workspace names; a workspace contributes
/// its dirty paths and the touched paths of its unexported changesets.
#[derive(Serialize)]
struct Overlap {
    path: String,
    parties: Vec<String>,
}

/// In-flight work crossing a path prefix another workspace claimed with
/// `ivk new --claim`. Advisory, like the claim itself.
#[derive(Serialize)]
struct ClaimViolation {
    path: String,
    /// Workspace doing the touching.
    toucher: String,
    /// Workspace holding the claim.
    claimant: String,
    claimed_prefix: String,
}

#[derive(Serialize)]
struct StatusPayload {
    repo_root: String,
    ivk_dir_present: bool,
    workspace_count: usize,
    workspaces: Vec<WorkspaceSummary>,
    /// Paths touched by 2+ in-flight workspaces (dirty edits or unexported
    /// changesets). The cheapest conflict signal there is: read it *before*
    /// assigning new work, not after merges start failing.
    overlap_count: usize,
    overlaps: Vec<Overlap>,
    /// In-flight paths crossing another workspace's advisory claim.
    claim_violation_count: usize,
    claim_violations: Vec<ClaimViolation>,
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

    // path -> set of workspaces whose in-flight work touches it.
    let mut touched_by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

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
                if dirty {
                    if let Ok(s) = GitCliBackend::new().status(&p) {
                        for path in s.touched_paths() {
                            touched_by.entry(path).or_default().insert(name.clone());
                        }
                    }
                }
                workspaces.push(WorkspaceSummary {
                    name,
                    status,
                    has_changes: dirty,
                });
            }
        }
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
    }

    // Recorded-but-unexported changesets are in-flight work too, attributed
    // to their workspace (which may already be clean, or even removed).
    let registry = crate::reg::open_synced_if_present(&cwd);
    if let Some(reg) = &registry {
        if let Ok(changesets) = reg.changesets() {
            for c in changesets.iter().filter(|c| c.exported_branch.is_none()) {
                for path in &c.touched_paths {
                    touched_by
                        .entry(path.clone())
                        .or_default()
                        .insert(c.workspace_name.clone());
                }
            }
        }
    }

    // In-flight paths crossing another workspace's advisory claim.
    let mut claim_violations: Vec<ClaimViolation> = Vec::new();
    if let Some(reg) = &registry {
        if let Ok(claims) = reg.claims() {
            for (path, owners) in &touched_by {
                for claim in &claims {
                    if !ivk_core::path_under_prefix(path, &claim.path_prefix) {
                        continue;
                    }
                    for owner in owners.iter().filter(|o| **o != claim.workspace_name) {
                        claim_violations.push(ClaimViolation {
                            path: path.clone(),
                            toucher: owner.clone(),
                            claimant: claim.workspace_name.clone(),
                            claimed_prefix: claim.path_prefix.clone(),
                        });
                    }
                }
            }
        }
    }

    let overlaps: Vec<Overlap> = touched_by
        .into_iter()
        .filter(|(_, parties)| parties.len() >= 2)
        .map(|(path, parties)| Overlap {
            path,
            parties: parties.into_iter().collect(),
        })
        .collect();

    let payload = StatusPayload {
        repo_root: cwd.display().to_string(),
        ivk_dir_present,
        workspace_count: workspaces.len(),
        workspaces,
        overlap_count: overlaps.len(),
        overlaps,
        claim_violation_count: claim_violations.len(),
        claim_violations,
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
                let mut v = if dirty.is_empty() {
                    vec!["All workspaces clean. Pick one and `cd .ivk/workspaces/<name>` to work in it.".into()]
                } else {
                    vec![
                        format!("{} workspace(s) have uncommitted changes.", dirty.len()),
                        "For each, decide: record a changeset (`ivk ch new <name>`) or discard (`ivk ws rm <name>`).".into(),
                    ]
                };
                if !payload.overlaps.is_empty() {
                    let preview: Vec<String> = payload
                        .overlaps
                        .iter()
                        .take(5)
                        .map(|o| format!("{} ({})", o.path, o.parties.join(", ")))
                        .collect();
                    v.push(format!(
                        "Predicted conflicts — {} path(s) touched by multiple in-flight workspaces: {}{}. First to export wins; serialize or re-scope the rest before they grow.",
                        payload.overlap_count,
                        preview.join("; "),
                        if payload.overlap_count > 5 { "; ..." } else { "" }
                    ));
                }
                if !payload.claim_violations.is_empty() {
                    let preview: Vec<String> = payload
                        .claim_violations
                        .iter()
                        .take(5)
                        .map(|c| {
                            format!(
                                "{} touches {} (claimed as `{}` by {})",
                                c.toucher, c.path, c.claimed_prefix, c.claimant
                            )
                        })
                        .collect();
                    v.push(format!(
                        "Claim violation(s) — {}: {}{}. Advisory: coordinate with the claimant or re-scope before conflicts materialize.",
                        payload.claim_violation_count,
                        preview.join("; "),
                        if payload.claim_violation_count > 5 { "; ..." } else { "" }
                    ));
                }
                v
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
        if !payload.overlaps.is_empty() {
            println!(
                "{} predicted conflict path(s) (touched by multiple in-flight workspaces):",
                payload.overlap_count
            );
            for o in &payload.overlaps {
                println!("  {}: {}", o.path, o.parties.join(", "));
            }
        }
        if !payload.claim_violations.is_empty() {
            println!("{} claim violation(s):", payload.claim_violation_count);
            for c in &payload.claim_violations {
                println!(
                    "  {} touches {} — claimed as `{}` by {}",
                    c.toucher, c.path, c.claimed_prefix, c.claimant
                );
            }
        }
    }
    0
}

fn workspace_status(ws_path: &Path) -> (&'static str, bool) {
    match GitCliBackend::new().status(ws_path) {
        Ok(s) if s.is_dirty() => ("dirty", true),
        Ok(_) => ("clean", false),
        Err(_) => ("unknown", false),
    }
}
