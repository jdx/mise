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
for tool in (cat tmp/pairs | gsort -R)
    set -l tool (string split " " $tool)
    if test -f "tmp/$tool[1]"
      continue
    end
    if test "$tool[1]" = "borg"
      continue
    end
    if test "$tool[1]" = "jiq"
      continue
    end
    if test "$tool[1]" = "eza"
      continue
    end
    if test "$tool[1]" = "gitsign"
      continue
    end
    if test "$tool[1]" = "terraform-lsp"
      continue
    end
    if test "$tool[1]" = "istioctl"
      continue
    end
    if test "$tool[1]" = "om"
      continue
    end
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
            if test "$tool[1]" = "odo"
              continue
            end
            if test "$tool[1]" = "iamlive"
              continue
            end
            if test "$tool[1]" = "tridentctl"
              continue
            end
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
