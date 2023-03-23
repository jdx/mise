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

Output src/rtx/docs/demo.gif
#Output src/rtx/docs/demo.webm

Require echo

Set Shell "bash"
Set FontFamily "Berkeley Mono"
Set FontSize 24
Set Width 1200
Set Height 600
Set Padding 10

Type "http rtx.pub/rtx-latest-macos-arm64 > ~/bin/rtx" Enter Sleep 2s
Type "chmod +x ~/bin/rtx" Sleep 1s Enter Sleep 1s
Type "rtx -v" Sleep 1s Enter Sleep 1s
Type 'eval "$(rtx activate bash)"' Sleep 1s Enter Sleep 2s
Type "rtx install nodejs@lts" Sleep 2s Enter Sleep 8s
Type "rtx global nodejs@lts" Sleep 2s Enter Sleep 3s
Type "node -v" Sleep 1s Enter Sleep 3s
Type "which node" Sleep 1s Enter Sleep 5s
Type "cd ~/myproj" Sleep 1s Enter Sleep 1s
Type "rtx install nodejs@19" Sleep 2s Enter Sleep 8s
Type "rtx local nodejs@19" Sleep 2s Enter Sleep 2s
Type "node -v" Sleep 1s Enter Sleep 3s
Type "cd .." Sleep 1s Enter Sleep 2s
Type "node -v" Sleep 1s Enter Sleep 3s

Sleep 5s
