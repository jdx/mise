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

#### SETUP
Hide
Type 'apt-get install -y curl gpg && curl https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh' Enter Wait
Type 'eval "$(mise activate bash)"' Enter Wait
Type 'mkdir myproj' Enter Wait
Type 'clear'
Enter
Wait
Show

#### DEMO START

Sleep 500ms

Type '# A quick demo of mise for installing tools' Enter
Sleep 2s

Type '# We will start by installing node@lts and make it the global default' Enter Wait
Sleep 2s

Type "mise use --global node@lts" Sleep 500ms Enter Wait
Sleep 2s

Type "node -v" Sleep 500ms Enter Wait
Sleep 1s

Type "which node" Sleep 500ms Enter Wait
Sleep 2s

Type '# Note that we get back the path to the real node here, not a shim' Enter
Sleep 3s

Type '# we can also install other tools with mise.' Enter
Type '# For example, we will install terraform, jq, and go' Enter
Sleep 2s

Type 'clear' Sleep 500ms Enter Wait

Type "mise use -g terraform jq go" Sleep 500ms Enter Wait
Sleep 2s

Type "terraform -v" Sleep 500ms Enter Wait
Type "jq --version" Sleep 500ms Enter Wait
Type "go version" Sleep 500ms Enter Wait
Sleep 3s

Type 'mise ls' Sleep 500ms Enter Wait
Sleep 3s

Type 'clear' Sleep 500ms Enter Wait
Type '# Lets enter a project directory where we will set up node@21' Enter
Sleep 1s

Type "cd myproj" Sleep 500ms Enter Wait
Sleep 1s

Type "mise use node@21" Sleep 500ms Enter Wait
Type "node -v" Enter Wait
Sleep 2s

Type '# As expected, `node -v` is now v21.x' Enter Wait
Sleep 2s

Type 'cat mise.toml' Sleep 500ms Enter Wait
Sleep 2s

Type '# We will leave this directory. The node version will revert to the global LTS version' Enter
Sleep 2s

Type "cd .." Sleep 500ms Enter Wait
Type "node -v" Sleep 500ms Enter Wait
Sleep 4s

# cleanup
Hide
Type 'rm -rf myproj' Enter
Wait
