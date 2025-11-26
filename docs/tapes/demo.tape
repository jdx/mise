# Use mise run docs:demo to record this tape

Output demo.gif
Output demo.mp4

# vhs settings: https://github.com/charmbracelet/vhs#settings
Set Shell "bash"
Set FontSize 24
Set Width 1200
Set Height 600
Set Padding 10
Set WindowBar Colorful
Set TypingSpeed 65ms

#### SETUP
Hide
Type 'apt-get update && apt-get install -y curl gpg && curl https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh' Enter Wait
Type 'mkdir myproj' Enter Wait
Type 'clear'
Enter Wait
Show

#### DEMO START

Sleep 500ms

Type '# Short demo showing how mise helps manage multiple versions of tools like Node.js, Python, Ruby, Go, Terraform, ripgrep, jq, and more.' Enter
Sleep 2s

Enter

Type '# "mise exec <tool> -- <command>" allows you to run any tools with mise'
Sleep 1s Enter Wait

Type "mise exec node@24 -- node -v"
Sleep 500ms Enter Wait Sleep 2s

Type '# node is only available in the mise environment, not globally' Sleep 500ms Enter Wait

Type 'node -v' Sleep 500ms Enter Wait
Sleep 2s

Type 'clear' Sleep 500ms Enter Wait

Type '# Here is another example where we run terraform with `mise exec`'
Sleep 1s Enter

Type "mise exec terraform -- terraform -v"
Sleep 500ms Enter Wait Sleep 3s

Type 'clear' Sleep 500ms Enter Wait

Type '# mise exec is great for running one-off commands, however it can be convenient to activate mise' Sleep 500ms Enter

Type '# When activated, mise will automatically update your PATH to include the tools you have installed, making them available directly' Enter
Sleep 2s Enter

Type 'clear' Sleep 500ms Enter Wait

Type "# Let's activate mise. (this usually goes into your shell config file)" Sleep 500ms Enter

Type 'eval "$(mise activate bash)"' Sleep 500ms Enter Wait

Type '# We will now make node@lts the global default.'
Sleep 2s Enter Wait

Type "mise use --global node@lts" Sleep 500ms Enter Wait
Sleep 2s

Type "node -v" Sleep 500ms Enter Wait
Sleep 1s

Type "which node" Sleep 500ms Enter Wait
Sleep 2s

Type '# Note that we get back the path to the real node here, not a shim'
Enter
Sleep 3s

Type '# we can also install other tools with mise.' Enter
Type '# For example, we will set up terraform, jq, bat and go globally' Enter
Sleep 2s

Type 'clear' Sleep 500ms Enter Wait

Type "mise use -g terraform jq go bat" Sleep 500ms Enter Wait
Sleep 2s

Type "terraform -v" Sleep 500ms Enter Wait
Type "jq --version" Sleep 500ms Enter Wait
Type "go version" Sleep 500ms Enter Wait
Sleep 3s

Type 'mise ls' Sleep 500ms Enter Wait
Sleep 3s

Type 'clear' Sleep 500ms Enter Wait
Type '# Lets enter a project directory where we will set up node@23 and pnpm@10' Enter
Sleep 1s

Type "cd myproj" Sleep 500ms Enter Wait
Sleep 1s

Type "mise use node@23 pnpm@10" Sleep 500ms Enter Wait
Type "node -v" Enter Wait
Sleep 2s

Type "pnpm -v" Enter Wait
Sleep 2s

Type '# As expected, `node -v` is now v23.x' Enter Wait
Sleep 2s

Type 'bat mise.toml' Sleep 500ms Enter Wait
Sleep 2s

Type 'mise ls' Sleep 500ms Enter Wait
Sleep 3s

Type '# We will leave this directory. The node version will revert to the global LTS version' Enter
Sleep 2s

Type "cd .." Sleep 500ms Enter Wait
Type "node -v" Sleep 500ms Enter Wait

Sleep 4s

# cleanup
Hide
Type 'rm -rf myproj' Enter
Wait
