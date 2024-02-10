set _usage_spec_mise (mise usage | string collect)
complete -xc mise -a '(/Users/jdx/src/usage/target/debug/usage complete-word -s "$_usage_spec_mise" -- (commandline -cop) (commandline -t))'
