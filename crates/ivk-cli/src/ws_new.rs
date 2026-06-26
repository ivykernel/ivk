//! `ivk new <name>...` and `ivk ws new <name>...` — create one or more workspaces.

use std::path::PathBuf;

use serde::Serialize;

use ivk_core::{absolutize, materialize_workspace, MaterializeOptions};

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
}

#[derive(Serialize)]
struct FailedWorkspace {
    name: String,
    reason: String,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let names: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();

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

    let mut created: Vec<CreatedWorkspace> = Vec::new();
    let mut failed: Vec<FailedWorkspace> = Vec::new();

    for name in &names {
        let dst = workspaces_dir.join(name);
        let opts = MaterializeOptions {
            src: src.clone(),
            dst: dst.clone(),
            with_git: true,
        };
        match materialize_workspace(&opts) {
            Ok(r) => {
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
                failed.push(FailedWorkspace {
                    name: (*name).to_owned(),
                    reason: e.to_string(),
                });
            }
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
            Some(if ok {
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
            })
        } else {
            None
        };

        let payload = WsNewPayload { created, failed };
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
