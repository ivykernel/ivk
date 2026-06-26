//! clonewt — bench-harness binary that materializes one workspace via ivk-core.

use std::path::Path;
use std::process::exit;

use ivk_core::{absolutize, materialize_workspace, MaterializeOptions};

fn main() {
    let raw: Vec<String> = std::env::args().collect();
    let mut verbose = false;
    let mut positionals: Vec<&str> = Vec::new();
    for a in &raw[1..] {
        match a.as_str() {
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                eprintln!("usage: clonewt [-v] <src_repo> <dst_workspace>");
                exit(0);
            }
            other => positionals.push(other),
        }
    }
    if positionals.len() != 2 {
        eprintln!("usage: clonewt [-v] <src_repo> <dst_workspace>");
        exit(2);
    }

    let src = absolutize(Path::new(positionals[0])).unwrap_or_else(|e| {
        eprintln!("clonewt: {}", e);
        exit(1);
    });
    let dst = absolutize(Path::new(positionals[1])).unwrap_or_else(|e| {
        eprintln!("clonewt: {}", e);
        exit(1);
    });

    let opts = MaterializeOptions {
        src,
        dst,
        with_git: true,
    };
    match materialize_workspace(&opts) {
        Ok(r) => {
            if verbose {
                if let Some(d) = r.git_worktree_add {
                    eprintln!("  git worktree add: {:?}", d);
                }
                eprintln!(
                    "  clone working tree: {} entries cloned, {} skipped, {:?}",
                    r.cloned_entries, r.skipped_entries, r.clone_tree
                );
                if let Some(d) = r.git_read_tree {
                    eprintln!("  read-tree HEAD: {:?}", d);
                }
                eprintln!("clonewt[{}]: total {:?}", r.strategy, r.total);
            }
        }
        Err(e) => {
            eprintln!("clonewt: {}", e);
            exit(1);
        }
    }
}
