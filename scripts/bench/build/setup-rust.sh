#!/usr/bin/env bash
# Set up a realistic Rust fixture as a committed git repo.
#
# A small CLI binary with a few real dependencies. We commit Cargo.lock and
# the populated target/ directory so approaches A and G both materialize
# them when cloning, mirroring "naive duplicate" vs "block-shared".
#
# Side effects:
#   .cache/fixture-rust/        — committed git repo, includes target/
#   .cache/cargo-target-shared/ — shared CARGO_TARGET_DIR for the R2 scenario
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

FIX="$ROOT/.cache/fixture-rust"

if [[ -f "$FIX/.git/HEAD" && -d "$FIX/target" ]]; then
  echo "[setup-rust] cache hit: $FIX"
  exit 0
fi

/bin/rm -rf "$FIX"
mkdir -p "$FIX"
cd "$FIX"

cat > Cargo.toml <<'CARGOTOML'
[package]
name = "ivk-bench-rust-fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "fs", "io-util"] }

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
CARGOTOML

mkdir -p src
cat > src/main.rs <<'RS'
use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
struct Args {
    /// Path to read.
    path: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Record { id: u64, name: String, count: u64 }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let body = tokio::fs::read_to_string(&args.path).await?;
    let parsed: Vec<Record> = serde_json::from_str(&body).unwrap_or_default();
    println!("read {} records from {}", parsed.len(), args.path);
    Ok(())
}
RS

# A handful of additional crates to make compile time more representative.
for i in $(/usr/bin/seq 1 10); do
  cat > "src/module${i}.rs" <<EOF
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Item${i} {
    pub id: u64,
    pub name: String,
    pub value: f64,
}

pub fn process_${i}(items: Vec<Item${i}>) -> Vec<Item${i}> {
    items.into_iter().map(|mut x| { x.value *= 1.1 + ${i} as f64; x }).collect()
}
EOF
  echo "pub mod module${i};" >> src/main.rs
done

echo "[setup-rust] running cargo build --release ..."
/usr/bin/time -h cargo build --release 2>&1 | /usr/bin/tail -3 || true

echo "[setup-rust] sizes:"
for d in target src; do
  if [[ -d "$d" ]]; then
    sz=$(/usr/bin/du -sk "$d" | awk '{print $1}')
    echo "  $d: $((sz/1024)) MB"
  fi
done

echo "[setup-rust] committing as git repo ..."
git init -q -b main
cat > .gitignore <<'EOF'
# Intentionally NOT ignoring target/ for the bench fixture.
EOF
git -c user.email=bench@ivykernel.dev -c user.name=bench add -A
git -c user.email=bench@ivykernel.dev -c user.name=bench commit -q -m "rust fixture: src + Cargo.lock + target"

echo "[setup-rust] done. HEAD=$(git rev-parse --short HEAD)"
