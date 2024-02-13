# if "usage" is not installed show an error
if ! command -v usage &> /dev/null
    echo "Error: usage not found. This is required for completions to work in mise. https://usage.jdx.dev" >&2
    return 1
end

set _usage_spec_mise (mise usage | string collect)
complete -xc mise -a '(usage complete-word -s "$_usage_spec_mise" -- (commandline -cop) (commandline -t))'
