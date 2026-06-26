#!/usr/bin/env bash
# Set up a realistic Vite + React + TS fixture as a committed git repo,
# so the build-artifact spike can clone it via approaches A and G.
#
# Side effects:
#   .cache/fixture-vite/        — git repo containing the project + node_modules + dist
#   .cache/pnpm-store/          — pnpm content-addressed store, shared across all workspaces
#
# Idempotent: skips work if the fixture already exists with a clean working tree.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

FIX="$ROOT/.cache/fixture-vite"
STORE="$ROOT/.cache/pnpm-store"

if [[ -f "$FIX/.git/HEAD" && -d "$FIX/node_modules" && -d "$FIX/dist" ]]; then
  echo "[setup-vite] cache hit: $FIX"
  exit 0
fi

/bin/rm -rf "$FIX"
mkdir -p "$FIX" "$STORE"

# Pin the pnpm store path so it lives inside our .cache/ and is not contaminated
# by the user's global ~/.pnpm-store. Workspaces inherit this via the local
# .npmrc we write into the fixture.
cat > "$FIX/../pnpm-store.path" <<EOF
$STORE
EOF

cd "$FIX"

# Create a minimal-but-realistic React + Vite + TypeScript scaffold by hand.
# We avoid `pnpm create vite` to stay deterministic and offline-resilient.
cat > package.json <<'PKGJSON'
{
  "name": "ivk-bench-vite-fixture",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@types/react": "^18.3.3",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.1",
    "typescript": "^5.5.3",
    "vite": "^5.3.4"
  }
}
PKGJSON

cat > .npmrc <<EOF
store-dir=$STORE
EOF

cat > tsconfig.json <<'TSJSON'
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true
  },
  "include": ["src"]
}
TSJSON

cat > vite.config.ts <<'VITECONFIG'
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    sourcemap: true,
  },
});
VITECONFIG

cat > index.html <<'HTML'
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>ivk bench fixture</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
HTML

mkdir -p src
cat > src/main.tsx <<'MAIN'
import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
MAIN

cat > src/App.tsx <<'APP'
import { useState } from "react";
export default function App() {
  const [count, setCount] = useState(0);
  return (
    <main style={{ fontFamily: "system-ui", padding: 32 }}>
      <h1>ivk bench fixture</h1>
      <p>This is a Vite + React project used for the build-artifact spike.</p>
      <button onClick={() => setCount((c) => c + 1)}>count is {count}</button>
    </main>
  );
}
APP

# Generate a fair number of source modules so the build does non-trivial work.
# Without this, tsc + vite finish in ~200 ms and the build-artifact comparison
# is dominated by node_modules / dist scaffolding instead of real bundling work.
for i in $(/usr/bin/seq 1 80); do
  cat > "src/Comp${i}.tsx" <<EOF
import { useState, useEffect } from "react";

interface Props { initial?: number; }

export default function Comp${i}({ initial = ${i} }: Props) {
  const [n, setN] = useState(initial);
  useEffect(() => {
    const t = setInterval(() => setN((v) => v + 1), 10_000);
    return () => clearInterval(t);
  }, []);
  const items = Array.from({ length: 12 }, (_, idx) => ({
    id: idx,
    label: \`row-\${idx}-of-comp-${i}\`,
    value: n + idx + ${i},
  }));
  return (
    <section data-comp="${i}">
      <h2>Comp${i}</h2>
      <ul>{items.map((it) => <li key={it.id}>{it.label}: {it.value}</li>)}</ul>
    </section>
  );
}
EOF
done

echo "[setup-vite] running pnpm install (cold) ..."
/usr/bin/time -h pnpm install --silent 2>&1 | /usr/bin/tail -5 || true

echo "[setup-vite] running pnpm build ..."
/usr/bin/time -h pnpm build 2>&1 | /usr/bin/tail -5 || true

echo "[setup-vite] sizes (du -sk, naive):"
for d in node_modules dist src; do
  if [[ -d "$d" ]]; then
    sz=$(/usr/bin/du -sk "$d" | awk '{print $1}')
    echo "  $d: $((sz/1024)) MB"
  fi
done

echo "[setup-vite] committing as git repo ..."
git init -q -b main
cat > .gitignore <<'EOF'
# Intentionally NOT ignoring node_modules / dist for the bench fixture —
# we want them committed so approach A (git worktree) materializes them in
# every worktree, mirroring the "naive" disk cost in the wild.
EOF
git -c user.email=bench@ivykernel.dev -c user.name=bench add -A
git -c user.email=bench@ivykernel.dev -c user.name=bench commit -q -m "vite fixture: src + node_modules + dist"

echo "[setup-vite] done. HEAD=$(git rev-parse --short HEAD)"
