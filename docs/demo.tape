# VHS documentation
#
# Output:
#   Output <path>.gif               Create a GIF output at the given <path>
#   Output <path>.mp4               Create an MP4 output at the given <path>
#   Output <path>.webm              Create a WebM output at the given <path>
#
# Require:
#   Require <string>                Ensure a program is on the $PATH to proceed
#
# Settings:
#   Set FontSize <number>           Set the font size of the terminal
#   Set FontFamily <string>         Set the font family of the terminal
#   Set Height <number>             Set the height of the terminal
#   Set Width <number>              Set the width of the terminal
#   Set LetterSpacing <float>       Set the font letter spacing (tracking)
#   Set LineHeight <float>          Set the font line height
#   Set LoopOffset <float>%         Set the starting frame offset for the GIF loop
#   Set Theme <json|string>         Set the theme of the terminal
#   Set Padding <number>            Set the padding of the terminal
#   Set Framerate <number>          Set the framerate of the recording
#   Set PlaybackSpeed <float>       Set the playback speed of the recording
#
# Sleep:
#   Sleep <time>                    Sleep for a set amount of <time> in seconds
#
# Type:
#   Type[@<time>] "<characters>"    Type <characters> into the terminal with a
#                                   <time> delay between each character
#
# Keys:
#   Backspace[@<time>] [number]     Press the Backspace key
#   Down[@<time>] [number]          Press the Down key
#   Enter[@<time>] [number]         Press the Enter key
#   Space[@<time>] [number]         Press the Space key
#   Tab[@<time>] [number]           Press the Tab key
#   Left[@<time>] [number]          Press the Left Arrow key
#   Right[@<time>] [number]         Press the Right Arrow key
#   Up[@<time>] [number]            Press the Up Arrow key
#   Down[@<time>] [number]          Press the Down Arrow key
#   PageUp[@<time>] [number]        Press the Page Up key
#   PageDown[@<time>] [number]      Press the Page Down key
#   Ctrl+<key>                      Press the Control key + <key> (e.g. Ctrl+C)
#
# Display:
#   Hide                            Hide the subsequent commands from the output
#   Show                            Show the subsequent commands in the output

Output src/mise/docs/demo.gif
#Output src/mise/docs/demo.webm

Require echo

Set Shell "zsh"
Set FontFamily "Berkeley Mono"
Set FontSize 24
Set Width 1200
Set Height 600
Set Padding 10
Set WindowBar Colorful
#Set Theme "Catppuccin Macchiato"

Sleep 50ms
Type 'echo "First, we will download/setup mise"' Enter Sleep 2s
Type "http https://mise.jdx.dev/mise-latest-macos-arm64 > ~/bin/mise" Enter Sleep 2s
Type "chmod +x ~/bin/mise" Sleep 1s Enter Sleep 1s
Type "mise -v" Sleep 1s Enter Sleep 1s
Type 'eval "$(mise activate zsh)"' Sleep 1s Enter Sleep 2s
Type 'echo "Ok, mise is ready to go now"' Enter Sleep 2s

Type 'echo "Now, we will install node@lts and make it the global default"' Enter Sleep 2s
Type "mise use --global node@lts" Sleep 2s Enter Sleep 3s
Type "node -v" Sleep 1s Enter Sleep 3s
Type "which node" Sleep 1s Enter Sleep 5s
Type 'echo "Note that we get back the path to the real node here, not a shim"' Enter Sleep 2s

Type 'echo "Lets enter a project directory we will setup to use node@21"' Enter Sleep 2s
Type "cd ~/myproj" Sleep 1s Enter Sleep 1s
Type "mise use node@21" Sleep 2s Enter Sleep 2s
Type "node -v" Sleep 1s Enter Sleep 3s
Type 'echo "As expected, now `node -v` is v21.x"' Enter Sleep 2s

Type 'echo "We will leave this directory and the node version will revert to the global LTS version"' Enter Sleep 2s
Type "cd .." Sleep 1s Enter Sleep 2s
Type "node -v" Sleep 1s Enter Sleep 3s

Sleep 5s
