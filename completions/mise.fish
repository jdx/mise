# if "usage" is not installed show an error
if ! command -v usage &> /dev/null
    echo >&2
    echo "Error: usage CLI not found. This is required for completions to work in mise." >&2
    echo "See https://usage.jdx.dev for more information." >&2
    return 1
end

if ! set -q _usage_spec_mise_2024_12_4
  set -g _usage_spec_mise_2024_12_4 (mise usage | string collect)
end
complete -xc mise -a '(usage complete-word --shell fish -s "$_usage_spec_mise_2024_12_4" -- (commandline -cop) (commandline -t))'
