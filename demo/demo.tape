# Records assets/demo.gif for the README. Run from the repository root: vhs demo/demo.tape
Output assets/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1200
Set Height 640
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms
Set WindowBar Colorful

Hide
Type "cd demo && export PS1='$ ' && cargo test -q >/dev/null 2>&1 && cargo llvm-cov --no-report -q --features coverage >/dev/null 2>&1 && clear"
Enter
Sleep 8s
Show

Type "# A passing proptest says only 'ok':"
Enter
Type "cargo test"
Enter
Sleep 3s

Type "clear"
Enter
Type "# With this crate: how far 256 passing test cases go"
Enter
Type "RESIDUAL_RISK=1 cargo test -- --nocapture"
Enter
Sleep 5s

Type "clear"
Enter
Type "# Add coverage and the chance of new code"
Enter
Type "RESIDUAL_RISK=1 cargo llvm-cov --no-report --features coverage -- --nocapture"
Enter
Sleep 8s
Screenshot assets/demo.png
Sleep 1s
