//! `ivk new <name>...` and `ivk ws new <name>...` — create one or more workspaces.

use std::path::PathBuf;

use serde::Serialize;

use ivk_core::{
    absolutize, materialize_workspace, BeginCreate, GitBackend, GitCliBackend, MaterializeOptions,
};

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

#[derive(Serialize)]
struct CreatedWorkspace {
    name: String,
    path: String,
    entries_cloned: usize,
    elapsed_ms: u128,
    strategy: &'static str,
}

#[derive(Serialize)]
struct WsNewPayload {
    created: Vec<CreatedWorkspace>,
    failed: Vec<FailedWorkspace>,
    /// Advisory path prefixes recorded for the created workspace (--claim).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    claims: Vec<String>,
    /// Human-readable collisions with other live workspaces' claims.
    /// Advisory: the workspace is still created; the orchestrator decides.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    claim_warnings: Vec<String>,
}

#[derive(Serialize)]
struct FailedWorkspace {
    name: String,
    reason: String,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);

    let mut names: Vec<&str> = Vec::new();
    let mut from: Option<&str> = None;
    let mut claims: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if a == "--from" {
            i += 1;
            match args.get(i) {
                Some(v) if !v.starts_with('-') => from = Some(*v),
                _ => {
                    return fail(
                        "missing_argument",
                        "--from requires a revision (e.g. --from main~2)",
                        "ivk help",
                        json || agent,
                        2,
                    )
                }
            }
        } else if a == "--claim" {
            i += 1;
            match args.get(i) {
                Some(v) if !v.starts_with('-') => claims.push(*v),
                _ => {
                    return fail(
                        "missing_argument",
                        "--claim requires a path prefix (e.g. --claim src/auth)",
                        "ivk help",
                        json || agent,
                        2,
                    )
                }
            }
        } else if !a.starts_with('-') {
            names.push(a);
        }
        i += 1;
    }

    if !claims.is_empty() && names.len() > 1 {
        return fail(
            "conflicting_args",
            "--claim declares one workspace's intent; pass exactly one workspace name",
            "ivk help",
            json || agent,
            2,
        );
    }

    if names.is_empty() {
        let msg = "ws new requires at least one workspace name";
        if json || agent {
            let env: Envelope<()> = Envelope {
                ok: false,
                command: "ws.new",
                next_command: Some("ivk help".into()),
                recommended_next_steps: None,
                error: Some(ErrorBlock {
                    code: "missing_argument",
                    message: msg.into(),
                }),
                data: (),
            };
            print_json(&env);
        } else {
            eprintln!("ivk: {}. Try `ivk new <name>`.", msg);
        }
        return 2;
    }

    let src = match absolutize(&PathBuf::from(".")) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ivk: cannot resolve current directory: {}", e);
            return 1;
        }
    };
    if !src.join(".git").exists() {
        let msg = "current directory does not contain a .git/; must be run from a git repo root";
        if json || agent {
            let env: Envelope<()> = Envelope {
                ok: false,
                command: "ws.new",
                next_command: Some("git init".into()),
                recommended_next_steps: None,
                error: Some(ErrorBlock {
                    code: "not_a_git_repo",
                    message: msg.into(),
                }),
                data: (),
            };
            print_json(&env);
        } else {
            eprintln!("ivk: {}", msg);
        }
        return 1;
    }

    let workspaces_dir = src.join(".ivk").join("workspaces");
    if !workspaces_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&workspaces_dir) {
            eprintln!("ivk: cannot create {}: {}", workspaces_dir.display(), e);
            return 1;
        }
    }

    let git = GitCliBackend::new();
    let registry = crate::reg::open_synced(&src);

    // Resolve the base once: fail fast on a bad --from before any workspace
    // is touched. An unresolvable HEAD (fresh empty repo) keeps the old
    // behavior — each materialize reports the failure per name.
    let base_rev = from.unwrap_or("HEAD");
    let base_snapshot = match git.resolve_revision(&src, base_rev) {
        Ok(sha) => Some(sha),
        Err(_) if from.is_some() => {
            return fail(
                "invalid_revision",
                &format!("cannot resolve revision `{}`", base_rev),
                "git log --oneline -5",
                json || agent,
                2,
            )
        }
        Err(_) => None,
    };

    let mut created: Vec<CreatedWorkspace> = Vec::new();
    let mut failed: Vec<FailedWorkspace> = Vec::new();

    for name in &names {
        let dst = workspaces_dir.join(name);
        // Journal intent first: a SIGKILL mid-materialize leaves a `creating`
        // row that `ivk doctor` reports and `ivk doctor --repair` rolls back.
        let began = registry
            .as_ref()
            .and_then(|r| r.begin_create(name, base_snapshot.as_deref()).ok());
        let opts = MaterializeOptions {
            src: src.clone(),
            dst: dst.clone(),
            with_git: true,
            rev: from.map(String::from),
        };
        match materialize_workspace(&opts) {
            Ok(r) => {
                if let Some(reg) = &registry {
                    let _ = reg.mark_ready(name);
                }
                let rel = dst.strip_prefix(&src).unwrap_or(&dst);
                created.push(CreatedWorkspace {
                    name: (*name).to_owned(),
                    path: format!("./{}", rel.display()),
                    entries_cloned: r.cloned_entries,
                    elapsed_ms: r.total.as_millis(),
                    strategy: r.strategy,
                });
            }
            Err(e) => {
                // Roll back what this run owns: the partial tree (never a
                // pre-existing destination) and the journal row (only if this
                // call inserted it).
                if !matches!(e, ivk_core::Error::DstExists(_)) {
                    let _ = ivk_core::remove_workspace(&git, &src, &dst);
                }
                if began == Some(BeginCreate::Started) {
                    if let Some(reg) = &registry {
                        let _ = reg.delete_workspace_row(name);
                    }
                }
                failed.push(FailedWorkspace {
                    name: (*name).to_owned(),
                    reason: e.to_string(),
                });
            }
        }
    }

    // Record claims for the (single) created workspace and surface
    // collisions with other live workspaces' claims. Purely advisory:
    // creation already succeeded, the warning is a planning fact.
    let mut claim_warnings: Vec<String> = Vec::new();
    if !claims.is_empty() && !created.is_empty() {
        let name = &created[0].name;
        if let Some(reg) = &registry {
            let existing = reg.claims().unwrap_or_default();
            for claim in &claims {
                for other in existing.iter().filter(|e| &e.workspace_name != name) {
                    if ivk_core::prefixes_overlap(claim, &other.path_prefix) {
                        claim_warnings.push(format!(
                            "claim `{}` overlaps `{}` held by workspace `{}`",
                            claim, other.path_prefix, other.workspace_name
                        ));
                    }
                }
                let _ = reg.add_claim(name, claim);
            }
        } else {
            claim_warnings.push("no registry available; claims were not recorded".into());
        }
    }

    let ok = failed.is_empty();
    let next = if ok && created.len() == 1 {
        Some(format!(
            "cd {} && ivk doctor --agent --json",
            created[0].path
        ))
    } else if ok {
        Some("ivk doctor --agent --json".into())
    } else {
        Some("ivk help".into())
    };

    if json || agent {
        let steps = if agent {
            let mut v = if ok {
                vec![
                    if created.len() == 1 {
                        format!("cd {} to work in the new workspace.", created[0].path)
                    } else {
                        format!(
                            "Created {} workspaces under .ivk/workspaces/. cd into one to start.",
                            created.len()
                        )
                    },
                    "Run tests / make edits inside the workspace.".into(),
                    "Run `ivk doctor --agent --json` when unsure of next step.".into(),
                ]
            } else {
                vec![
                    format!(
                        "{} workspace(s) failed to create. See `failed` array.",
                        failed.len()
                    ),
                    "Run `ivk help --agent` for the golden path.".into(),
                ]
            };
            if !claim_warnings.is_empty() {
                v.push(format!(
                    "Claim collision(s): {}. Consider re-scoping this task or serializing it after the holder exports.",
                    claim_warnings.join("; ")
                ));
            }
            Some(v)
        } else {
            None
        };

        let payload = WsNewPayload {
            created,
            failed,
            claims: claims
                .iter()
                .map(|c| c.trim_end_matches('/').to_string())
                .collect(),
            claim_warnings,
        };
        let env = Envelope {
            ok,
            command: "ws.new",
            next_command: next,
            recommended_next_steps: steps,
            error: None,
            data: payload,
        };
        print_json(&env);
    } else {
        for w in &created {
            println!(
                "created workspace {} ({} entries, {}ms, strategy={})",
                w.path, w.entries_cloned, w.elapsed_ms, w.strategy,
            );
        }
        for c in &claims {
            println!("claimed {}", c.trim_end_matches('/'));
        }
        for warn in &claim_warnings {
            eprintln!("ivk: warning: {}", warn);
        }
        for f in &failed {
            eprintln!("ivk: failed to create workspace `{}`: {}", f.name, f.reason);
        }
    }

    if ok {
        0
    } else {
        1
    }
}

fn fail(code: &'static str, msg: &str, next: &str, as_json: bool, exit: i32) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "ws.new",
            next_command: Some(next.into()),
            recommended_next_steps: None,
            error: Some(ErrorBlock {
                code,
                message: msg.into(),
            }),
            data: (),
        };
        print_json(&env);
    } else {
        eprintln!("ivk: {}", msg);
    }
    exit
}
