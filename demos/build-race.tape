# LP demo (live): cargo build race between git worktree and ivk.
#
# Background: clonefile-cloned `target/` preserves cargo's fingerprints,
# so cargo finds nothing to do in the ivk workspace and exits in ~0.3 s.
# A git worktree has different absolute paths and cargo invalidates the
# fingerprint cache, doing a full rebuild from scratch (~40 s).
#
# Same source tree. Same `cargo build --release`. Sub-second vs 40-second.

Output demos/build-race.mp4
Output demos/build-race.gif

Set FontSize 16
Set Width 1200
Set Height 320
Set Padding 16
Set Theme "Catppuccin Mocha"
Set Shell "bash"
Set TypingSpeed 25ms

Hide
Type "export TERM=xterm-256color && bash scripts/bench/build/setup-rust.sh >/dev/null 2>&1 && clear"
Enter
Sleep 1s
Show

Type "bash scripts/bench/live-build-race.sh"
Enter
# Cargo build for git worktree takes ~35-45 s. ivk side finishes in <1 s.
# Leave generous headroom so the viewer sees the gap fully form.
Sleep 50s
Sleep 4s
