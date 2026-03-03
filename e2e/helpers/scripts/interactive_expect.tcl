#!/usr/bin/env expect -f
set timeout 40

if {[llength $argv] < 2} {
    puts stderr "usage: interactive_expect.tcl <mode> <cmd> ?pattern? ?payload?"
    exit 2
}

set mode [lindex $argv 0]
set cmd [lindex $argv 1]
set pattern [lindex $argv 2]
set payload [lindex $argv 3]

spawn bash -lc $cmd

if {$mode == "with-input"} {
    expect -re $pattern
    send -- "$payload\r"
    expect eof
} elseif {$mode == "no-prompt"} {
    expect {
        -re $pattern {
            puts stderr "forbidden prompt seen: $pattern"
            exit 99
        }
        eof
    }
} elseif {$mode == "ctrl-c-after"} {
    expect -re $pattern
    set child [exp_pid]
    send -- "\003"
    after 200
    catch {exec kill -INT -- -$child}
    catch {exec kill -INT $child}
    expect eof
} elseif {$mode == "wait-eof"} {
    expect eof
} else {
    puts stderr "unknown mode: $mode"
    exit 2
}

set ws [wait]
if {[llength $ws] >= 4} {
    exit [lindex $ws 3]
}
exit 1
