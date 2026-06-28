# LP demo (live): two disk-usage bars that grow as workspaces are created,
# one per approach. Inspired by htop's live process bars.
#
# Rendered with vhs (https://github.com/charmbracelet/vhs):
#   brew install vhs
#   vhs demos/disk-race.tape
#
# Outputs: demos/disk-race.{gif,mp4}
#
# The script behind it (scripts/bench/live-race.sh) alternates: create one
# git worktree, then one ivk workspace, then redraw the two bars. Watching
# the red bar take off while the blue bar stays nearly flat is the entire
# pitch in one screen.

Output demos/disk-race.mp4
Output demos/disk-race.gif

Set FontSize 16
Set Width 1200
Set Height 360
Set Padding 16
Set Theme "Catppuccin Mocha"
Set Shell "bash"
Set TypingSpeed 25ms

Hide
Type "export TERM=xterm-256color && bash scripts/bench/gen-repo.sh M >/dev/null 2>&1 && clear"
Enter
Sleep 500ms
Show

Type "bash scripts/bench/live-race.sh 20"
Enter
# live-race.sh creates 40 workspaces (20 pairs); each pair ~1.4s.
# Leave generous headroom for the bars to fully animate before fade-out.
Sleep 35s
Sleep 3s
