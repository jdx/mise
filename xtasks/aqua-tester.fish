#!/usr/bin/env fish

cargo b
alias m target/debug/mise
m registry | grep -v '(core|ubi|aqua|vfox):' | awk '{print $1}'> tmp/missing_asdf_tools
rm -f tmp/pairs
for tool in (cat tmp/missing_asdf_tools)
     set -l aqua (cat ../mise-versions/docs/aqua-registry/all | grep "/$tool\$")
     if not test -z "$aqua"
        echo "$tool aqua:$aqua"
        echo "$tool aqua:$aqua" >> tmp/pairs
    end
end
for tool in (cat tmp/pairs | sort -R)
    set -l tool (string split " " $tool)
    set -l cmd "m x $tool[2] -- $tool[1] -v"
    echo $cmd
    set -l output (m x $tool[2] -- $tool[1] -v)
    set -l cmd_status $status
    if test "$cmd_status" = "0"
       echo OK
       rm -f "tmp/$tool[1]"
       echo "AQUA: $tool[2]" > "tmp/$tool[1]"
       echo "COMMAND: $cmd" >> "tmp/$tool[1]"
       echo "OUTPUT: $output" >> "tmp/$tool[1]"
    else
        set -l cmd "m x $tool[2] -- $tool[1] --version"
        echo $cmd
        set -l output (m x $tool[2] -- $tool[1] --version)
        set -l cmd_status $status
        if test "$cmd_status" = "0"
           echo OK
           rm -f "tmp/$tool[1]"
           echo "AQUA: $tool[2]" > "tmp/$tool[1]"
           echo "COMMAND: $cmd" >> "tmp/$tool[1]"
           echo "OUTPUT: $output" >> "tmp/$tool[1]"
        else
            set -l cmd "m x $tool[2] -- $tool[1] version"
            echo $cmd
            set -l output (m x $tool[2] -- $tool[1] version)
            set -l cmd_status $status
            if test "$cmd_status" = "0"
               echo OK
               rm -f "tmp/$tool[1]"
               echo "AQUA: $tool[2]" > "tmp/$tool[1]"
               echo "COMMAND: $cmd" >> "tmp/$tool[1]"
               echo "OUTPUT: $output" >> "tmp/$tool[1]"
            else
                echo FAIL $cmd_status
            end
        end
    end
end
