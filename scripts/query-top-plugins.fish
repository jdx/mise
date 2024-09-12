#!/usr/bin/env fish

# Print out list of asdf plugins sorted by number of stars. Full list is
# persisted to stargazer_count.txt

if test -n (type -t gh)
else
    echo "GitHub CLI Missing! Aborting!"
    exit 1
end


if test -d /tmp/asdf-plugins
    set current_dir (pwd)
    cd /tmp/asdf-plugins
    git pull /tmp/asdf-plugins
    cd $current_dir
else
    git clone --depth=1 git@github.com:mise-plugins/registry.git /tmp/asdf-plugins
end

if test -e stargazer_count.txt
    rm -rf stargazer_count.txt
end


for i in /tmp/asdf-plugins/plugins/*
    cat $i | \
        string split -f2 '=' | \
        string trim | \
        xargs -I '{}' gh repo view '{}' --json stargazerCount,name,owner | \
        jq -r "[.stargazerCount,\"$(basename $i)\",\"$(string split '=' -f2 (cat $i))\"]|@tsv" >> stargazer_count.txt &
end

echo "$(cat stargazer_count.txt)" | sort -nr | uniq >stargazer_count.txt
head -n 50 stargazer_count.txt
