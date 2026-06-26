# LP demo: git worktree vs ivk on disk consumption.
#
# Rendered with vhs (https://github.com/charmbracelet/vhs):
#   brew install vhs
#   vhs demos/disk-comparison.tape
#
# Output: demos/disk-comparison.mp4 + demos/disk-comparison.gif
#
# What this demonstrates:
#   1. Both workflows create 50 isolated workspaces from the same base repo.
#   2. df is sampled before/after to show real disk consumption.
#   3. git worktree fills ~10 GB. ivk fills ~50 MB. Same end state.
#
# Why 50 workspaces and not 100: keeps the demo under 60 seconds while still
# making the divergence obvious. The CSV chart (demos/chart.svg) covers N=100.

Output demos/disk-comparison.mp4
Output demos/disk-comparison.gif

Set FontSize 18
Set Width 1400
Set Height 800
Set Padding 20
Set Theme "Catppuccin Mocha"
Set Shell "bash"
Set TypingSpeed 30ms

# Setup: ensure repo-M is generated and we're in a clean state.
# The recording must be invoked from the ivk repo root:
#   cd <ivk-repo-root> && vhs demos/disk-comparison.tape
Hide
Type "bash scripts/bench/gen-repo.sh M >/dev/null 2>&1 && clear"
Enter
Sleep 500ms
Show

Type "# Goal: create 50 isolated workspaces of a 200 MB repo."
Sleep 800ms
Enter
Type "# Two ways to do it. Watch the disk."
Sleep 1s
Enter
Enter

# --- Way 1: git worktree ---
Type "echo '--- 50 worktrees the standard way (git worktree) ---'"
Enter
Sleep 600ms

Type "BEFORE=$(df -k . | awk 'NR==2 {print $4}')"
Enter
Sleep 300ms
Type "echo \"disk free before: $((BEFORE/1024/1024)) GB\""
Enter
Sleep 600ms

Type "time (for i in $(seq 1 50); do git -C .cache/repo-M worktree add -q --detach .cache/demo-wt/ws-$i HEAD; done)"
Enter
Sleep 200ms
# Wait for the actual command to complete (~25 s on M)
Sleep 25s

Type "AFTER=$(df -k . | awk 'NR==2 {print $4}')"
Enter
Sleep 300ms
Type "echo \"disk consumed: $(( (BEFORE - AFTER) / 1024 )) MB\""
Enter
Sleep 1500ms

# Cleanup before next round
Hide
Type "for i in $(seq 1 50); do git -C .cache/repo-M worktree remove --force .cache/demo-wt/ws-$i 2>/dev/null; done; rm -rf .cache/demo-wt; git -C .cache/repo-M worktree prune"
Enter
Sleep 8s
Show

Enter

# --- Way 2: ivk (approach F via current shell bench) ---
Type "echo '--- 50 workspaces the ivk way (clonefile + worktree pointer) ---'"
Enter
Sleep 600ms

Type "BEFORE=$(df -k . | awk 'NR==2 {print $4}')"
Enter
Sleep 300ms
Type "echo \"disk free before: $((BEFORE/1024/1024)) GB\""
Enter
Sleep 600ms

Type "time (for i in $(seq 1 50); do bash scripts/bench/create-one.sh F .cache/repo-M .cache/demo-ivk/ws-$i; done)"
Enter
Sleep 200ms
# Wait for the actual command to complete (~120 s on M with current shell impl)
Sleep 130s

Type "AFTER=$(df -k . | awk 'NR==2 {print $4}')"
Enter
Sleep 300ms
Type "echo \"disk consumed: $(( (BEFORE - AFTER) / 1024 )) MB\""
Enter
Sleep 2s

# Cleanup
Hide
Type "for i in $(seq 1 50); do git -C .cache/repo-M worktree remove --force .cache/demo-ivk/ws-$i 2>/dev/null; done; rm -rf .cache/demo-ivk; git -C .cache/repo-M worktree prune"
Enter
Sleep 8s
Show

Enter
Type "# Same 50 workspaces. ~200x less disk."
Sleep 3s
Enter
