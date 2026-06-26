# LP demo: git worktree vs ivk on disk consumption.
#
# Rendered with vhs (https://github.com/charmbracelet/vhs):
#   brew install vhs
#   vhs demos/disk-comparison.tape
#
# Outputs: demos/disk-comparison.mp4 + demos/disk-comparison.gif
#
# What this demonstrates:
#   N = 20 isolated workspaces of a 200 MB repo.
#   Left half: created via `git worktree`. Disk usage measured with `du -sh`.
#   Right half: created via `ivk new`. Same measurement.
#   The disk-usage line at the end of each run is the punch line.
#
# Why 20 and not 100: keeps the recording under 90 seconds while still showing
# the magnitude gap clearly. The full bench at N=100 lives in results/summary.md.

Output demos/disk-comparison.mp4
Output demos/disk-comparison.gif

Set FontSize 18
Set Width 1400
Set Height 720
Set Padding 20
Set Theme "Catppuccin Mocha"
Set Shell "bash"
Set TypingSpeed 30ms

# Setup. The recording must be invoked from the ivk repo root:
#   cd <ivk-repo-root> && vhs demos/disk-comparison.tape
Hide
Type "bash scripts/bench/gen-repo.sh M >/dev/null 2>&1 && clear"
Enter
Sleep 500ms
Show

Type "# Goal: 20 isolated workspaces of a 200 MB repo. Two ways."
Sleep 1s
Enter
Enter

# --- Way 1: git worktree ---
Type "echo '--- the standard way: 20 git worktrees ---'"
Enter
Sleep 800ms

Type "time bash -c 'for i in $(seq 1 20); do git -C .cache/repo-M worktree add -q --detach .cache/demo-wt/ws-$i HEAD; done'"
Enter
Sleep 200ms
Sleep 12s

Type "du -sh .cache/demo-wt"
Enter
Sleep 500ms
Sleep 2s

# Cleanup before next round
Hide
Type "for i in $(seq 1 20); do git -C .cache/repo-M worktree remove --force .cache/demo-wt/ws-$i 2>/dev/null; done; rm -rf .cache/demo-wt; git -C .cache/repo-M worktree prune"
Enter
Sleep 5s
Show

Enter

# --- Way 2: ivk (Rust clonefile binary) ---
Type "echo '--- ivk: 20 workspaces via clonefile + git worktree pointer ---'"
Enter
Sleep 800ms

Type "time bash -c 'for i in $(seq 1 20); do bash scripts/bench/create-one.sh G .cache/repo-M .cache/demo-ivk/ws-$i; done'"
Enter
Sleep 200ms
Sleep 8s

Type "du -sh .cache/demo-ivk"
Enter
Sleep 500ms
Sleep 2s

# Cleanup
Hide
Type "for i in $(seq 1 20); do git -C .cache/repo-M worktree remove --force .cache/demo-ivk/ws-$i 2>/dev/null; done; rm -rf .cache/demo-ivk; git -C .cache/repo-M worktree prune"
Enter
Sleep 5s
Show

Enter
Type "# Same 20 workspaces. ivk uses orders of magnitude less disk."
Sleep 3s
Enter
