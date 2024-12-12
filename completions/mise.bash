#                                                          -*- shell-script -*-
#
#   bash_completion - programmable completion functions for bash 4.2+
#
#   Copyright © 2006-2008, Ian Macdonald <ian@caliban.org>
#             © 2009-2020, Bash Completion Maintainers
#
#   This program is free software; you can redistribute it and/or modify
#   it under the terms of the GNU General Public License as published by
#   the Free Software Foundation; either version 2, or (at your option)
#   any later version.
#
#   This program is distributed in the hope that it will be useful,
#   but WITHOUT ANY WARRANTY; without even the implied warranty of
#   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
#   GNU General Public License for more details.
#
#   You should have received a copy of the GNU General Public License
#   along with this program; if not, write to the Free Software Foundation,
#   Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.
#
#   The latest version of this software can be obtained here:
#
#   https://github.com/scop/bash-completion

BASH_COMPLETION_VERSINFO=(
    2  # x-release-please-major
    15 # x-release-please-minor
    0  # x-release-please-patch
)

if [[ $- == *v* ]]; then
    _comp__init_original_set_v="-v"
else
    _comp__init_original_set_v="+v"
fi

if [[ ${BASH_COMPLETION_DEBUG-} ]]; then
    set -v
else
    set +v
fi

# Turn on extended globbing and programmable completion
shopt -s extglob progcomp

# Declare a compatibility function name
# @param $1 Version of bash-completion where the deprecation occurred
# @param $2 Old function name
# @param $3 New function name
# @since 2.12
_comp_deprecate_func()
{
    if (($# != 3)); then
        printf 'bash_completion: %s: usage: %s DEPRECATION_VERSION OLD_NAME NEW_NAME\n' "$FUNCNAME" "$FUNCNAME"
        return 2
    fi
    if [[ $2 != [a-zA-Z_]*([a-zA-Z_0-9]) ]]; then
        printf 'bash_completion: %s: %s\n' "$FUNCNAME" "\$2: invalid function name '$1'" >&2
        return 2
    elif [[ $3 != [a-zA-Z_]*([a-zA-Z_0-9]) ]]; then
        printf 'bash_completion: %s: %s\n' "$FUNCNAME" "\$3: invalid function name '$2'" >&2
        return 2
    fi
    eval -- "$2() { $3 \"\$@\"; }"
}

# Declare a compatibility variable name.
# For bash 4.3+, a real name alias is created, allowing value changes to
# "apply through" when the variables are set later. For bash versions earlier
# than that, the operation is once-only; the value of the new variable
# (if it's unset) is set to that of the old (if set) at call time.
#
# @param $1 Version of bash-completion where the deprecation occurred
# @param $2 Old variable name
# @param $3 New variable name
# @since 2.12
_comp_deprecate_var()
{
    if (($# != 3)); then
        printf 'bash_completion: %s: usage: %s DEPRECATION_VERSION OLD_NAME NEW_NAME\n' "$FUNCNAME" "$FUNCNAME"
        return 2
    fi
    if [[ $2 != [a-zA-Z_]*([a-zA-Z_0-9]) ]]; then
        printf 'bash_completion: %s: %s\n' "$FUNCNAME" "\$2: invalid variable name '$1'" >&2
        return 2
    elif [[ $3 != [a-zA-Z_]*([a-zA-Z_0-9]) ]]; then
        printf 'bash_completion: %s: %s\n' "$FUNCNAME" "\$3: invalid variable name '$2'" >&2
        return 2
    fi
    if ((BASH_VERSINFO[0] >= 5 || BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 3)); then
        eval "declare -gn $2=$3"
    elif [[ -v $2 && ! -v $3 ]]; then
        printf -v "$3" %s "$2"
    fi
}

# A lot of the following one-liners were taken directly from the
# completion examples provided with the bash 2.04 source distribution

# start of section containing compspecs that can be handled within bash

# user commands see only users
complete -u groups slay w sux

# bg completes with stopped jobs
complete -A stopped -P '"%' -S '"' bg

# other job commands
complete -j -P '"%' -S '"' fg jobs disown

# readonly and unset complete with shell variables
complete -v readonly unset

# shopt completes with shopt options
complete -A shopt shopt

# unalias completes with aliases
complete -a unalias

# type and which complete on commands
complete -c command type which

# builtin completes on builtins
complete -b builtin

# start of section containing completion functions called by other functions

# Check if we're running on the given userland
# @param $1 userland to check for
# @since 2.12
_comp_userland()
{
    local userland=$(uname -s)
    [[ $userland == @(Linux|GNU/*) ]] && userland=GNU
    [[ $userland == "$1" ]]
}

# This function sets correct SysV init directories
#
# @since 2.12
_comp_sysvdirs()
{
    sysvdirs=()
    [[ -d /etc/rc.d/init.d ]] && sysvdirs+=(/etc/rc.d/init.d)
    [[ -d /etc/init.d ]] && sysvdirs+=(/etc/init.d)
    # Slackware uses /etc/rc.d
    [[ -f /etc/slackware-version ]] && sysvdirs=(/etc/rc.d)
    ((${#sysvdirs[@]}))
}

# This function checks whether we have a given program on the system.
#
# @since 2.12
_comp_have_command()
{
    # Completions for system administrator commands are installed as well in
    # case completion is attempted via `sudo command ...'.
    PATH=$PATH:/usr/sbin:/sbin:/usr/local/sbin type "$1" &>/dev/null
}

# This function checks whether a given readline variable
# is `on'.
#
# @since 2.12
_comp_readline_variable_on()
{
    [[ $(bind -v) == *$1+([[:space:]])on* ]]
}

# This function shell-quotes the argument
# @param    $1  String to be quoted
# @var[out] REPLY Resulting string
# @since 2.12
_comp_quote()
{
    REPLY=\'${1//\'/\'\\\'\'}\'
}

# shellcheck disable=SC1003
_comp_dequote__initialize()
{
    local regex_param='\$([_a-zA-Z][_a-zA-Z0-9]*|[-*@#?$!0-9_])|\$\{[!#]?([_a-zA-Z][_a-zA-Z0-9]*(\[([0-9]+|[*@])\])?|[-*@#?$!0-9_])\}'
    local regex_quoted='\\.|'\''[^'\'']*'\''|\$?"([^\"$`!]|'$regex_param'|\\.)*"|\$'\''([^\'\'']|\\.)*'\'''
    _comp_dequote__regex_safe_word='^([^\'\''"$`;&|<>()!]|'$regex_quoted'|'$regex_param')*$'
    unset -f "$FUNCNAME"
}
_comp_dequote__initialize

# This function expands a word using `eval` in a safe way.  This function can
# be typically used to get the expanded value of `${word[i]}` as
# `_comp_dequote "${word[i]}"`.  When the word contains unquoted shell special
# characters, command substitutions, and other unsafe strings, the function
# call fails before applying `eval`.  Otherwise, `eval` is applied to the
# string to generate the result.
#
# @param    $1  String to be expanded.  A safe word consists of the following
#               sequence of substrings:
#
#               - Shell non-special characters: [^\'"$`;&|<>()!].
#               - Parameter expansions of the forms $PARAM, ${!PARAM},
#                 ${#PARAM}, ${NAME[INDEX]}, ${!NAME[INDEX]}, ${#NAME[INDEX]}
#                 where INDEX is an integer, `*` or `@`, NAME is a valid
#                 variable name [_a-zA-Z][_a-zA-Z0-9]*, and PARAM is NAME or a
#                 parameter [-*@#?$!0-9_].
#               - Quotes \?, '...', "...", $'...', and $"...".  In the double
#                 quotations, parameter expansions are allowed.
#
# @var[out] REPLY  Array that contains the expanded results.  Multiple words or
#                  no words may be generated through pathname expansions.
#
# Note: This function allows parameter expansions as safe strings, which might
# cause unexpected results:
#
# * This allows execution of arbitrary commands through extra expansions of
#   array subscripts in name references. For example,
#
#     declare -n v='dummy[$(echo xxx >/dev/tty)]'
#     echo "$v"            # This line executes the command 'echo xxx'.
#     _comp_dequote '"$v"' # This line also executes it.
#
# * This may change the internal state of the variable that has side effects.
#   For example, the state of the random number generator of RANDOM can change:
#
#     RANDOM=1234               # Set seed
#     echo "$RANDOM"            # This produces 30658.
#     RANDOM=1234               # Reset seed
#     _comp_dequote '"$RANDOM"' # This line changes the internal state.
#     echo "$RANDOM"            # This fails to reproduce 30658.
#
# We allow these parameter expansions as a part of safe strings assuming the
# referential transparency of the simple parameter expansions and the sane
# setup of the variables by the user or other frameworks that the user loads.
# @since 2.12
_comp_dequote()
{
    REPLY=() # fallback value for unsafe word and failglob
    [[ $1 =~ $_comp_dequote__regex_safe_word ]] || return 1
    eval "REPLY=($1)" 2>/dev/null # may produce failglob
}

# Unset the given variables across a scope boundary. Useful for unshadowing
# global scoped variables. Note that simply calling unset on a local variable
# will not unshadow the global variable. Rather, the result will be a local
# variable in an unset state.
# Usage: local IFS='|'; _comp_unlocal IFS
# @param $* Variable names to be unset
# @since 2.12
_comp_unlocal()
{
    if ((BASH_VERSINFO[0] >= 5)) && shopt -q localvar_unset; then
        shopt -u localvar_unset
        unset -v "$@"
        shopt -s localvar_unset
    else
        unset -v "$@"
    fi
}

# Assign variables one scope above the caller
# Usage: local varname [varname ...] &&
#        _comp_upvars [-v varname value] | [-aN varname [value ...]] ...
# Available OPTIONS:
#     -aN  Assign next N values to varname as array
#     -v   Assign single value to varname
# @return  1 if error occurs
# @see https://fvue.nl/wiki/Bash:_Passing_variables_by_reference
# @since 2.12
_comp_upvars()
{
    if ! (($#)); then
        echo "bash_completion: $FUNCNAME: usage: $FUNCNAME" \
            "[-v varname value] | [-aN varname [value ...]] ..." >&2
        return 2
    fi
    while (($#)); do
        case $1 in
            -a*)
                # Error checking
                [[ ${1#-a} ]] || {
                    echo "bash_completion: $FUNCNAME:" \
                        "\`$1': missing number specifier" >&2
                    return 1
                }
                printf %d "${1#-a}" &>/dev/null || {
                    echo bash_completion: \
                        "$FUNCNAME: \`$1': invalid number specifier" >&2
                    return 1
                }
                # Assign array of -aN elements
                # shellcheck disable=SC2015,SC2140  # TODO
                [[ $2 ]] && unset -v "$2" && eval "$2"=\(\"\$"{@:3:${1#-a}}"\"\) &&
                    shift $((${1#-a} + 2)) || {
                    echo bash_completion: \
                        "$FUNCNAME: \`$1${2+ }$2': missing argument(s)" \
                        >&2
                    return 1
                }
                ;;
            -v)
                # Assign single value
                # shellcheck disable=SC2015  # TODO
                [[ $2 ]] && unset -v "$2" && eval "$2"=\"\$3\" &&
                    shift 3 || {
                    echo "bash_completion: $FUNCNAME: $1:" \
                        "missing argument(s)" >&2
                    return 1
                }
                ;;
            *)
                echo "bash_completion: $FUNCNAME: $1: invalid option" >&2
                return 1
                ;;
        esac
    done
}

# Get the list of filenames that match with the specified glob pattern.
# This function does the globbing in a controlled environment, avoiding
# interference from user's shell options/settings or environment variables.
# @param $1 array_name  Array name
#   The array name should not start with an underscore "_", which is internally
#   used.  The array name should not be "GLOBIGNORE" or "GLOBSORT".
# @param $2 pattern     Pattern string to be evaluated.
#   This pattern string will be evaluated using "eval", so brace expansions,
#   parameter expansions, command substitutions, and other expansions will be
#   processed.  The user-provided strings should not be directly specified to
#   this argument.
# @return 0 if at least one path is generated, 1 if no path is generated, or 2
#   if the usage is incorrect.
# @since 2.12
_comp_expand_glob()
{
    if (($# != 2)); then
        printf 'bash-completion: %s: unexpected number of arguments\n' "$FUNCNAME" >&2
        printf 'usage: %s ARRAY_NAME PATTERN\n' "$FUNCNAME" >&2
        return 2
    elif [[ $1 == @(GLOBIGNORE|GLOBSORT|_*|*[^_a-zA-Z0-9]*|[0-9]*|'') ]]; then
        printf 'bash-completion: %s: invalid array name "%s"\n' "$FUNCNAME" "$1" >&2
        return 2
    fi

    # Save and adjust the settings.
    local _original_opts=$SHELLOPTS:$BASHOPTS
    set +o noglob
    shopt -s nullglob
    shopt -u failglob dotglob

    # Also the user's GLOBIGNORE and GLOBSORT (bash >= 5.3) may affect the
    # result of pathname expansions.
    local GLOBIGNORE="" GLOBSORT=name

    # To canonicalize the sorting order of the generated paths, we set
    # LC_COLLATE=C and unset LC_ALL while preserving LC_CTYPE.
    local LC_COLLATE=C LC_CTYPE=${LC_ALL:-${LC_CTYPE:-${LANG-}}} LC_ALL=

    eval -- "$1=()" # a fallback in case that the next line fails.
    eval -- "$1=($2)"

    # Restore the settings.  Note: Changing GLOBIGNORE affects the state of
    # "shopt -q dotglob", so we need to explicitly restore the original state
    # of "shopt -q dotglob".
    _comp_unlocal GLOBIGNORE
    if [[ :$_original_opts: == *:dotglob:* ]]; then
        shopt -s dotglob
    else
        shopt -u dotglob
    fi
    [[ :$_original_opts: == *:nullglob:* ]] || shopt -u nullglob
    [[ :$_original_opts: == *:failglob:* ]] && shopt -s failglob
    [[ :$_original_opts: == *:noglob:* ]] && set -o noglob
    eval "((\${#$1[@]}))"
}

# Split a string and assign to an array.  This function basically performs
# `IFS=<sep>; <array_name>=(<text>)` but properly handles saving/restoring the
# state of `IFS` and the shell option `noglob`.  A naive splitting by
# `arr=(...)` suffers from unexpected IFS and pathname expansions, so one
# should prefer this function to such naive splitting.
# OPTIONS
#   -a      Append to the array
#   -F sep  Set a set of separator characters (used as IFS).  The default
#           separator is $' \t\n'
#   -l      The same as -F $'\n'
# @param $1 array_name  The array name
#   The array name should not start with an underscores "_", which is
#   internally used.  The array name should not be either "IFS" or
#   "OPT{IND,ARG,ERR}".
# @param $2 text        The string to split
# @return 2 when the usage is wrong, 0 when one or more completions are
#   generated, or 1 when the execution succeeds but no candidates are
#   generated.
# @since 2.12
_comp_split()
{
    local _append="" IFS=$' \t\n'

    local OPTIND=1 OPTARG="" OPTERR=0 _opt
    while getopts ':alF:' _opt "$@"; do
        case $_opt in
            a) _append=set ;;
            l) IFS=$'\n' ;;
            F) IFS=$OPTARG ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 2
                ;;
        esac
    done
    shift "$((OPTIND - 1))"
    if (($# != 2)); then
        printf '%s\n' "bash_completion: $FUNCNAME: unexpected number of arguments" >&2
        printf '%s\n' "usage: $FUNCNAME [-al] [-F SEP] ARRAY_NAME TEXT" >&2
        return 2
    elif [[ $1 == @(*[^_a-zA-Z0-9]*|[0-9]*|''|_*|IFS|OPTIND|OPTARG|OPTERR) ]]; then
        printf '%s\n' "bash_completion: $FUNCNAME: invalid array name '$1'" >&2
        return 2
    fi

    local _original_opts=$SHELLOPTS
    set -o noglob

    local _old_size _new_size
    if [[ $_append ]]; then
        eval "$1+=()" # in case $1 is unset
        eval "_old_size=\${#$1[@]}"
        eval "$1+=(\$2)"
    else
        _old_size=0
        eval "$1=(\$2)"
    fi
    eval "_new_size=\${#$1[@]}"

    [[ :$_original_opts: == *:noglob:* ]] || set +o noglob
    ((_new_size > _old_size))
}

# Helper function for _comp_compgen
# @var[in] $?
# @var[in] _var
# @var[in] _append
# @return original $?
_comp_compgen__error_fallback()
{
    local _status=$?
    if [[ $_append ]]; then
        # make sure existence of variable
        eval -- "$_var+=()"
    else
        eval -- "$_var=()"
    fi
    return "$_status"
}

# Provide a common interface to generate completion candidates in COMPREPLY or
# in a specified array.
# OPTIONS
#   -a      Append to the array
#   -v arr  Store the results to the array ARR. The default is `COMPREPLY`.
#           The array name should not start with an underscores "_", which is
#           internally used.  The array name should not be any of "cur", "IFS"
#           or "OPT{IND,ARG,ERR}".
#   -U var  Unlocalize VAR before performing the assignments.  This option can
#           be specified multiple times to register multiple variables.  This
#           option is supposed to be used in implementing a generator (G1) when
#           G1 defines a local variable name that does not start with `_`.  In
#           such a case, when the target variable specified to G1 by `-v VAR1`
#           conflicts with the local variable, the assignment to the target
#           variable fails to propagate outside G1.  To avoid such a situation,
#           G1 can call `_comp_compgen` with `-U VAR` to unlocalize `VAR`
#           before accessing the target variable.  For a builtin compgen call
#           (i.e., _comp_compgen [options] -- options), VAR is unlocalized
#           after calling the builtin `compgen` but before assigning results to
#           the target array.  For a generator call (i.e., _comp_compgen
#           [options] G2 ...), VAR is unlocalized before calling the child
#           generator function `_comp_compgen_G2`.
#   -c cur  Set a word used as a prefix to filter the completions.  The default
#           is ${cur-}.
#   -R      The same as -c ''.  Use raw outputs without filtering.
#   -C dir  Evaluate compgen/generator in the specified directory.
# @var[in,opt] cur  Used as the default value of a prefix to filter the
#   completions.
#
# Usage #1: _comp_compgen [-alR|-F sep|-v arr|-c cur|-C dir] -- options...
# Call `compgen` with the specified arguments and store the results in the
# specified array.  This function essentially performs arr=($(compgen args...))
# but properly handles shell options, IFS, etc. using _comp_split.  This
# function is equivalent to `_comp_split [-a] -l arr "$(IFS=sep; compgen
# args... -- cur)"`, but this pattern is frequent in the codebase and is good
# to separate out as a function for the possible future implementation change.
# OPTIONS
#   -F sep  Set a set of separator characters (used as IFS in evaluating
#           `compgen').  The default separator is $' \t\n'.  Note that this is
#           not the set of separators to delimit output of `compgen', but the
#           separators in evaluating the expansions of `-W '...'`, etc.  The
#           delimiter of the output of `compgen` is always a newline.
#   -l      The same as -F $'\n'.  Use lines as words in evaluating compgen.
# @param $1... options  Arguments that are passed to compgen (if $1 starts with
#   a hyphen `-`).
#
#   Note: References to positional parameters $1, $2, ... (such as -W '$1')
#   will not work as expected because these reference the arguments of
#   `_comp_compgen' instead of those of the caller function.  When there are
#   needs to reference them, save the arguments to an array and reference the
#   array instead.
#
#   Note: The array option `-V arr` in bash >= 5.3 should be instead specified
#   as `-v arr` as a part of the `_comp_compgen` options.
# @return  True (0) if at least one completion is generated, False (1) if no
#   completion is generated, or 2 with an incorrect usage.
#
# Usage #2: _comp_compgen [-aR|-v arr|-c cur|-C dir|-i cmd|-x cmd] name args...
# Call the generator `_comp_compgen_NAME ARGS...` with the specified options.
# This provides a common interface to call the functions `_comp_compgen_NAME`,
# which produce completion candidates, with custom options [-alR|-v arr|-c
# cur].  The option `-F sep` is not used with this usage.
# OPTIONS
#   -x cmd  Call exported generator `_comp_xfunc_CMD_compgen_NAME`
#   -i cmd  Call internal generator `_comp_cmd_CMD__compgen_NAME`
# @param $1... name args  Calls the function _comp_compgen_NAME with the
#   specified ARGS (if $1 does not start with a hyphen `-`).  The options
#   [-alR|-v arr|-c cur] are inherited by the child calls of `_comp_compgen`
#   inside `_comp_compgen_NAME` unless the child call `_comp_compgen` receives
#   overriding options.
# @var[in,opt,internal] _comp_compgen__append
# @var[in,opt,internal] _comp_compgen__var
# @var[in,opt,internal] _comp_compgen__cur
#   These variables are internally used to pass the effect of the options
#   [-alR|-v arr|-c cur] to the child calls of `_comp_compgen` in
#   `_comp_compgen_NAME`.
# @return   Exit status of the generator.
#
# @remarks When no options are supplied to _comp_compgen, `_comp_compgen NAME
# args` is equivalent to the direct call `_comp_compgen_NAME args`.  As the
# direct call is slightly more efficient, the direct call is preferred over
# calling it through `_comp_compgen`.
#
# @remarks Design `_comp_compgen_NAME`: a function that produce completions can
# be defined with the name _comp_compgen_NAME.  The function is supposed to
# generate completions by calling `_comp_compgen`.  To reflect the options
# specified to the outer calls of `_comp_compgen`, the function should not
# directly modify `COMPREPLY`.  To add words, one can call
#
#     _comp_compgen -- -W '"${words[@]}"'
#
# To directly add words without filtering by `cur`, one can call
#
#     _comp_compgen -R -- -W '"${words[@]}"'
#
# or use the utility `_comp_compgen_set`:
#
#     _comp_compgen_set "${words[@]}"
#
# Other nested calls of _comp_compgen can also be used.  The function is
# supposed to replace the existing content of the array by default to allow the
# caller control whether to replace or append by the option `-a`.
#
# @since 2.12
_comp_compgen()
{
    local _append=
    local _var=
    local _cur=${_comp_compgen__cur-${cur-}}
    local _dir=""
    local _ifs=$' \t\n' _has_ifs=""
    local _icmd="" _xcmd=""
    local -a _upvars=()

    local _old_nocasematch=""
    if shopt -q nocasematch; then
        _old_nocasematch=set
        shopt -u nocasematch
    fi
    local OPTIND=1 OPTARG="" OPTERR=0 _opt
    while getopts ':av:U:Rc:C:lF:i:x:' _opt "$@"; do
        case $_opt in
            a) _append=set ;;
            v)
                if [[ $OPTARG == @(*[^_a-zA-Z0-9]*|[0-9]*|''|_*|IFS|OPTIND|OPTARG|OPTERR|cur) ]]; then
                    printf 'bash_completion: %s: -v: invalid array name `%s'\''\n' "$FUNCNAME" "$OPTARG" >&2
                    return 2
                fi
                _var=$OPTARG
                ;;
            U)
                if [[ $OPTARG == @(*[^_a-zA-Z0-9]*|[0-9]*|'') ]]; then
                    printf 'bash_completion: %s: -U: invalid variable name `%s'\''\n' "$FUNCNAME" "$OPTARG" >&2
                    return 2
                elif [[ $OPTARG == @(_*|IFS|OPTIND|OPTARG|OPTERR|cur) ]]; then
                    printf 'bash_completion: %s: -U: unnecessary to mark `%s'\'' as upvar\n' "$FUNCNAME" "$OPTARG" >&2
                    return 2
                fi
                _upvars+=("$OPTARG")
                ;;
            c) _cur=$OPTARG ;;
            R) _cur="" ;;
            C)
                if [[ ! $OPTARG ]]; then
                    printf 'bash_completion: %s: -C: invalid directory name `%s'\''\n' "$FUNCNAME" "$OPTARG" >&2
                    return 2
                fi
                _dir=$OPTARG
                ;;
            l) _has_ifs=set _ifs=$'\n' ;;
            F) _has_ifs=set _ifs=$OPTARG ;;
            [ix])
                if [[ ! $OPTARG ]]; then
                    printf 'bash_completion: %s: -%s: invalid command name `%s'\''\n' "$FUNCNAME" "$_opt" "$OPTARG" >&2
                    return 2
                elif [[ $_icmd ]]; then
                    printf 'bash_completion: %s: -%s: `-i %s'\'' is already specified\n' "$FUNCNAME" "$_opt" "$_icmd" >&2
                    return 2
                elif [[ $_xcmd ]]; then
                    printf 'bash_completion: %s: -%s: `-x %s'\'' is already specified\n' "$FUNCNAME" "$_opt" "$_xcmd" >&2
                    return 2
                fi
                ;;&
            i) _icmd=$OPTARG ;;
            x) _xcmd=$OPTARG ;;
            *)
                printf 'bash_completion: %s: usage error\n' "$FUNCNAME" >&2
                return 2
                ;;
        esac
    done
    [[ $_old_nocasematch ]] && shopt -s nocasematch
    shift "$((OPTIND - 1))"
    if (($# == 0)); then
        printf 'bash_completion: %s: unexpected number of arguments\n' "$FUNCNAME" >&2
        printf 'usage: %s [-alR|-F SEP|-v ARR|-c CUR] -- ARGS...' "$FUNCNAME" >&2
        return 2
    fi
    if [[ ! $_var ]]; then
        # Inherit _append and _var only when -v var is unspecified.
        _var=${_comp_compgen__var-COMPREPLY}
        [[ $_append ]] || _append=${_comp_compgen__append-}
    fi

    if [[ $1 != -* ]]; then
        # usage: _comp_compgen [options] NAME args
        if [[ $_has_ifs ]]; then
            printf 'bash_completion: %s: `-l'\'' and `-F sep'\'' are not supported for generators\n' "$FUNCNAME" >&2
            return 2
        fi

        local -a _generator
        if [[ $_icmd ]]; then
            _generator=("_comp_cmd_${_icmd//[^a-zA-Z0-9_]/_}__compgen_$1")
        elif [[ $_xcmd ]]; then
            _generator=(_comp_xfunc "$_xcmd" "compgen_$1")
        else
            _generator=("_comp_compgen_$1")
        fi
        if ! declare -F -- "${_generator[0]}" &>/dev/null; then
            printf 'bash_completion: %s: unrecognized generator `%s'\'' (function %s not found)\n' "$FUNCNAME" "$1" "${_generator[0]}" >&2
            return 2
        fi
        shift

        _comp_compgen__call_generator "$@"
    else
        # usage: _comp_compgen [options] -- [compgen_options]
        if [[ $_icmd || $_xcmd ]]; then
            printf 'bash_completion: %s: generator name is unspecified for `%s'\''\n' "$FUNCNAME" "${_icmd:+-i $_icmd}${_xcmd:+x $_xcmd}" >&2
            return 2
        fi

        # Note: $* in the below checks would be affected by uncontrolled IFS in
        # bash >= 5.0, so we need to set IFS to the normal value.  The behavior
        # in bash < 5.0, where unquoted $* in conditional command did not honor
        # IFS, was a bug.
        # Note: Also, ${_cur:+-- "$_cur"} and ${_append:+-a} would be affected
        # by uncontrolled IFS.
        local IFS=$' \t\n'
        # Note: extglob *\$?(\{)[0-9]* can be extremely slow when the string
        # "${*:2:_nopt}" becomes longer, so we test \$[0-9] and \$\{[0-9]
        # separately.
        if [[ $* == *\$[0-9]* || $* == *\$\{[0-9]* ]]; then
            printf 'bash_completion: %s: positional parameter $1, $2, ... do not work inside this function\n' "$FUNCNAME" >&2
            return 2
        fi

        _comp_compgen__call_builtin "$@"
    fi
}

# Helper function for _comp_compgen.  This function calls a generator.
# @param $1... generator_args
# @var[in] _dir
# @var[in] _cur
# @arr[in] _generator
# @arr[in] _upvars
# @var[in] _append
# @var[in] _var
_comp_compgen__call_generator()
{
    ((${#_upvars[@]})) && _comp_unlocal "${_upvars[@]}"

    if [[ $_dir ]]; then
        local _original_pwd=$PWD
        local PWD=${PWD-} OLDPWD=${OLDPWD-}
        # Note: We also redirect stdout because `cd` may output the target
        # directory to stdout when CDPATH is set.
        command cd -- "$_dir" &>/dev/null ||
            {
                _comp_compgen__error_fallback
                return
            }
    fi

    local _comp_compgen__append=$_append
    local _comp_compgen__var=$_var
    local _comp_compgen__cur=$_cur cur=$_cur
    # Note: we use $1 as a part of a function name, and we use $2... as
    # arguments to the function if any.
    # shellcheck disable=SC2145
    "${_generator[@]}" "$@"
    local _status=$?

    # Go back to the original directory.
    # Note: Failure of this line results in the change of the current
    # directory visible to the user.  We intentionally do not redirect
    # stderr so that the error message appear in the terminal.
    # shellcheck disable=SC2164
    [[ $_dir ]] && command cd -- "$_original_pwd"

    return "$_status"
}

# Helper function for _comp_compgen.  This function calls the builtin compgen.
# @param $1... compgen_args
# @var[in] _dir
# @var[in] _ifs
# @var[in] _cur
# @arr[in] _upvars
# @var[in] _append
# @var[in] _var
if ((BASH_VERSINFO[0] > 5 || BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 3)); then
    # bash >= 5.3 has `compgen -V array_name`
    _comp_compgen__call_builtin()
    {
        if [[ $_dir ]]; then
            local _original_pwd=$PWD
            local PWD=${PWD-} OLDPWD=${OLDPWD-}
            # Note: We also redirect stdout because `cd` may output the target
            # directory to stdout when CDPATH is set.
            command cd -- "$_dir" &>/dev/null || {
                _comp_compgen__error_fallback
                return
            }
        fi

        local -a _result=()

        # Note: We specify -X '' to exclude empty completions to make the
        # behavior consistent with the implementation for Bash < 5.3 where
        # `_comp_split -l` removes empty lines.  If the caller specifies -X
        # pat, the effect of -X '' is overwritten by the specified one.
        IFS=$_ifs compgen -V _result -X '' "$@" ${_cur:+-- "$_cur"} || {
            _comp_compgen__error_fallback
            return
        }

        # Go back to the original directory.
        # Note: Failure of this line results in the change of the current
        # directory visible to the user.  We intentionally do not redirect
        # stderr so that the error message appear in the terminal.
        # shellcheck disable=SC2164
        [[ $_dir ]] && command cd -- "$_original_pwd"

        ((${#_upvars[@]})) && _comp_unlocal "${_upvars[@]}"
        ((${#_result[@]})) || return
        if [[ $_append ]]; then
            eval -- "$_var+=(\"\${_result[@]}\")"
        else
            eval -- "$_var=(\"\${_result[@]}\")"
        fi
        return
    }
else
    _comp_compgen__call_builtin()
    {
        local _result
        _result=$(
            if [[ $_dir ]]; then
                # Note: We also redirect stdout because `cd` may output the target
                # directory to stdout when CDPATH is set.
                command cd -- "$_dir" &>/dev/null || return
            fi
            IFS=$_ifs compgen "$@" ${_cur:+-- "$_cur"}
        ) || {
            _comp_compgen__error_fallback
            return
        }

        ((${#_upvars[@]})) && _comp_unlocal "${_upvars[@]}"
        _comp_split -l ${_append:+-a} "$_var" "$_result"
    }
fi

# usage: _comp_compgen_set [words...]
# Reset COMPREPLY with the specified WORDS.  If no arguments are specified, the
# array is cleared.
#
# When an array name is specified by `-v VAR` in a caller _comp_compgen, the
# array is reset instead of COMPREPLY.  When the `-a` flag is specified in a
# caller _comp_compgen, the words are appended to the existing elements of the
# array instead of replacing the existing elements.  This function ignores
# ${cur-} or the prefix specified by `-v CUR`.
# @return 0 if at least one completion is generated, or 1 otherwise.
# @since 2.12
_comp_compgen_set()
{
    local _append=${_comp_compgen__append-}
    local _var=${_comp_compgen__var-COMPREPLY}
    eval -- "$_var${_append:++}=(\"\$@\")"
    (($#))
}

# Simply split the text and generate completions.  This function should be used
# instead of `_comp_compgen -- -W "$(command)"`, which is vulnerable because
# option -W evaluates the shell expansions included in the option argument.
# Options:
#   -F sep  Specify the separators. The default is $' \t\n'
#   -l      The same as -F $'\n'
#   -X arg  The same as the compgen option -X.
#   -S arg  The same as the compgen option -S.
#   -P arg  The same as the compgen option -P.
#   -o arg  The same as the compgen option -o.
# @param $1 String to split
# @return 0 if at least one completion is generated, or 1 otherwise.
# @since 2.12
_comp_compgen_split()
{
    local _ifs=$' \t\n'
    local -a _compgen_options=()

    local OPTIND=1 OPTARG="" OPTERR=0 _opt
    while getopts ':lF:X:S:P:o:' _opt "$@"; do
        case $_opt in
            l) _ifs=$'\n' ;;
            F) _ifs=$OPTARG ;;
            [XSPo]) _compgen_options+=("-$_opt" "$OPTARG") ;;
            *)
                printf 'bash_completion: usage: %s [-l|-F sep] [--] str\n' "$FUNCNAME" >&2
                return 2
                ;;
        esac
    done
    shift "$((OPTIND - 1))"
    if (($# != 1)); then
        printf 'bash_completion: %s: unexpected number of arguments.\n' "$FUNCNAME" >&2
        printf 'usage: %s [-l|-F sep] [--] str' "$FUNCNAME" >&2
        return 2
    fi

    local input=$1 IFS=$' \t\n'
    _comp_compgen -F "$_ifs" -U input -- ${_compgen_options[@]+"${_compgen_options[@]}"} -W '$input'
}

# Check if the argument looks like a path.
# @param $1 thing to check
# @return True (0) if it does, False (> 0) otherwise
# @since 2.12
_comp_looks_like_path()
{
    [[ ${1-} == @(*/|[.~])* ]]
}

# Reassemble command line words, excluding specified characters from the
# list of word completion separators (COMP_WORDBREAKS).
# @param $1 chars  Characters out of $COMP_WORDBREAKS which should
#     NOT be considered word breaks. This is useful for things like scp where
#     we want to return host:path and not only path, so we would pass the
#     colon (:) as $1 here.
# @param $2 words  Name of variable to return words to
# @param $3 cword  Name of variable to return cword to
#
_comp__reassemble_words()
{
    local exclude="" i j line ref
    # Exclude word separator characters?
    if [[ $1 ]]; then
        # Yes, exclude word separator characters;
        # Exclude only those characters, which were really included
        exclude="[${1//[^$COMP_WORDBREAKS]/}]"
    fi

    # Default to cword unchanged
    printf -v "$3" %s "$COMP_CWORD"
    # Are characters excluded which were former included?
    if [[ $exclude ]]; then
        # Yes, list of word completion separators has shrunk;
        line=$COMP_LINE
        # Re-assemble words to complete
        for ((i = 0, j = 0; i < ${#COMP_WORDS[@]}; i++, j++)); do
            # Is current word not word 0 (the command itself) and is word not
            # empty and is word made up of just word separator characters to
            # be excluded and is current word not preceded by whitespace in
            # original line?
            while [[ $i -gt 0 && ${COMP_WORDS[i]} == +($exclude) ]]; do
                # Is word separator not preceded by whitespace in original line
                # and are we not going to append to word 0 (the command
                # itself), then append to current word.
                [[ $line != [[:blank:]]* ]] && ((j >= 2)) && ((j--))
                # Append word separator to current or new word
                ref="$2[$j]"
                printf -v "$ref" %s "${!ref-}${COMP_WORDS[i]}"
                # Indicate new cword
                ((i == COMP_CWORD)) && printf -v "$3" %s "$j"
                # Remove optional whitespace + word separator from line copy
                line=${line#*"${COMP_WORDS[i]}"}
                # Indicate next word if available, else end *both* while and
                # for loop
                if ((i < ${#COMP_WORDS[@]} - 1)); then
                    ((i++))
                else
                    break 2
                fi
                # Start new word if word separator in original line is
                # followed by whitespace.
                [[ $line == [[:blank:]]* ]] && ((j++))
            done
            # Append word to current word
            ref="$2[$j]"
            printf -v "$ref" %s "${!ref-}${COMP_WORDS[i]}"
            # Remove optional whitespace + word from line copy
            line=${line#*"${COMP_WORDS[i]}"}
            # Indicate new cword
            ((i == COMP_CWORD)) && printf -v "$3" %s "$j"
        done
        ((i == COMP_CWORD)) && printf -v "$3" %s "$j"
    else
        # No, list of word completions separators hasn't changed;
        for i in "${!COMP_WORDS[@]}"; do
            printf -v "$2[i]" %s "${COMP_WORDS[i]}"
        done
    fi
}

# @param $1 exclude  Characters out of $COMP_WORDBREAKS which should NOT be
#     considered word breaks. This is useful for things like scp where
#     we want to return host:path and not only path, so we would pass the
#     colon (:) as $1 in this case.
# @param $2 words  Name of variable to return words to
# @param $3 cword  Name of variable to return cword to
# @param $4 cur  Name of variable to return current word to complete to
# @see _comp__reassemble_words()
_comp__get_cword_at_cursor()
{
    local cword words=()
    _comp__reassemble_words "$1" words cword

    local i cur="" index=$COMP_POINT lead=${COMP_LINE:0:COMP_POINT}
    # Cursor not at position 0 and not led by just space(s)?
    if [[ $index -gt 0 && ($lead && ${lead//[[:space:]]/}) ]]; then
        cur=$COMP_LINE
        for ((i = 0; i <= cword; ++i)); do
            # Current word fits in $cur, and $cur doesn't match cword?
            while [[ ${#cur} -ge ${#words[i]} &&
                ${cur:0:${#words[i]}} != "${words[i]-}" ]]; do
                # Strip first character
                cur=${cur:1}
                # Decrease cursor position, staying >= 0
                ((index > 0)) && ((index--))
            done

            # Does found word match cword?
            if ((i < cword)); then
                # No, cword lies further;
                local old_size=${#cur}
                cur=${cur#"${words[i]}"}
                local new_size=${#cur}
                ((index -= old_size - new_size))
            fi
        done
        # Clear $cur if just space(s)
        [[ $cur && ! ${cur//[[:space:]]/} ]] && cur=
        # Zero $index if negative
        ((index < 0)) && index=0
    fi

    local IFS=$' \t\n'
    local "$2" "$3" "$4" && _comp_upvars -a"${#words[@]}" "$2" ${words[@]+"${words[@]}"} \
        -v "$3" "$cword" -v "$4" "${cur:0:index}"
}

# Get the word to complete and optional previous words.
# This is nicer than ${COMP_WORDS[COMP_CWORD]}, since it handles cases
# where the user is completing in the middle of a word.
# (For example, if the line is "ls foobar",
# and the cursor is here -------->   ^
# Also one is able to cross over possible wordbreak characters.
# Usage: _comp_get_words [OPTIONS] [VARNAMES]
# Available VARNAMES:
#     cur         Return cur via $cur
#     prev        Return prev via $prev
#     words       Return words via $words
#     cword       Return cword via $cword
#
# Available OPTIONS:
#     -n EXCLUDE  Characters out of $COMP_WORDBREAKS which should NOT be
#                 considered word breaks. This is useful for things like scp
#                 where we want to return host:path and not only path, so we
#                 would pass the colon (:) as -n option in this case.
#     -c VARNAME  Return cur via $VARNAME
#     -p VARNAME  Return prev via $VARNAME
#     -w VARNAME  Return words via $VARNAME
#     -i VARNAME  Return cword via $VARNAME
#
# Example usage:
#
#    $ _comp_get_words -n : cur prev
#
# @since 2.12
_comp_get_words()
{
    local exclude="" flag i OPTIND=1
    local cur cword words=()
    local upargs=() upvars=() vcur="" vcword="" vprev="" vwords=""

    while getopts "c:i:n:p:w:" flag "$@"; do
        case $flag in
            [cipw])
                if [[ $OPTARG != [a-zA-Z_]*([a-zA-Z_0-9])?(\[*\]) ]]; then
                    echo "bash_completion: $FUNCNAME: -$flag: invalid variable name \`$OPTARG'" >&2
                    return 1
                fi
                ;;&
            c) vcur=$OPTARG ;;
            i) vcword=$OPTARG ;;
            n) exclude=$OPTARG ;;
            p) vprev=$OPTARG ;;
            w) vwords=$OPTARG ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 1
                ;;
        esac
    done
    while [[ $# -ge $OPTIND ]]; do
        case ${!OPTIND} in
            cur) vcur=cur ;;
            prev) vprev=prev ;;
            cword) vcword=cword ;;
            words) vwords=words ;;
            *)
                echo "bash_completion: $FUNCNAME: \`${!OPTIND}':" \
                    "unknown argument" >&2
                return 1
                ;;
        esac
        ((OPTIND += 1))
    done

    _comp__get_cword_at_cursor "${exclude-}" words cword cur

    [[ $vcur ]] && {
        upvars+=("$vcur")
        upargs+=(-v "$vcur" "$cur")
    }
    [[ $vcword ]] && {
        upvars+=("$vcword")
        upargs+=(-v "$vcword" "$cword")
    }
    [[ $vprev ]] && {
        local value=""
        ((cword >= 1)) && value=${words[cword - 1]}
        upvars+=("$vprev")
        upargs+=(-v "$vprev" "$value")
    }
    [[ $vwords ]] && {
        # Note: bash < 4.4 has a bug that all the elements are connected with
        # ${v+"$@"} when IFS does not contain whitespace.
        local IFS=$' \t\n'
        upvars+=("$vwords")
        upargs+=(-a"${#words[@]}" "$vwords" ${words+"${words[@]}"})
    }

    ((${#upvars[@]})) && local "${upvars[@]}" && _comp_upvars "${upargs[@]}"
}

# Generate the specified items after left-trimming with the word-to-complete
# containing a colon (:).  If the word-to-complete does not contain a colon,
# this generates the specified items without modifications.
# @param $@     items to generate
# @var[in] cur  current word to complete
#
# @remarks In Bash, with a colon in COMP_WORDBREAKS, words containing colons
# are always completed as entire words if the word to complete contains a
# colon.  This function fixes this behavior by removing the
# colon-containing-prefix from the items.
#
# The preferred solution is to remove the colon (:) from COMP_WORDBREAKS in
# your .bashrc:
#
#    # Remove colon (:) from list of word completion separators
#    COMP_WORDBREAKS=${COMP_WORDBREAKS//:}
#
# See also: Bash FAQ - E13) Why does filename completion misbehave if a colon
# appears in the filename? - https://tiswww.case.edu/php/chet/bash/FAQ
#
# @since 2.12
_comp_compgen_ltrim_colon()
{
    (($#)) || return 0
    local -a _tmp
    _tmp=("$@")
    if [[ $cur == *:* && $COMP_WORDBREAKS == *:* ]]; then
        # Remove colon-word prefix from items
        local _colon_word=${cur%"${cur##*:}"}
        _tmp=("${_tmp[@]#"$_colon_word"}")
    fi
    _comp_compgen_set "${_tmp[@]}"
}

# If the word-to-complete contains a colon (:), left-trim COMPREPLY items with
# word-to-complete.
#
# @param $1 current word to complete (cur)
# @var[in,out] COMPREPLY
#
# @since 2.12
_comp_ltrim_colon_completions()
{
    ((${#COMPREPLY[@]})) || return 0
    _comp_compgen -c "$1" ltrim_colon "${COMPREPLY[@]}"
}

# This function quotes the argument in a way so that readline dequoting
# results in the original argument.  This is necessary for at least
# `compgen` which requires its arguments quoted/escaped:
#
#     $ ls "a'b/"
#     c
#     $ compgen -f "a'b/"       # Wrong, doesn't return output
#     $ compgen -f "a\'b/"      # Good
#     a\'b/c
#
# See also:
# - https://lists.gnu.org/archive/html/bug-bash/2009-03/msg00155.html
# - https://www.mail-archive.com/bash-completion-devel@lists.alioth.debian.org/msg01944.html
# @param $1      Argument to quote
# @var[out] REPLY  Quoted result is stored in this variable
# @since 2.12
# shellcheck disable=SC2178 # The assignment is not intended for the global "REPLY"
_comp_quote_compgen()
{
    if [[ $1 == \'* ]]; then
        # Leave out first character
        REPLY=${1:1}
    else
        printf -v REPLY %q "$1"

        # If result becomes quoted like this: $'string', re-evaluate in order
        # to drop the additional quoting.  See also:
        # https://www.mail-archive.com/bash-completion-devel@lists.alioth.debian.org/msg01942.html
        if [[ $REPLY == \$\'*\' ]]; then
            local value=${REPLY:2:-1} # Strip beginning $' and ending '.
            value=${value//'%'/%%}    # Escape % for printf format.
            # shellcheck disable=SC2059
            printf -v REPLY "$value" # Decode escape sequences of \....
        fi
    fi
}

# This function performs file and directory completion. It's better than
# simply using 'compgen -f', because it honours spaces in filenames.
# @param $1  If `-d', complete only on directories.  Otherwise filter/pick only
#            completions with `.$1' and the uppercase version of it as file
#            extension.
# @return 0 if at least one completion is generated, or 1 otherwise.
#
# @since 2.12
_comp_compgen_filedir()
{
    _comp_compgen_tilde && return

    local -a toks
    local _arg=${1-}

    if [[ $_arg == -d ]]; then
        _comp_compgen -v toks -- -d
    else
        local REPLY
        _comp_quote_compgen "${cur-}"
        local _quoted=$REPLY
        _comp_unlocal REPLY

        # work around bash-4.2 where compgen -f "''" produces nothing.
        [[ $_quoted == "''" ]] && _quoted=""

        # Munge xspec to contain uppercase version too
        # https://lists.gnu.org/archive/html/bug-bash/2010-09/msg00036.html
        # news://news.gmane.io/4C940E1C.1010304@case.edu
        local _xspec=${_arg:+"!*.@($_arg|${_arg^^})"} _plusdirs=()

        # Use plusdirs to get dir completions if we have a xspec; if we don't,
        # there's no need, dirs come along with other completions. Don't use
        # plusdirs quite yet if fallback is in use though, in order to not ruin
        # the fallback condition with the "plus" dirs.
        local _opts=(-f -X "$_xspec")
        [[ $_xspec ]] && _plusdirs=(-o plusdirs)
        [[ ${BASH_COMPLETION_FILEDIR_FALLBACK-} || ! ${_plusdirs-} ]] ||
            _opts+=("${_plusdirs[@]}")

        _comp_compgen -v toks -c "$_quoted" -- "${_opts[@]}"

        # Try without filter if it failed to produce anything and configured to
        [[ ${BASH_COMPLETION_FILEDIR_FALLBACK-} &&
            $_arg && ${#toks[@]} -lt 1 ]] &&
            _comp_compgen -av toks -c "$_quoted" -- \
                -f ${_plusdirs+"${_plusdirs[@]}"}
    fi

    if ((${#toks[@]} != 0)); then
        # Remove . and .. (as well as */. and */..) from suggestions, unless
        # .. or */.. was typed explicitly by the user (for users who use
        # tab-completion to append a slash after '..')
        if [[ $cur != ?(*/).. ]]; then
            _comp_compgen -Rv toks -- -X '?(*/)@(.|..)' -W '"${toks[@]}"'
        fi
    fi

    if ((${#toks[@]} != 0)); then
        # 2>/dev/null for direct invocation, e.g. in the _comp_compgen_filedir
        # unit test
        compopt -o filenames 2>/dev/null
    fi

    # Note: bash < 4.4 has a bug that all the elements are connected with
    # ${v+"${a[@]}"} when IFS does not contain whitespace.
    local IFS=$' \t\n'
    _comp_compgen -U toks set ${toks[@]+"${toks[@]}"}
}

# This function splits $cur=--foo=bar into $prev=--foo, $cur=bar, making it
# easier to support both "--foo bar" and "--foo=bar" style completions.
# `=' should have been removed from COMP_WORDBREAKS when setting $cur for
# this to be useful.
# Returns 0 if current option was split, 1 otherwise.
#
_comp__split_longopt()
{
    if [[ $cur == --?*=* ]]; then
        # Cut also backslash before '=' in case it ended up there
        # for some reason.
        prev=${cur%%?(\\)=*}
        cur=${cur#*=}
        return 0
    fi

    return 1
}

# Complete variables.
# @return  True (0) if variables were completed,
#          False (> 0) if not.
# @since 2.12
_comp_compgen_variables()
{
    if [[ $cur =~ ^(\$(\{[!#]?)?)([A-Za-z0-9_]*)$ ]]; then
        # Completing $var / ${var / ${!var / ${#var
        if [[ $cur == '${'* ]]; then
            local arrs vars
            _comp_compgen -v vars -c "${BASH_REMATCH[3]}" -- -A variable -P "${BASH_REMATCH[1]}" -S '}'
            _comp_compgen -v arrs -c "${BASH_REMATCH[3]}" -- -A arrayvar -P "${BASH_REMATCH[1]}" -S '['
            if ((${#vars[@]} == 1 && ${#arrs[@]} != 0)); then
                # Complete ${arr with ${array[ if there is only one match, and that match is an array variable
                compopt -o nospace
                _comp_compgen -U vars -U arrs -R -- -W '"${arrs[@]}"'
            else
                # Complete ${var with ${variable}
                _comp_compgen -U vars -U arrs -R -- -W '"${vars[@]}"'
            fi
        else
            # Complete $var with $variable
            _comp_compgen -ac "${BASH_REMATCH[3]}" -- -A variable -P '$'
        fi
        return 0
    elif [[ $cur =~ ^(\$\{[#!]?)([A-Za-z0-9_]*)\[([^]]*)$ ]]; then
        # Complete ${array[i with ${array[idx]}
        local vars
        _comp_compgen -v vars -c "${BASH_REMATCH[3]}" -- -W '"${!'"${BASH_REMATCH[2]}"'[@]}"' \
            -P "${BASH_REMATCH[1]}${BASH_REMATCH[2]}[" -S ']}'
        # Complete ${arr[@ and ${arr[*
        if [[ ${BASH_REMATCH[3]} == [@*] ]]; then
            vars+=("${BASH_REMATCH[1]}${BASH_REMATCH[2]}[${BASH_REMATCH[3]}]}")
        fi
        # array indexes may have colons
        if ((${#vars[@]})); then
            _comp_compgen -U vars -c "$cur" ltrim_colon "${vars[@]}"
        else
            _comp_compgen_set
        fi
        return 0
    elif [[ $cur =~ ^\$\{[#!]?[A-Za-z0-9_]*\[.*\]$ ]]; then
        # Complete ${array[idx] with ${array[idx]}
        _comp_compgen -c "$cur" ltrim_colon "$cur}"
        return 0
    fi
    return 1
}

# Complete a delimited value.
#
# Usage: [-k] DELIMITER COMPGEN_ARG...
#         -k: do not filter out already present tokens in value
# @since 2.12
_comp_delimited()
{
    local prefix="" delimiter=$1 deduplicate=set
    shift
    if [[ $delimiter == -k ]]; then
        deduplicate=""
        delimiter=$1
        shift
    fi
    [[ $cur == *"$delimiter"* ]] && prefix=${cur%"$delimiter"*}$delimiter

    if [[ $deduplicate ]]; then
        # We could construct a -X pattern to feed to compgen, but that'd
        # conflict with possibly already set -X in $@, as well as have
        # glob char escaping issues to deal with. Do removals by hand instead.
        _comp_compgen -R -- "$@"
        local -a existing
        _comp_split -F "$delimiter" existing "$cur"
        # Do not remove the last from existing if it's not followed by the
        # delimiter so we get space appended.
        [[ ! $cur || $cur == *"$delimiter" ]] || unset -v "existing[${#existing[@]}-1]"
        if ((${#COMPREPLY[@]})); then
            local x i
            for x in ${existing+"${existing[@]}"}; do
                for i in "${!COMPREPLY[@]}"; do
                    if [[ $x == "${COMPREPLY[i]}" ]]; then
                        unset -v 'COMPREPLY[i]'
                        continue 2 # assume no dupes in COMPREPLY
                    fi
                done
            done
            ((${#COMPREPLY[@]})) &&
                _comp_compgen -c "${cur##*"$delimiter"}" -- -W '"${COMPREPLY[@]}"'
        fi
    else
        _comp_compgen -c "${cur##*"$delimiter"}" -- "$@"
    fi

    # It would seem that in some specific cases we could avoid adding the
    # prefix to all completions, thereby making the list of suggestions
    # cleaner, and only adding it when there's exactly one completion.
    # The cases where this opportunity has been observed involve having
    # `show-all-if-ambiguous` on, but even that has cases where it fails
    # and the last separator including everything before it is lost.
    # https://github.com/scop/bash-completion/pull/913#issuecomment-1490140309
    local i
    for i in "${!COMPREPLY[@]}"; do
        COMPREPLY[i]="$prefix${COMPREPLY[i]}"
    done

    [[ $delimiter != : ]] || _comp_ltrim_colon_completions "$cur"
}

# Complete assignment of various known environment variables.
#
# The word to be completed is expected to contain the entire assignment,
# including the variable name and the "=". Some known variables are completed
# with colon separated values; for those to work, colon should not have been
# used to split words. See related parameters to _comp_initialize.
#
# @param  $1 variable assignment to be completed
# @return True (0) if variable value completion was attempted,
#         False (> 0) if not.
# @since 2.12
_comp_variable_assignments()
{
    local cur=${1-} i

    if [[ $cur =~ ^([A-Za-z_][A-Za-z0-9_]*)=(.*)$ ]]; then
        prev=${BASH_REMATCH[1]}
        cur=${BASH_REMATCH[2]}
    else
        return 1
    fi

    case $prev in
        TZ)
            cur=/usr/share/zoneinfo/$cur
            _comp_compgen_filedir
            if ((${#COMPREPLY[@]})); then
                for i in "${!COMPREPLY[@]}"; do
                    if [[ ${COMPREPLY[i]} == *.tab ]]; then
                        unset -v 'COMPREPLY[i]'
                        continue
                    elif [[ -d ${COMPREPLY[i]} ]]; then
                        COMPREPLY[i]+=/
                        compopt -o nospace
                    fi
                    COMPREPLY[i]=${COMPREPLY[i]#/usr/share/zoneinfo/}
                done
            fi
            ;;
        TERM)
            _comp_compgen_terms
            ;;
        LANG | LC_*)
            _comp_compgen_split -- "$(locale -a 2>/dev/null)"
            ;;
        LANGUAGE)
            _comp_delimited : -W '$(locale -a 2>/dev/null)'
            ;;
        *)
            _comp_compgen_variables && return 0
            _comp_compgen -a filedir
            ;;
    esac

    return 0
}

# Initialize completion and deal with various general things: do file
# and variable completion where appropriate, and adjust prev, words,
# and cword as if no redirections exist so that completions do not
# need to deal with them.  Before calling this function, make sure
# cur, prev, words, and cword are local, ditto split if you use -s.
#
# Options:
#     -n EXCLUDE  Passed to _comp_get_words -n with redirection chars
#     -e XSPEC    Passed to _comp_compgen_filedir as first arg for stderr
#                 redirections
#     -o XSPEC    Passed to _comp_compgen_filedir as first arg for other output
#                 redirections
#     -i XSPEC    Passed to _comp_compgen_filedir as first arg for stdin
#                 redirections
#     -s          Split long options with _comp__split_longopt, implies -n =
# @param $1...$3 args Original arguments specified to the completion function.
#                     The first argument $1 is command name.  The second
#                     argument $2 is the string before the cursor in the
#                     current word.  The third argument $3 is the previous
#                     word.
# @var[out] cur           Reconstructed current word
# @var[out] prev          Reconstructed previous word
# @var[out] words         Reconstructed words
# @var[out] cword         Current word index in `words`
# @var[out] comp_args     Original arguments specified to the completion
#                         function are saved in this array, if the arguments
#                         $1...$3 is specified.
# @var[out,opt] was_split When "-s" is specified, `"set"/""` is set depending
#                         on whether the split happened.
# @return  True (0) if completion needs further processing,
#          False (> 0) no further processing is necessary.
#
# @since 2.12
_comp_initialize()
{
    local exclude="" opt_split="" outx="" errx="" inx=""

    local flag OPTIND=1 OPTARG="" OPTERR=0
    while getopts "n:e:o:i:s" flag "$@"; do
        case $flag in
            n) exclude+=$OPTARG ;;
            e) errx=$OPTARG ;;
            o) outx=$OPTARG ;;
            i) inx=$OPTARG ;;
            s)
                opt_split="set"
                was_split=""
                exclude+="="
                ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 1
                ;;
        esac
    done
    shift "$((OPTIND - 1))"
    (($#)) && comp_args=("$@")

    COMPREPLY=()
    local redir='@(?(+([0-9])|{[a-zA-Z_]*([a-zA-Z_0-9])})@(>?([>|&])|<?([>&])|<<?([-<]))|&>?(>))'
    _comp_get_words -n "$exclude<>&" cur prev words cword

    # Complete variable names.
    _comp_compgen_variables && return 1

    # Complete on files if current is a redirect possibly followed by a
    # filename, e.g. ">foo", or previous is a "bare" redirect, e.g. ">".
    # shellcheck disable=SC2053
    if [[ $cur == $redir* || ${prev-} == $redir ]]; then
        local xspec
        case $cur in
            2'>'*) xspec=${errx-} ;;
            *'>'*) xspec=${outx-} ;;
            *'<'*) xspec=${inx-} ;;
            *)
                case $prev in
                    2'>'*) xspec=${errx-} ;;
                    *'>'*) xspec=${outx-} ;;
                    *'<'*) xspec=${inx-} ;;
                esac
                ;;
        esac
        # shellcheck disable=SC2295 # redir is a pattern
        cur=${cur##$redir}
        _comp_compgen_filedir "$xspec"
        return 1
    fi

    # Remove all redirections so completions don't have to deal with them.
    local i skip
    for ((i = 1; i < ${#words[@]}; )); do
        if [[ ${words[i]} == $redir* ]]; then
            # If "bare" redirect, remove also the next word (skip=2).
            # shellcheck disable=SC2053
            [[ ${words[i]} == $redir ]] && skip=2 || skip=1
            words=("${words[@]:0:i}" "${words[@]:i+skip}")
            ((i <= cword)) && ((cword -= skip))
        else
            ((i++))
        fi
    done

    ((cword <= 0)) && return 1
    prev=${words[cword - 1]}

    [[ $opt_split ]] && _comp__split_longopt && was_split="set"

    return 0
}

# Helper function for _comp_compgen_help and _comp_compgen_usage.
# Obtain the help output based on the arguments.
# @param $@ args  Arguments specified to the caller.
# @var[out] _lines
# @return 2 if the usage is wrong, 1 if no output is obtained, or otherwise 0.
_comp_compgen_help__get_help_lines()
{
    local -a help_cmd
    case ${1-} in
        -)
            if (($# > 1)); then
                printf 'bash_completion: %s -: extra arguments for -\n' "${FUNCNAME[1]}" >&2
                printf 'usage: %s -\n' "${FUNCNAME[1]}" >&2
                printf 'usage: %s -c cmd args...\n' "${FUNCNAME[1]}" >&2
                printf 'usage: %s [-- args...]\n' "${FUNCNAME[1]}" >&2
                return 2
            fi
            help_cmd=(exec cat)
            ;;
        -c)
            if (($# < 2)); then
                printf 'bash_completion: %s -c: no command is specified\n' "${FUNCNAME[1]}" >&2
                printf 'usage: %s -\n' "${FUNCNAME[1]}" >&2
                printf 'usage: %s -c cmd args...\n' "${FUNCNAME[1]}" >&2
                printf 'usage: %s [-- args...]\n' "${FUNCNAME[1]}" >&2
                return 2
            fi
            help_cmd=("${@:2}")
            ;;
        --) shift 1 ;&
        *)
            local REPLY
            _comp_dequote "${comp_args[0]-}" || REPLY=${comp_args[0]-}
            help_cmd=("${REPLY:-false}" "$@")
            ;;
    esac

    local REPLY
    _comp_split -l REPLY "$(LC_ALL=C "${help_cmd[@]}" 2>&1)" &&
        _lines=("${REPLY[@]}")
}

# Helper function for _comp_compgen_help and _comp_compgen_usage.
# @var[in,out] options Add options
# @return True (0) if an option was found, False (> 0) otherwise
_comp_compgen_help__parse()
{
    local option option2 i

    # Take first found long option, or first one (short) if not found.
    option=
    local -a array
    if _comp_split -F $' \t\n,/|' array "$1"; then
        for i in "${array[@]}"; do
            case "$i" in
                ---*) break ;;
                --?*)
                    option=$i
                    break
                    ;;
                -?*) [[ $option ]] || option=$i ;;
                *) break ;;
            esac
        done
    fi
    [[ $option ]] || return 1

    # Expand --[no]foo to --foo and --nofoo etc
    if [[ $option =~ (\[((no|dont)-?)\]). ]]; then
        option2=${option/"${BASH_REMATCH[1]}"/}
        option2=${option2%%[<{().[]*}
        options+=("${option2/=*/=}")
        option=${option/"${BASH_REMATCH[1]}"/"${BASH_REMATCH[2]}"}
    fi

    [[ $option =~ ^([^=<{().[]|\.[A-Za-z0-9])+=? ]] &&
        options+=("$BASH_REMATCH")
}

# Parse GNU style help output of the given command and generate and store
# completions in an array. The help output is produced in the way depending on
# the usage:
# usage: _comp_compgen_help -              # read from stdin
# usage: _comp_compgen_help -c cmd args... # run "cmd args..."
# usage: _comp_compgen_help [[--] args...] # run "${comp_args[0]} args..."
# When no arguments are specified, `--help` is assumed.
#
# @var[in] comp_args[0]
# @since 2.12
_comp_compgen_help()
{
    (($#)) || set -- -- --help

    local -a _lines
    _comp_compgen_help__get_help_lines "$@" || return "$?"

    local -a options=()
    local _line
    for _line in "${_lines[@]}"; do
        [[ $_line == *([[:blank:]])-* ]] || continue
        # transform "-f FOO, --foo=FOO" to "-f , --foo=FOO" etc
        while [[ $_line =~ ((^|[^-])-[A-Za-z0-9?][[:space:]]+)\[?[A-Z0-9]+([,_-]+[A-Z0-9]+)?(\.\.+)?\]? ]]; do
            _line=${_line/"${BASH_REMATCH[0]}"/"${BASH_REMATCH[1]}"}
        done
        _comp_compgen_help__parse "${_line// or /, }"
    done
    ((${#options[@]})) || return 1

    _comp_compgen -U options -- -W '"${options[@]}"'
    return 0
}

# Parse BSD style usage output (options in brackets) of the given command. The
# help output is produced in the way depending on the usage:
# usage: _comp_compgen_usage -              # read from stdin
# usage: _comp_compgen_usage -c cmd args... # run "cmd args..."
# usage: _comp_compgen_usage [[--] args...] # run "${comp_args[0]} args..."
# When no arguments are specified, `--usage` is assumed.
#
# @var[in] comp_args[0]
# @since 2.12
_comp_compgen_usage()
{
    (($#)) || set -- -- --usage

    local -a _lines
    _comp_compgen_help__get_help_lines "$@" || return "$?"

    local -a options=()
    local _line _match _option _i _char
    for _line in "${_lines[@]}"; do
        while [[ $_line =~ \[[[:space:]]*(-[^]]+)[[:space:]]*\] ]]; do
            _match=${BASH_REMATCH[0]}
            _option=${BASH_REMATCH[1]}
            case $_option in
                -?(\[)+([a-zA-Z0-9?]))
                    # Treat as bundled short options
                    for ((_i = 1; _i < ${#_option}; _i++)); do
                        _char=${_option:_i:1}
                        [[ $_char != '[' ]] && options+=("-$_char")
                    done
                    ;;
                *)
                    _comp_compgen_help__parse "$_option"
                    ;;
            esac
            _line=${_line#*"$_match"}
        done
    done
    ((${#options[@]})) || return 1

    _comp_compgen -U options -- -W '"${options[@]}"'
    return 0
}

# This function completes on signal names (minus the SIG prefix)
# @param $1 prefix
#
# @since 2.12
_comp_compgen_signals()
{
    local -a sigs
    _comp_compgen -v sigs -c "SIG${cur#"${1-}"}" -- -A signal &&
        _comp_compgen -RU sigs -- -P "${1-}" -W '"${sigs[@]#SIG}"'
}

# This function completes on known mac addresses
#
# @since 2.12
_comp_compgen_mac_addresses()
{
    local _re='\([A-Fa-f0-9]\{2\}:\)\{5\}[A-Fa-f0-9]\{2\}'
    local PATH="$PATH:/sbin:/usr/sbin"
    local -a addresses

    # Local interfaces
    # - ifconfig on Linux: HWaddr or ether
    # - ifconfig on FreeBSD: ether
    # - ip link: link/ether
    _comp_compgen -v addresses split -- "$(
        {
            ip -c=never link show || ip link show || LC_ALL=C ifconfig -a
        } 2>/dev/null | command sed -ne \
            "s/.*[[:space:]]HWaddr[[:space:]]\{1,\}\($_re\)[[:space:]].*/\1/p" -ne \
            "s/.*[[:space:]]HWaddr[[:space:]]\{1,\}\($_re\)[[:space:]]*$/\1/p" -ne \
            "s|.*[[:space:]]\(link/\)\{0,1\}ether[[:space:]]\{1,\}\($_re\)[[:space:]].*|\2|p" -ne \
            "s|.*[[:space:]]\(link/\)\{0,1\}ether[[:space:]]\{1,\}\($_re\)[[:space:]]*$|\2|p"
    )"

    # ARP cache
    _comp_compgen -av addresses split -- "$(
        {
            arp -an || ip -c=never neigh show || ip neigh show
        } 2>/dev/null | command sed -ne \
            "s/.*[[:space:]]\($_re\)[[:space:]].*/\1/p" -ne \
            "s/.*[[:space:]]\($_re\)[[:space:]]*$/\1/p"
    )"

    # /etc/ethers
    _comp_compgen -av addresses split -- "$(command sed -ne \
        "s/^[[:space:]]*\($_re\)[[:space:]].*/\1/p" /etc/ethers 2>/dev/null)"

    _comp_compgen -U addresses ltrim_colon "${addresses[@]}"
}

# This function completes on configured network interfaces
#
# @since 2.12
_comp_compgen_configured_interfaces()
{
    local -a files
    if [[ -f /etc/debian_version ]]; then
        # Debian system
        _comp_expand_glob files '/etc/network/interfaces /etc/network/interfaces.d/*' || return 0
        _comp_compgen -U files split -- "$(command sed -ne \
            's|^iface \([^ ]\{1,\}\).*$|\1|p' "${files[@]}" 2>/dev/null)"
    elif [[ -f /etc/SuSE-release ]]; then
        # SuSE system
        _comp_expand_glob files '/etc/sysconfig/network/ifcfg-*' || return 0
        _comp_compgen -U files split -- "$(printf '%s\n' "${files[@]}" |
            command sed -ne 's|.*ifcfg-\([^*].*\)$|\1|p')"
    elif [[ -f /etc/pld-release ]]; then
        # PLD Linux
        _comp_compgen -U files split -- "$(command ls -B /etc/sysconfig/interfaces |
            command sed -ne 's|.*ifcfg-\([^*].*\)$|\1|p')"
    else
        # Assume Red Hat
        _comp_expand_glob files '/etc/sysconfig/network-scripts/ifcfg-*' || return 0
        _comp_compgen -U files split -- "$(printf '%s\n' "${files[@]}" |
            command sed -ne 's|.*ifcfg-\([^*].*\)$|\1|p')"
    fi
}

# Local IP addresses.
# If producing IPv6 completions, `_comp_initialize` with `-n :`.
#
# -4: IPv4 addresses only (default)
# -6: IPv6 addresses only
# -a: All addresses
#
# @since 2.12
_comp_compgen_ip_addresses()
{
    local _n
    case ${1-} in
        -a) _n='6\{0,1\}' ;;
        -6) _n='6' ;;
        *) _n= ;;
    esac
    local PATH=$PATH:/sbin
    local addrs
    _comp_compgen -v addrs split -- "$({
        ip -c=never addr show || ip addr show || LC_ALL=C ifconfig -a
    } 2>/dev/null |
        command sed -e 's/[[:space:]]addr:/ /' -ne \
            "s|.*inet${_n}[[:space:]]\{1,\}\([^[:space:]/]*\).*|\1|p")" ||
        return

    if [[ ! $_n ]]; then
        _comp_compgen -U addrs set "${addrs[@]}"
    else
        _comp_compgen -U addrs ltrim_colon "${addrs[@]}"
    fi
}

# This function completes on available kernel versions
#
# @since 2.12
_comp_compgen_kernel_versions()
{
    _comp_compgen_split -- "$(command ls /lib/modules)"
}

# This function completes on all available network interfaces
# -a: restrict to active interfaces only
# -w: restrict to wireless interfaces only
#
# @since 2.12
_comp_compgen_available_interfaces()
{
    local PATH=$PATH:/sbin
    local generated
    _comp_compgen -v generated split -- "$({
        if [[ ${1-} == -w ]]; then
            iwconfig
        elif [[ ${1-} == -a ]]; then
            # Note: we prefer ip (iproute2) to ifconfig (inetutils) since long
            # interface names will be truncated by ifconfig [1].
            # [1]: https://github.com/scop/bash-completion/issues/1089
            ip -c=never link show up || ip link show up || ifconfig
        else
            ip -c=never link show || ip link show || ifconfig -a
        fi
    } 2>/dev/null | _comp_awk \
        '/^[^ \t]/ { if ($1 ~ /^[0-9]+:/) { print $2 } else { print $1 } }')" &&
        _comp_compgen -U generated set "${generated[@]%:}"
}

# Echo number of CPUs, falling back to 1 on failure.
# @var[out] REPLY
# @return 0 if it successfully obtained the number of CPUs, or otherwise 1
# @since 2.12
_comp_get_ncpus()
{
    local var=NPROCESSORS_ONLN
    [[ $OSTYPE == *@(linux|msys|cygwin)* ]] && var=_$var
    if REPLY=$(getconf $var 2>/dev/null) && ((REPLY >= 1)); then
        return 0
    else
        REPLY=1
        return 1
    fi
}

# Perform tilde (~) completion
# @return  False (1) if completion needs further processing,
#          True (0) if tilde is followed by a valid username, completions are
#          put in COMPREPLY and no further processing is necessary.
# @since 2.12
_comp_compgen_tilde()
{
    if [[ ${cur-} == \~* && $cur != */* ]]; then
        # Try generate ~username completions
        if _comp_compgen -c "${cur#\~}" -- -P '~' -u; then
            # 2>/dev/null for direct invocation, e.g. in the
            # _comp_compgen_tilde unit test
            compopt -o filenames 2>/dev/null
            return 0
        fi
    fi
    return 1
}

# Expand string starting with tilde (~)
# We want to expand ~foo/... to /home/foo/... to avoid problems when
# word-to-complete starting with a tilde is fed to commands and ending up
# quoted instead of expanded.
# Only the first portion of the variable from the tilde up to the first slash
# (~../) is expanded.  The remainder of the variable, containing for example
# a dollar sign variable ($) or asterisk (*) is not expanded.
# Example usage:
#
#    $ _comp_expand_tilde "~"; echo "$REPLY"
#
# Example output:
#
#       $1                 REPLY
#    --------         ----------------
#    ~                /home/user
#    ~foo/bar         /home/foo/bar
#    ~foo/$HOME       /home/foo/$HOME
#    ~foo/a  b        /home/foo/a  b
#    ~foo/*           /home/foo/*
#
# @param $1     Value to expand
# @var[out] REPLY Expanded result
# @since 2.12
_comp_expand_tilde()
{
    REPLY=$1
    if [[ $1 == \~* ]]; then
        printf -v REPLY '~%q' "${1#\~}"
        eval "REPLY=$REPLY"
    fi
}

# This function expands tildes in pathnames
#
# @since 2.12
_comp_expand()
{
    # Expand ~username type directory specifications.  We want to expand
    # ~foo/... to /home/foo/... to avoid problems when $cur starting with
    # a tilde is fed to commands and ending up quoted instead of expanded.

    case ${cur-} in
        ~*/*)
            local REPLY
            _comp_expand_tilde "$cur"
            cur=$REPLY
            ;;
        ~*)
            _comp_compgen -v COMPREPLY tilde &&
                eval "COMPREPLY[0]=$(printf ~%q "${COMPREPLY[0]#\~}")" &&
                return 1
            ;;
    esac
    return 0
}

# Process ID related functions.
# for AIX and Solaris we use X/Open syntax, BSD for others.
#
# @since 2.12
if [[ $OSTYPE == *@(solaris|aix)* ]]; then
    # This function completes on process IDs.
    _comp_compgen_pids()
    {
        _comp_compgen_split -- "$(command ps -efo pid | command sed 1d)"
    }

    _comp_compgen_pgids()
    {
        _comp_compgen_split -- "$(command ps -efo pgid | command sed 1d)"
    }
    _comp_compgen_pnames()
    {
        _comp_compgen_split -X '<defunct>' -- "$(command ps -efo comm |
            command sed -e 1d -e 's:.*/::' -e 's/^-//' | sort -u)"
    }
else
    _comp_compgen_pids()
    {
        _comp_compgen_split -- "$(command ps ax -o pid=)"
    }
    _comp_compgen_pgids()
    {
        _comp_compgen_split -- "$(command ps ax -o pgid=)"
    }
    # @param $1 if -s, don't try to avoid truncated command names
    _comp_compgen_pnames()
    {
        local -a procs=()
        if [[ ${1-} == -s ]]; then
            _comp_split procs "$(command ps ax -o comm | command sed -e 1d)"
        else
            # Some versions of ps don't support "command", but do "comm", e.g.
            # some busybox ones. Fall back
            local -a psout
            _comp_split -l psout "$({
                command ps ax -o command= || command ps ax -o comm=
            } 2>/dev/null)"
            local line i=-1
            for line in "${psout[@]}"; do
                if ((i == -1)); then
                    # First line, see if it has COMMAND column header. For
                    # example some busybox ps versions do that, i.e. don't
                    # respect command=
                    if [[ $line =~ ^(.*[[:space:]])COMMAND([[:space:]]|$) ]]; then
                        # It does; store its index.
                        i=${#BASH_REMATCH[1]}
                    else
                        # Nope, fall through to "regular axo command=" parsing.
                        break
                    fi
                else
                    #
                    line=${line:i}   # take command starting from found index
                    line=${line%% *} # trim arguments
                    [[ $line ]] && procs+=("$line")
                fi
            done
            if ((i == -1)); then
                # Regular command= parsing
                for line in "${psout[@]}"; do
                    if [[ $line =~ ^[[(](.+)[])]$ ]]; then
                        procs+=("${BASH_REMATCH[1]}")
                    else
                        line=${line%% *}      # trim arguments
                        line=${line##@(*/|-)} # trim leading path and -
                        [[ $line ]] && procs+=("$line")
                    fi
                done
            fi
        fi
        ((${#procs[@]})) &&
            _comp_compgen -U procs -- -X "<defunct>" -W '"${procs[@]}"'
    }
fi

# This function completes on user IDs
#
# @since 2.12
_comp_compgen_uids()
{
    if type getent &>/dev/null; then
        _comp_compgen_split -- "$(getent passwd | cut -d: -f3)"
    elif type perl &>/dev/null; then
        _comp_compgen_split -- "$(perl -e 'while (($uid) = (getpwent)[2]) { print $uid . "\n" }')"
    else
        # make do with /etc/passwd
        _comp_compgen_split -- "$(cut -d: -f3 /etc/passwd)"
    fi
}

# This function completes on group IDs
#
# @since 2.12
_comp_compgen_gids()
{
    if type getent &>/dev/null; then
        _comp_compgen_split -- "$(getent group | cut -d: -f3)"
    elif type perl &>/dev/null; then
        _comp_compgen_split -- "$(perl -e 'while (($gid) = (getgrent)[2]) { print $gid . "\n" }')"
    else
        # make do with /etc/group
        _comp_compgen_split -- "$(cut -d: -f3 /etc/group)"
    fi
}

# Glob for matching various backup files.
#
_comp_backup_glob='@(#*#|*@(~|.@(bak|orig|rej|swp|@(dpkg|ucf)-*|rpm@(orig|new|save))))'

# Complete on xinetd services
#
# @since 2.12
_comp_compgen_xinetd_services()
{
    local xinetddir=${_comp__test_xinetd_dir:-/etc/xinetd.d}
    if [[ -d $xinetddir ]]; then
        local -a svcs
        if _comp_expand_glob svcs '$xinetddir/!($_comp_backup_glob)'; then
            _comp_compgen -U svcs -U xinetddir -- -W '"${svcs[@]#$xinetddir/}"'
        fi
    fi
}

# This function completes on services
#
# @since 2.12
_comp_compgen_services()
{
    local sysvdirs
    _comp_sysvdirs || return 1

    local services
    _comp_expand_glob services '${sysvdirs[0]}/!($_comp_backup_glob|functions|README)'

    local _generated=$({
        systemctl list-units --full --all ||
            systemctl list-unit-files
    } 2>/dev/null |
        _comp_awk '$1 ~ /\.service$/ { sub("\\.service$", "", $1); print $1 }')
    _comp_split -la services "$_generated"

    if [[ -x /sbin/upstart-udev-bridge ]]; then
        _comp_split -la services "$(initctl list 2>/dev/null | cut -d' ' -f1)"
    fi

    ((${#services[@]})) || return 1
    _comp_compgen -U services -U sysvdirs -- -W '"${services[@]#${sysvdirs[0]}/}"'
}

# This completes on a list of all available service scripts for the
# 'service' command and/or the SysV init.d directory, followed by
# that script's available commands
# This function is in the main bash_completion file rather than in a separate
# one, because we set it up eagerly as completer for scripts in sysv init dirs
# below.
#
# @since 2.12
_comp_complete_service()
{
    local cur prev words cword comp_args
    _comp_initialize -- "$@" || return

    # don't complete past 2nd token
    ((cword > 2)) && return

    if [[ $cword -eq 1 && $prev == ?(*/)service ]]; then
        _comp_compgen_services
        [[ -e /etc/mandrake-release ]] && _comp_compgen_xinetd_services
    else
        local sysvdirs
        _comp_sysvdirs || return 1
        _comp_compgen_split -l -- "$(command sed -e 'y/|/ /' \
            -ne 's/^.*\(U\|msg_u\)sage.*{\(.*\)}.*$/\2/p' \
            "${sysvdirs[0]}/${prev##*/}" 2>/dev/null) start stop"
    fi
} &&
    complete -F _comp_complete_service service

_comp__init_set_up_service_completions()
{
    local sysvdirs svc svcdir svcs
    _comp_sysvdirs &&
        for svcdir in "${sysvdirs[@]}"; do
            if _comp_expand_glob svcs '"$svcdir"/!($_comp_backup_glob)'; then
                for svc in "${svcs[@]}"; do
                    [[ -x $svc ]] && complete -F _comp_complete_service "$svc"
                done
            fi
        done
    unset -f "$FUNCNAME"
}
_comp__init_set_up_service_completions

# This function completes on kernel modules
# @param $1 kernel version
#
# @since 2.12
_comp_compgen_kernel_modules()
{
    local _modpath=/lib/modules/$1
    _comp_compgen_split -- "$(command ls -RL "$_modpath" 2>/dev/null |
        command sed -ne 's/^\(.*\)\.k\{0,1\}o\(\.[gx]z\)\{0,1\}$/\1/p' \
            -e 's/^\(.*\)\.ko\.zst$/\1/p')"
}

# This function completes on inserted kernel modules
# @param $1 prefix to filter with, default $cur
#
# @since 2.12
_comp_compgen_inserted_kernel_modules()
{
    _comp_compgen -c "${1:-$cur}" split -- "$(PATH="$PATH:/sbin" lsmod |
        _comp_awk '{if (NR != 1) print $1}')"
}

# This function completes on user or user:group format; as for chown and cpio.
#
# The : must be added manually; it will only complete usernames initially.
# The legacy user.group format is not supported.
#
# @param $1  If -u, only return users/groups the user has access to in
#            context of current completion.
#
# @since 2.12
_comp_compgen_usergroups()
{
    if [[ $cur == *\\\\* || $cur == *:*:* ]]; then
        # Give up early on if something seems horribly wrong.
        return
    elif [[ $cur == *\\:* ]]; then
        # Completing group after 'user\:gr<TAB>'.
        # Reply with a list of groups prefixed with 'user:', readline will
        # escape to the colon.
        local tmp
        if [[ ${1-} == -u ]]; then
            _comp_compgen -v tmp -c "${cur#*:}" allowed_groups
        else
            _comp_compgen -v tmp -c "${cur#*:}" -- -g
        fi
        if ((${#tmp[@]})); then
            local _prefix=${cur%%*([^:])}
            _prefix=${_prefix//\\/}
            _comp_compgen -Rv tmp -- -P "$_prefix" -W '"${tmp[@]}"'
            _comp_compgen -U tmp set "${tmp[@]}"
        fi
    elif [[ $cur == *:* ]]; then
        # Completing group after 'user:gr<TAB>'.
        # Reply with a list of unprefixed groups since readline with split on :
        # and only replace the 'gr' part
        if [[ ${1-} == -u ]]; then
            _comp_compgen -c "${cur#*:}" allowed_groups
        else
            _comp_compgen -c "${cur#*:}" -- -g
        fi
    else
        # Completing a partial 'usernam<TAB>'.
        #
        # Don't suffix with a : because readline will escape it and add a
        # slash. It's better to complete into 'chown username ' than 'chown
        # username\:'.
        if [[ ${1-} == -u ]]; then
            _comp_compgen_allowed_users
        else
            _comp_compgen -- -u
        fi
    fi
}

# @since 2.12
_comp_compgen_allowed_users()
{
    if _comp_as_root; then
        _comp_compgen -- -u
    else
        _comp_compgen_split -- "$(id -un 2>/dev/null || whoami 2>/dev/null)"
    fi
}

# @since 2.12
_comp_compgen_allowed_groups()
{
    if _comp_as_root; then
        _comp_compgen -- -g
    else
        _comp_compgen_split -- "$(id -Gn 2>/dev/null || groups 2>/dev/null)"
    fi
}

# @since 2.12
_comp_compgen_selinux_users()
{
    _comp_compgen_split -- "$(semanage user -nl 2>/dev/null |
        _comp_awk '{ print $1 }')"
}

# This function completes on valid shells
# @param $1 chroot to search from
#
# @since 2.12
_comp_compgen_shells()
{
    local -a shells=()
    local _shell _rest
    while read -r _shell _rest; do
        [[ $_shell == /* ]] && shells+=("$_shell")
    done 2>/dev/null <"${1-}"/etc/shells
    _comp_compgen -U shells -- -W '"${shells[@]}"'
}

# This function completes on valid filesystem types
#
# @since 2.12
_comp_compgen_fstypes()
{
    local _fss

    if [[ -e /proc/filesystems ]]; then
        # Linux
        _fss="$(cut -d$'\t' -f2 /proc/filesystems)
             $(_comp_awk '! /\*/ { print $NF }' /etc/filesystems 2>/dev/null)"
    else
        # Generic
        _fss="$(_comp_awk '/^[ \t]*[^#]/ { print $3 }' /etc/fstab 2>/dev/null)
             $(_comp_awk '/^[ \t]*[^#]/ { print $3 }' /etc/mnttab 2>/dev/null)
             $(_comp_awk '/^[ \t]*[^#]/ { print $4 }' /etc/vfstab 2>/dev/null)
             $(_comp_awk '{ print $1 }' /etc/dfs/fstypes 2>/dev/null)
             $(lsvfs 2>/dev/null | _comp_awk '$1 !~ /^(Filesystem|[^a-zA-Z])/ { print $1 }')
             $([[ -d /etc/fs ]] && command ls /etc/fs)"
    fi

    [[ $_fss ]] && _comp_compgen_split -- "$_fss"
}

# Get absolute path to a file, with rudimentary canonicalization.
# No symlink resolution or existence checks are done;
# see `_comp_realcommand` for those.
# @param $1 The file
# @var[out] REPLY The path
# @since 2.12
_comp_abspath()
{
    REPLY=$1
    [[ $REPLY == /* ]] || REPLY=$PWD/$REPLY
    REPLY=${REPLY//+(\/)/\/}
    while true; do
        # Process "." and "..".  To avoid reducing "/../../ => /", we convert
        # "/*/../" one by one. "/.."  at the beginning is ignored. Then, /*/../
        # in the middle is processed.  Finally, /*/.. at the end is removed.
        case $REPLY in
            */./*) REPLY=${REPLY//\/.\//\/} ;;
            */.) REPLY=${REPLY%/.} ;;
            /..?(/*)) REPLY=${REPLY#/..} ;;
            */+([^/])/../*) REPLY=${REPLY/\/+([^\/])\/..\//\/} ;;
            */+([^/])/..) REPLY=${REPLY%/+([^/])/..} ;;
            *) break ;;
        esac
    done
    [[ $REPLY ]] || REPLY=/
}

# Get real command.
# Command is the filename of command in PATH with possible symlinks resolved
# (if resolve tooling available), empty string if command not found.
# @param    $1 Command
# @var[out] REPLY Resulting string
# @return   True (0) if command found, False (> 0) if not.
# @since 2.12
_comp_realcommand()
{
    REPLY=""
    local file
    file=$(type -P -- "$1") || return $?
    if type -p realpath >/dev/null; then
        REPLY=$(realpath "$file")
    elif type -p greadlink >/dev/null; then
        REPLY=$(greadlink -f "$file")
    elif type -p readlink >/dev/null; then
        REPLY=$(readlink -f "$file")
    else
        _comp_abspath "$file"
    fi
}

# This function returns the position of the first argument, excluding options
#
# Options:
#     -a GLOB  Pattern of options that take an option argument
#
# @var[out] REPLY Position of the first argument before the current one being
# completed if any, or otherwise an empty string
# @return True (0) if any argument is found, False (> 0) otherwise.
# @since 2.12
_comp_locate_first_arg()
{
    local has_optarg=""
    local OPTIND=1 OPTARG="" OPTERR=0 _opt
    while getopts ':a:' _opt "$@"; do
        case $_opt in
            a) has_optarg=$OPTARG ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 2
                ;;
        esac
    done
    shift "$((OPTIND - 1))"

    local i
    REPLY=
    for ((i = 1; i < cword; i++)); do
        # shellcheck disable=SC2053
        if [[ $has_optarg && ${words[i]} == $has_optarg ]]; then
            ((i++))
        elif [[ ${words[i]} != -?* ]]; then
            REPLY=$i
            return 0
        elif [[ ${words[i]} == -- ]]; then
            ((i + 1 < cword)) && REPLY=$((i + 1)) && return 0
            break
        fi
    done
    return 1
}

# This function returns the first argument, excluding options
#
# Options:
#     -a GLOB  Pattern of options that take an option argument
#
# @var[out] REPLY First argument before the current one being completed if any,
# or otherwise an empty string
# @return True (0) if any argument is found, False (> 0) otherwise.
# @since 2.12
_comp_get_first_arg()
{
    _comp_locate_first_arg "$@" && REPLY=${words[REPLY]}
}

# This function counts the number of args, excluding options
#
# Options:
#     -n CHARS  Characters out of $COMP_WORDBREAKS which should
#               NOT be considered word breaks. See
#               _comp__reassemble_words.
#     -a GLOB   Options whose following argument should not be counted
#     -i GLOB   Options that should be counted as args
#
# @var[out] REPLY    Return the number of arguments
# @since 2.12
_comp_count_args()
{
    local has_optarg="" has_exclude="" exclude="" glob_include=""
    local OPTIND=1 OPTARG="" OPTERR=0 _opt
    while getopts ':a:n:i:' _opt "$@"; do
        case $_opt in
            a) has_optarg=$OPTARG ;;
            n) has_exclude=set exclude+=$OPTARG ;;
            i) glob_include=$OPTARG ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 2
                ;;
        esac
    done
    shift "$((OPTIND - 1))"

    if [[ $has_exclude ]]; then
        local cword words
        _comp__reassemble_words "$exclude<>&" words cword
    fi

    local i
    REPLY=1
    for ((i = 1; i < cword; i++)); do
        # shellcheck disable=SC2053
        if [[ $has_optarg && ${words[i]} == $has_optarg ]]; then
            ((i++))
        elif [[ ${words[i]} != -?* || $glob_include && ${words[i]} == $glob_include ]]; then
            ((REPLY++))
        elif [[ ${words[i]} == -- ]]; then
            ((REPLY += cword - i - 1))
            break
        fi
    done
}

# This function completes on PCI IDs
#
# @since 2.12
_comp_compgen_pci_ids()
{
    _comp_compgen_split -- "$(PATH="$PATH:/sbin" lspci -n | _comp_awk '{print $3}')"
}

# This function completes on USB IDs
#
# @since 2.12
_comp_compgen_usb_ids()
{
    _comp_compgen_split -- "$(PATH="$PATH:/sbin" lsusb | _comp_awk '{print $6}')"
}

# CD device names
#
# @since 2.12
_comp_compgen_cd_devices()
{
    _comp_compgen -c "${cur:-/dev/}" -- -f -d -X "!*/?([amrs])cd!(c-*)"
}

# DVD device names
#
# @since 2.12
_comp_compgen_dvd_devices()
{
    _comp_compgen -c "${cur:-/dev/}" -- -f -d -X "!*/?(r)dvd*"
}

# TERM environment variable values
#
# @since 2.12
_comp_compgen_terms()
{
    _comp_compgen_split -- "$({
        command sed -ne 's/^\([^[:space:]#|]\{2,\}\)|.*/\1/p' /etc/termcap
        {
            toe -a || toe
        } | _comp_awk '{ print $1 }'
        _comp_expand_glob dirs '/{etc,lib,usr/lib,usr/share}/terminfo/?' &&
            find "${dirs[@]}" -type f -maxdepth 1 |
            _comp_awk -F / '{ print $NF }'
    } 2>/dev/null)"
}

# @since 2.12
_comp_try_faketty()
{
    if type unbuffer &>/dev/null; then
        unbuffer -p "$@"
    elif script --version 2>&1 | command grep -qF util-linux; then
        # BSD and Solaris "script" do not seem to have required features
        script -qaefc "$*" /dev/null
    else
        "$@" # no can do, fallback
    fi
}

# a little help for FreeBSD ports users
[[ $OSTYPE == *freebsd* ]] && complete -W 'index search fetch fetch-list
    extract patch configure build install reinstall deinstall clean
    clean-depends kernel buildworld' make

# This function provides simple user@host completion
#
# @since 2.12
_comp_complete_user_at_host()
{
    local cur prev words cword comp_args
    _comp_initialize -n : -- "$@" || return

    if [[ $cur == *@* ]]; then
        _comp_compgen_known_hosts "$cur"
    else
        _comp_compgen -- -u -S @
        compopt -o nospace
    fi
}
shopt -u hostcomplete && complete -F _comp_complete_user_at_host talk ytalk finger

# NOTE: Using this function as a helper function is deprecated.  Use
#       `_comp_compgen_known_hosts' instead.
# @since 2.12
_comp_complete_known_hosts()
{
    local cur prev words cword comp_args
    _comp_initialize -n : -- "$@" || return

    # NOTE: Using `_known_hosts' (the old name of `_comp_complete_known_hosts')
    #       as a helper function and passing options to `_known_hosts' is
    #       deprecated: Use `_comp_compgen_known_hosts' instead.
    local -a options=()
    [[ ${1-} == -a || ${2-} == -a ]] && options+=(-a)
    [[ ${1-} == -c || ${2-} == -c ]] && options+=(-c)
    local IFS=$' \t\n' # Workaround for connected ${v+"$@"} in bash < 4.4
    _comp_compgen_known_hosts ${options[@]+"${options[@]}"} -- "$cur"
}

# Helper function to locate ssh included files in configs
# This function looks for the "Include" keyword in ssh config files and
# includes them recursively, adding each result to the config variable.
_comp__included_ssh_config_files()
{
    (($# < 1)) &&
        echo "bash_completion: $FUNCNAME: missing mandatory argument CONFIG" >&2
    local configfile i files f REPLY
    configfile=$1

    # From man ssh_config:
    # "Files without absolute paths are assumed to be in ~/.ssh if included
    # in a user configuration file or /etc/ssh if included from the system
    # configuration file."
    # This behavior is not affected by the the including file location -
    # if the system configuration file is included from the user's config,
    # relative includes are still resolved in the user's ssh config directory.
    local relative_include_base
    if [[ $configfile == /etc/ssh* ]]; then
        relative_include_base="/etc/ssh"
    else
        relative_include_base="$HOME/.ssh"
    fi

    local depth=1
    local -a included
    local -a include_files
    included=("$configfile")

    # Max recursion depth per openssh's READCONF_MAX_DEPTH:
    # https://github.com/openssh/openssh-portable/blob/5ec5504f1d328d5bfa64280cd617c3efec4f78f3/readconf.c#L2240
    local max_depth=16
    while ((${#included[@]} > 0 && depth++ < max_depth)); do
        _comp_split include_files "$(command sed -ne 's/^[[:blank:]]*[Ii][Nn][Cc][Ll][Uu][Dd][Ee][[:blank:]]\(.*\)$/\1/p' "${included[@]}")" || return
        included=()
        for i in "${include_files[@]}"; do
            if [[ $i != [~/]* ]]; then
                i="${relative_include_base}/${i}"
            fi
            _comp_expand_tilde "$i"
            if _comp_expand_glob files '$REPLY'; then
                # In case the expanded variable contains multiple paths
                for f in "${files[@]}"; do
                    if [[ -r $f && ! -d $f ]]; then
                        config+=("$f")
                        included+=("$f")
                    fi
                done
            fi
        done
    done
}

# Helper function for completing _comp_complete_known_hosts.
# This function performs host completion based on ssh's config and known_hosts
# files, as well as hostnames reported by avahi-browse if
# BASH_COMPLETION_KNOWN_HOSTS_WITH_AVAHI is set to a non-empty value.
# Also hosts from HOSTFILE (compgen -A hostname) are added, unless
# BASH_COMPLETION_KNOWN_HOSTS_WITH_HOSTFILE is set to an empty value.
# Usage: _comp_compgen_known_hosts [OPTIONS] CWORD
# Options:
#     -a             Use aliases from ssh config files
#     -c             Use `:' suffix
#     -F configfile  Use `configfile' for configuration settings
#     -p PREFIX      Use PREFIX
#     -4             Filter IPv6 addresses from results
#     -6             Filter IPv4 addresses from results
# @var[out] COMPREPLY  Completions, starting with CWORD, are added
# @return True (0) if one or more completions are generated, or otherwise False
# (1).
# @since 2.12
_comp_compgen_known_hosts()
{
    local known_hosts
    _comp_compgen_known_hosts__impl "$@" || return "$?"
    _comp_compgen -U known_hosts set "${known_hosts[@]}"
}
_comp_compgen_known_hosts__impl()
{
    known_hosts=()

    local configfile="" flag prefix=""
    local cur suffix="" aliases="" i host ipv4="" ipv6=""
    local -a kh tmpkh=() khd=() config=()

    # TODO remove trailing %foo from entries

    local OPTIND=1
    while getopts "ac46F:p:" flag "$@"; do
        case $flag in
            a) aliases=set ;;
            c) suffix=':' ;;
            F)
                if [[ ! $OPTARG ]]; then
                    echo "bash_completion: $FUNCNAME: -F: an empty filename is specified" >&2
                    return 2
                fi
                configfile=$OPTARG
                ;;
            p) prefix=$OPTARG ;;
            4) ipv4=set ;;
            6) ipv6=set ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 2
                ;;
        esac
    done
    if (($# < OPTIND)); then
        echo "bash_completion: $FUNCNAME: missing mandatory argument CWORD" >&2
        return 2
    fi
    cur=${!OPTIND}
    ((OPTIND += 1))
    if (($# >= OPTIND)); then
        echo "bash_completion: $FUNCNAME($*): unprocessed arguments:" \
            "$(while (($# >= OPTIND)); do
                printf '%s ' ${!OPTIND}
                shift
            done)" >&2
        return 2
    fi

    [[ $cur == *@* ]] && prefix=$prefix${cur%@*}@ && cur=${cur#*@}
    kh=()

    # ssh config files
    if [[ $configfile ]]; then
        [[ -r $configfile && ! -d $configfile ]] && config+=("$configfile")
    else
        for i in /etc/ssh/ssh_config ~/.ssh/config ~/.ssh2/config; do
            [[ -r $i && ! -d $i ]] && config+=("$i")
        done
    fi

    # "Include" keyword in ssh config files
    if ((${#config[@]} > 0)); then
        for i in "${config[@]}"; do
            _comp__included_ssh_config_files "$i"
        done
    fi

    # Known hosts files from configs
    if ((${#config[@]} > 0)); then
        # expand paths (if present) to global and user known hosts files
        # TODO(?): try to make known hosts files with more than one consecutive
        #          spaces in their name work (watch out for ~ expansion
        #          breakage! Alioth#311595)
        if _comp_split -l tmpkh "$(_comp_awk 'sub("^[ \t]*([Gg][Ll][Oo][Bb][Aa][Ll]|[Uu][Ss][Ee][Rr])[Kk][Nn][Oo][Ww][Nn][Hh][Oo][Ss][Tt][Ss][Ff][Ii][Ll][Ee][ \t=]+", "") { print $0 }' "${config[@]}" | sort -u)"; then
            local tmpkh2 j REPLY
            for i in "${tmpkh[@]}"; do
                # First deal with quoted entries...
                while [[ $i =~ ^([^\"]*)\"([^\"]*)\"(.*)$ ]]; do
                    i=${BASH_REMATCH[1]}${BASH_REMATCH[3]}
                    _comp_expand_tilde "${BASH_REMATCH[2]}" # Eval/expand possible `~' or `~user'
                    [[ -r $REPLY ]] && kh+=("$REPLY")
                done
                # ...and then the rest.
                _comp_split tmpkh2 "$i" || continue
                for j in "${tmpkh2[@]}"; do
                    _comp_expand_tilde "$j" # Eval/expand possible `~' or `~user'
                    [[ -r $REPLY ]] && kh+=("$REPLY")
                done
            done
        fi
    fi

    if [[ ! $configfile ]]; then
        # Global and user known_hosts files
        for i in /etc/ssh/ssh_known_hosts /etc/ssh/ssh_known_hosts2 \
            /etc/known_hosts /etc/known_hosts2 ~/.ssh/known_hosts \
            ~/.ssh/known_hosts2; do
            [[ -r $i && ! -d $i ]] && kh+=("$i")
        done
        for i in /etc/ssh2/knownhosts ~/.ssh2/hostkeys; do
            [[ -d $i ]] || continue
            _comp_expand_glob tmpkh '"$i"/*.pub' && khd+=("${tmpkh[@]}")
        done
    fi

    # If we have known_hosts files to use
    if ((${#kh[@]} + ${#khd[@]} > 0)); then
        if ((${#kh[@]} > 0)); then
            # https://man.openbsd.org/sshd.8#SSH_KNOWN_HOSTS_FILE_FORMAT
            for i in "${kh[@]}"; do
                while read -ra tmpkh; do
                    ((${#tmpkh[@]} == 0)) && continue
                    # Skip entries starting with | (hashed) and # (comment)
                    [[ ${tmpkh[0]} == [\|\#]* ]] && continue
                    # Ignore leading @foo (markers)
                    local host_list=${tmpkh[0]}
                    [[ ${tmpkh[0]} == @* ]] && host_list=${tmpkh[1]-}
                    # Split entry on commas
                    local -a hosts
                    if _comp_split -F , hosts "$host_list"; then
                        for host in "${hosts[@]}"; do
                            # Skip hosts containing wildcards
                            [[ $host == *[*?]* ]] && continue
                            # Remove leading [
                            host=${host#[}
                            # Remove trailing ] + optional :port
                            host=${host%]?(:+([0-9]))}
                            # Add host to candidates
                            [[ $host ]] && known_hosts+=("$host")
                        done
                    fi
                done <"$i"
            done
        fi
        if ((${#khd[@]} > 0)); then
            # Needs to look for files called
            # .../.ssh2/key_22_<hostname>.pub
            # dont fork any processes, because in a cluster environment,
            # there can be hundreds of hostkeys
            for i in "${khd[@]}"; do
                if [[ $i == *key_22_*.pub && -r $i ]]; then
                    host=${i/#*key_22_/}
                    host=${host/%.pub/}
                    [[ $host ]] && known_hosts+=("$host")
                fi
            done
        fi

        # apply suffix and prefix
        ((${#known_hosts[@]})) &&
            _comp_compgen -v known_hosts -- -W '"${known_hosts[@]}"' -P "$prefix" -S "$suffix"
    fi

    # append any available aliases from ssh config files
    if [[ ${#config[@]} -gt 0 && $aliases ]]; then
        local -a hosts
        if _comp_split hosts "$(command sed -ne 's/^[[:blank:]]*[Hh][Oo][Ss][Tt][[:blank:]=]\{1,\}\(.*\)$/\1/p' "${config[@]}")"; then
            _comp_compgen -av known_hosts -- -P "$prefix" \
                -S "$suffix" -W '"${hosts[@]%%[*?%]*}"' -X '@(\!*|)'
        fi
    fi

    # Add hosts reported by avahi-browse, if desired and it's available.
    if [[ ${BASH_COMPLETION_KNOWN_HOSTS_WITH_AVAHI-} ]] &&
        type avahi-browse &>/dev/null; then
        # Some old versions of avahi-browse reportedly didn't have -k
        # (even if mentioned in the manpage); those we do not support any more.
        local generated=$(avahi-browse -cprak 2>/dev/null | _comp_awk -F ';' \
            '/^=/ && $5 ~ /^_(ssh|workstation)\._tcp$/ { print $7 }' |
            sort -u)
        _comp_compgen -av known_hosts -- -P "$prefix" -S "$suffix" -W '$generated'
    fi

    # Add hosts reported by ruptime.
    if type ruptime &>/dev/null; then
        local generated=$(ruptime 2>/dev/null | _comp_awk '!/^ruptime:/ { print $1 }')
        _comp_compgen -av known_hosts -- -W '$generated'
    fi

    # Add results of normal hostname completion, unless
    # `BASH_COMPLETION_KNOWN_HOSTS_WITH_HOSTFILE' is set to an empty value.
    if [[ ${BASH_COMPLETION_KNOWN_HOSTS_WITH_HOSTFILE-set} ]]; then
        _comp_compgen -av known_hosts -- -A hostname -P "$prefix" -S "$suffix"
    fi

    ((${#known_hosts[@]})) || return 1

    if [[ $ipv4 ]]; then
        known_hosts=("${known_hosts[@]/*:*$suffix/}")
    fi
    if [[ $ipv6 ]]; then
        known_hosts=("${known_hosts[@]/+([0-9]).+([0-9]).+([0-9]).+([0-9])$suffix/}")
    fi
    if [[ $ipv4 || $ipv6 ]]; then
        for i in "${!known_hosts[@]}"; do
            [[ ${known_hosts[i]} ]] || unset -v 'known_hosts[i]'
        done
    fi
    ((${#known_hosts[@]})) || return 1

    _comp_compgen -v known_hosts -c "$prefix$cur" ltrim_colon "${known_hosts[@]}"
}
complete -F _comp_complete_known_hosts traceroute traceroute6 \
    fping fping6 telnet rsh rlogin ftp dig drill ssh-installkeys showmount

# Convert the word index in `words` to the index in `COMP_WORDS`.
# @param $1           Index in the array WORDS.
# @var[in,opt] words  Words that contain reassmbled words.
# @var[in,opt] cword  Current word index in WORDS.
#                     WORDS and CWORD, if any, are expected to be created by
#                     _comp__reassemble_words.
#
_comp__find_original_word()
{
    REPLY=$1

    # If CWORD or WORDS are undefined, we return the first argument without any
    # processing.
    [[ -v cword && -v words ]] || return 0

    local reassembled_offset=$1 i=0 j
    for ((j = 0; j < reassembled_offset; j++)); do
        local word=${words[j]}
        while [[ $word && i -lt ${#COMP_WORDS[@]} && $word == *"${COMP_WORDS[i]}"* ]]; do
            word=${word#*"${COMP_WORDS[i++]}"}
        done
    done
    REPLY=$i
}
# A meta-command completion function for commands like sudo(8), which need to
# first complete on a command, then complete according to that command's own
# completion definition.
#
# @since 2.12
_comp_command_offset()
{
    # rewrite current completion context before invoking
    # actual command completion

    # obtain the word index in COMP_WORDS
    local REPLY
    _comp__find_original_word "$1"
    local word_offset=$REPLY

    # make changes to COMP_* local.  Note that bash-4.3..5.0 have a
    # bug that `local -a arr=("${arr[@]}")` fails.  We instead first
    # assign the values of `COMP_WORDS` to another array `comp_words`.
    local COMP_LINE=$COMP_LINE COMP_POINT=$COMP_POINT COMP_CWORD=$COMP_CWORD
    local -a comp_words=("${COMP_WORDS[@]}")
    local -a COMP_WORDS=("${comp_words[@]}")

    # find new first word position, then
    # rewrite COMP_LINE and adjust COMP_POINT
    local i tail
    for ((i = 0; i < word_offset; i++)); do
        tail=${COMP_LINE#*"${COMP_WORDS[i]}"}
        ((COMP_POINT -= ${#COMP_LINE} - ${#tail}))
        COMP_LINE=$tail
    done

    # shift COMP_WORDS elements and adjust COMP_CWORD
    COMP_WORDS=("${COMP_WORDS[@]:word_offset}")
    ((COMP_CWORD -= word_offset))

    COMPREPLY=()
    local cur
    _comp_get_words cur

    if ((COMP_CWORD == 0)); then
        _comp_compgen_commands
    else
        _comp_dequote "${COMP_WORDS[0]}" || REPLY=${COMP_WORDS[0]}
        local cmd=$REPLY compcmd=$REPLY
        local cspec=$(complete -p -- "$cmd" 2>/dev/null)

        # If we have no completion for $cmd yet, see if we have for basename
        if [[ ! $cspec && $cmd == */* ]]; then
            cspec=$(complete -p -- "${cmd##*/}" 2>/dev/null)
            [[ $cspec ]] && compcmd=${cmd##*/}
        fi
        # If still nothing, just load it for the basename
        if [[ ! $cspec ]]; then
            compcmd=${cmd##*/}
            _comp_load -D -- "$compcmd"
            cspec=$(complete -p -- "$compcmd" 2>/dev/null)
        fi

        local retry_count=0
        while true; do # loop for the retry request by status 124
            local args original_cur=${comp_args[1]-$cur}
            if ((${#COMP_WORDS[@]} >= 2)); then
                args=("$cmd" "$original_cur" "${COMP_WORDS[-2]}")
            else
                args=("$cmd" "$original_cur")
            fi

            if [[ ! $cspec ]]; then
                if ((${#COMPREPLY[@]} == 0)); then
                    # XXX will probably never happen as long as completion loader loads
                    #     *something* for every command thrown at it ($cspec != empty)
                    _comp_complete_minimal "${args[@]}"
                fi
            elif [[ $cspec == *\ -[CF]\ * ]]; then
                if [[ $cspec == *' -F '* ]]; then
                    # complete -F <function>

                    # get function name
                    local func=${cspec#* -F }
                    func=${func%% *}
                    $func "${args[@]}"

                    # restart completion (once) if function exited with 124
                    if (($? == 124 && retry_count++ == 0)); then
                        # Note: When the completion function returns 124, the
                        # state of COMPREPLY is discarded.
                        COMPREPLY=()

                        cspec=$(complete -p -- "$compcmd" 2>/dev/null)

                        # Note: When completion spec is removed after 124, we
                        # do not generate any completions including the default
                        # ones. This is the behavior of the original Bash
                        # progcomp.
                        [[ $cspec ]] || break

                        continue
                    fi
                else
                    # complete -C <command>

                    # get command name
                    local completer=${cspec#* -C \'}

                    # completer commands are always single-quoted
                    if ! _comp_dequote "'$completer"; then
                        _minimal "${args[@]}"
                        break
                    fi
                    completer=${REPLY[0]}

                    local -a suggestions

                    local IFS=$' \t\n'
                    local reset_monitor=$(shopt -po monitor) reset_lastpipe=$(shopt -p lastpipe) reset_noglob=$(shopt -po noglob)
                    set +o monitor
                    shopt -s lastpipe
                    set -o noglob

                    COMP_KEY="$COMP_KEY" COMP_LINE="$COMP_LINE" \
                        COMP_POINT="$COMP_POINT" COMP_TYPE="$COMP_TYPE" \
                        $completer "${args[@]}" | mapfile -t suggestions

                    $reset_monitor
                    $reset_lastpipe
                    $reset_noglob
                    _comp_unlocal IFS

                    local suggestion
                    local i=0
                    COMPREPLY=()
                    for suggestion in "${suggestions[@]}"; do
                        COMPREPLY[i]+=${COMPREPLY[i]+$'\n'}$suggestion

                        if [[ $suggestion != *\\ ]]; then
                            ((i++))
                        fi
                    done
                fi

                # restore initial compopts
                local opt
                while [[ $cspec == *" -o "* ]]; do
                    # FIXME: should we take "+o opt" into account?
                    cspec=${cspec#*-o }
                    opt=${cspec%% *}
                    compopt -o "$opt"
                    cspec=${cspec#"$opt"}
                done
            else
                cspec=${cspec#complete}
                cspec=${cspec%%@("$compcmd"|"'${compcmd//\'/\'\\\'\'}'")}
                eval "_comp_compgen -- $cspec"
            fi
            break
        done
    fi
}

# A _comp_command_offset wrapper function for use when the offset is unknown.
# Only intended to be used as a completion function directly associated
# with a command, not to be invoked from within other completion functions.
#
# @since 2.12
_comp_command()
{
    # We unset the shell variable `words` locally to tell
    # `_comp_command_offset` that the index is intended to be that in
    # `COMP_WORDS` instead of `words`.
    local words
    unset -v words

    local offset i

    # find actual offset, as position of the first non-option
    offset=1
    for ((i = 1; i <= COMP_CWORD; i++)); do
        if [[ ${COMP_WORDS[i]} != -* ]]; then
            offset=$i
            break
        fi
    done
    _comp_command_offset $offset
}
complete -F _comp_command aoss command "do" else eval exec ltrace nice nohup padsp \
    "then" time tsocks vsound xargs

# @since 2.12
_comp_root_command()
{
    local PATH=$PATH:/sbin:/usr/sbin:/usr/local/sbin
    local _comp_root_command=$1
    _comp_command
}
complete -F _comp_root_command fakeroot gksu gksudo kdesudo really

# Return true if the completion should be treated as running as root
#
# @since 2.12
_comp_as_root()
{
    [[ $EUID -eq 0 || ${_comp_root_command-} ]]
}

# Complete on available commands, subject to `no_empty_cmd_completion`.
# @return True (0) if one or more completions are generated, or otherwise False
# (1).  Note that it returns 1 even when the completion generation is canceled
# by `shopt -s no_empty_cmd_completion`.
#
# @since 2.12
_comp_compgen_commands()
{
    [[ ! ${cur-} ]] && shopt -q no_empty_cmd_completion && return 1
    # -o filenames for e.g. spaces in paths to and in command names
    _comp_compgen -- -c -o plusdirs && compopt -o filenames
}

# @since 2.12
_comp_complete_longopt()
{
    local cur prev words cword was_split comp_args
    _comp_initialize -s -- "$@" || return

    case "${prev,,}" in
        --help | --usage | --version)
            return
            ;;
        --!(no-*)dir*)
            _comp_compgen -a filedir -d
            return
            ;;
        --!(no-*)@(file|path)*)
            _comp_compgen -a filedir
            return
            ;;
        --+([-a-z0-9_]))
            local argtype=$(LC_ALL=C $1 --help 2>&1 | command sed -ne \
                "s|.*$prev\[\{0,1\}=[<[]\{0,1\}\([-A-Za-z0-9_]\{1,\}\).*|\1|p")
            case ${argtype,,} in
                *dir*)
                    _comp_compgen -a filedir -d
                    return
                    ;;
                *file* | *path*)
                    _comp_compgen -a filedir
                    return
                    ;;
            esac
            ;;
    esac

    [[ $was_split ]] && return

    if [[ $cur == -* ]]; then
        _comp_compgen_split -- "$(LC_ALL=C $1 --help 2>&1 |
            while read -r line; do
                [[ $line =~ --[A-Za-z0-9]+([-_][A-Za-z0-9]+)*=? ]] &&
                    printf '%s\n' "${BASH_REMATCH[0]}"
            done)"
        [[ ${COMPREPLY-} == *= ]] && compopt -o nospace
    elif [[ $1 == *@(rmdir|chroot) ]]; then
        _comp_compgen -a filedir -d
    else
        [[ $1 == *mkdir ]] && compopt -o nospace
        _comp_compgen -a filedir
    fi
}
# makeinfo and texi2dvi are defined elsewhere.
complete -F _comp_complete_longopt \
    a2ps awk base64 bash bc bison cat chroot colordiff cp \
    csplit cut date df diff dir du enscript expand fmt fold gperf \
    grep grub head irb ld ldd less ln ls m4 mkdir mkfifo mknod \
    mv netstat nl nm objcopy objdump od paste pr ptx readelf rm rmdir \
    sed seq shar sort split strip sum tac tail tee \
    texindex touch tr uname unexpand uniq units vdir wc who

# @since 2.12
declare -Ag _comp_xspecs

# @since 2.12
_comp_complete_filedir_xspec()
{
    local cur prev words cword comp_args
    _comp_initialize -- "$@" || return
    _comp_compgen_filedir_xspec "$1"
}

# @since 2.12
_comp_compgen_filedir_xspec()
{
    _comp_compgen_tilde && return

    local REPLY
    _comp_quote_compgen "$cur"
    local quoted=$REPLY

    local xspec=${_comp_xspecs[${1##*/}]-${_xspecs[${1##*/}]-}}
    local -a toks
    _comp_compgen -v toks -c "$quoted" -- -d

    # Munge xspec to contain uppercase version too
    # https://lists.gnu.org/archive/html/bug-bash/2010-09/msg00036.html
    # news://news.gmane.io/4C940E1C.1010304@case.edu
    eval xspec="${xspec}"
    local matchop=!
    if [[ $xspec == !* ]]; then
        xspec=${xspec#!}
        matchop=@
    fi
    xspec="$matchop($xspec|${xspec^^})"

    _comp_compgen -av toks -c "$quoted" -- -f -X "@(|!($xspec))"

    # Try without filter if it failed to produce anything and configured to
    [[ ${BASH_COMPLETION_FILEDIR_FALLBACK-} && ${#toks[@]} -lt 1 ]] &&
        _comp_compgen -av toks -c "$quoted" -- -f

    ((${#toks[@]})) || return 1

    # Remove . and .. (as well as */. and */..) from suggestions, unless .. or
    # */.. was typed explicitly by the user (for users who use tab-completion
    # to append a slash after '..')
    if [[ $cur != ?(*/).. ]]; then
        _comp_compgen -Rv toks -- -X '?(*/)@(.|..)' -W '"${toks[@]}"' || return 1
    fi

    compopt -o filenames
    _comp_compgen -RU toks -- -W '"${toks[@]}"'
}

_comp__init_install_xspec()
{
    local xspec=$1 cmd
    shift
    for cmd in "$@"; do
        _comp_xspecs[$cmd]=$xspec
    done
}
# bzcmp, bzdiff, bz*grep, bzless, bzmore intentionally not here, see Debian: #455510
_comp__init_install_xspec '!*.?(t)bz?(2)' bunzip2 bzcat pbunzip2 pbzcat lbunzip2 lbzcat
_comp__init_install_xspec '!*.@(zip|[aegjkswx]ar|exe|pk3|wsz|zargo|xpi|s[tx][cdiw]|sx[gm]|o[dt][tspgfc]|od[bm]|oxt|?(o)xps|epub|cbz|apk|aab|ipa|do[ct][xm]|p[op]t[mx]|xl[st][xm]|pyz|vsix|whl|[Ff][Cc][Ss]td)' unzip zipinfo
_comp__init_install_xspec '*.Z' compress znew
# zcmp, zdiff, z*grep, zless, zmore intentionally not here, see Debian: #455510
_comp__init_install_xspec '!*.@(Z|[gGd]z|t[ag]z)' gunzip zcat
_comp__init_install_xspec '!*.@(Z|[gGdz]z|t[ag]z)' unpigz
_comp__init_install_xspec '!*.Z' uncompress
# lzcmp, lzdiff intentionally not here, see Debian: #455510
_comp__init_install_xspec '!*.@(tlz|lzma)' lzcat lzegrep lzfgrep lzgrep lzless lzmore unlzma
_comp__init_install_xspec '!*.@(?(t)xz|tlz|lzma)' unxz xzcat
_comp__init_install_xspec '!*.lrz' lrunzip
_comp__init_install_xspec '!*.@(gif|jp?(e)g|miff|tif?(f)|pn[gm]|p[bgp]m|bmp|xpm|ico|xwd|tga|pcx)' ee
_comp__init_install_xspec '!*.@(gif|jp?(e)g|tif?(f)|png|p[bgp]m|bmp|x[bp]m|rle|rgb|pcx|fits|pm|svg)' qiv
_comp__init_install_xspec '!*.@(gif|jp?(e)g?(2)|j2[ck]|jp[2f]|tif?(f)|png|p[bgpn]m|webp|bmp|x[bp]m|rle|rgb|pcx|fits|pm|?(e)ps)' xv
_comp__init_install_xspec '!*.@(@(?(e)ps|?(E)PS|pdf|PDF)?(.gz|.GZ|.bz2|.BZ2|.Z))' gv ggv kghostview
_comp__init_install_xspec '!*.@(dvi|DVI)?(.@(gz|Z|bz2))' xdvi kdvi
_comp__init_install_xspec '!*.dvi' dvips dviselect dvitype dvipdf advi dvipdfm dvipdfmx
_comp__init_install_xspec '!*.[pf]df' acroread gpdf xpdf
_comp__init_install_xspec '!*.@(?(e)ps|pdf)' kpdf
_comp__init_install_xspec '!*.@(okular|@(?(e|x)ps|?(E|X)PS|[pf]df|[PF]DF|dvi|DVI|cb[rz]|CB[RZ]|djv?(u)|DJV?(U)|dvi|DVI|gif|jp?(e)g|miff|tif?(f)|pn[gm]|p[bgp]m|bmp|xpm|ico|xwd|tga|pcx|GIF|JP?(E)G|MIFF|TIF?(F)|PN[GM]|P[BGP]M|BMP|XPM|ICO|XWD|TGA|PCX|epub|EPUB|odt|ODT|fb?(2)|FB?(2)|mobi|MOBI|g3|G3|chm|CHM|md|markdown)?(.?(gz|GZ|bz2|BZ2|xz|XZ)))' okular
_comp__init_install_xspec '!*.pdf' epdfview pdfunite
_comp__init_install_xspec '!*.@(cb[rz7t]|djv?(u)|?(e)ps|pdf)' zathura
_comp__init_install_xspec '!*.@(?(e)ps|pdf)' ps2pdf ps2pdf12 ps2pdf13 ps2pdf14 ps2pdfwr
_comp__init_install_xspec '!*.texi*' makeinfo texi2html
_comp__init_install_xspec '!*.@(?(la)tex|texi|dtx|ins|ltx|dbj)' tex latex slitex jadetex pdfjadetex pdftex pdflatex texi2dvi xetex xelatex luatex lualatex
_comp__init_install_xspec '!*.mp3' mpg123 mpg321 madplay
_comp__init_install_xspec '!*@(.@(mp?(e)g|MP?(E)G|wm[av]|WM[AV]|avi|AVI|asf|vob|VOB|bin|dat|divx|DIVX|vcd|ps|pes|fli|flv|FLV|fxm|FXM|viv|rm|ram|yuv|mov|MOV|qt|QT|web[am]|WEB[AM]|mp[234]|MP[234]|m?(p)4[av]|M?(P)4[AV]|mkv|MKV|og[agmv]|OG[AGMV]|t[ps]|T[PS]|m2t?(s)|M2T?(S)|mts|MTS|wav|WAV|flac|FLAC|asx|ASX|mng|MNG|srt|m[eo]d|M[EO]D|s[3t]m|S[3T]M|it|IT|xm|XM)|+([0-9]).@(vdr|VDR))?(.@(crdownload|part))' xine aaxine cacaxine fbxine
_comp__init_install_xspec '!*@(.@(mp?(e)g|MP?(E)G|wm[av]|WM[AV]|avi|AVI|asf|vob|VOB|bin|dat|divx|DIVX|vcd|ps|pes|fli|flv|FLV|fxm|FXM|viv|rm|ram|yuv|mov|MOV|qt|QT|web[am]|WEB[AM]|mp[234]|MP[234]|m?(p)4[av]|M?(P)4[AV]|mkv|MKV|og[agmv]|OG[AGMV]|opus|OPUS|t[ps]|T[PS]|m2t?(s)|M2T?(S)|mts|MTS|wav|WAV|flac|FLAC|asx|ASX|mng|MNG|srt|m[eo]d|M[EO]D|s[3t]m|S[3T]M|it|IT|xm|XM|iso|ISO)|+([0-9]).@(vdr|VDR))?(.@(crdownload|part))' kaffeine dragon totem
_comp__init_install_xspec '!*.@(avi|asf|wmv)' aviplay
_comp__init_install_xspec '!*.@(rm?(j)|ra?(m)|smi?(l))' realplay
_comp__init_install_xspec '!*.@(mpg|mpeg|avi|mov|qt)' xanim
_comp__init_install_xspec '!*.@(og[ag]|m3u|flac|spx)' ogg123
_comp__init_install_xspec '!*.@(mp3|ogg|pls|m3u)' gqmpeg freeamp
_comp__init_install_xspec '!*.fig' xfig
_comp__init_install_xspec '!*.@(mid?(i)|cmf)' playmidi
_comp__init_install_xspec '!*.@(mid?(i)|rmi|rcp|[gr]36|g18|mod|xm|it|x3m|s[3t]m|kar)' timidity
_comp__init_install_xspec '!*.@(669|abc|am[fs]|d[bs]m|dmf|far|it|mdl|m[eo]d|mid?(i)|mt[2m]|oct|okt?(a)|p[st]m|s[3t]m|ult|umx|wav|xm)' modplugplay modplug123
_comp__init_install_xspec '*.@([ao]|so|so.!(conf|*/*)|[rs]pm|gif|jp?(e)g|mp3|mp?(e)g|avi|asf|ogg|class)' vi vim gvim rvim view rview rgvim rgview gview emacs xemacs sxemacs kate kwrite
_comp__init_install_xspec '!*.@(zip|z|gz|tgz)' bzme
# konqueror not here on purpose, it's more than a web/html browser
_comp__init_install_xspec '!*.@(?([xX]|[sS])[hH][tT][mM]?([lL]))' netscape mozilla lynx galeon dillo elinks amaya epiphany
_comp__init_install_xspec '!*.@(sxw|stw|sxg|sgl|doc?([mx])|dot?([mx])|rtf|txt|htm|html|?(f)odt|ott|odm|pdf)' oowriter lowriter
_comp__init_install_xspec '!*.@(sxi|sti|pps?(x)|ppt?([mx])|pot?([mx])|?(f)odp|otp)' ooimpress loimpress
_comp__init_install_xspec '!*.@(sxc|stc|xls?([bmx])|xlw|xlt?([mx])|[ct]sv|?(f)ods|ots)' oocalc localc
_comp__init_install_xspec '!*.@(sxd|std|sda|sdd|?(f)odg|otg)' oodraw lodraw
_comp__init_install_xspec '!*.@(sxm|smf|mml|odf)' oomath lomath
_comp__init_install_xspec '!*.odb' oobase lobase
_comp__init_install_xspec '!*.[rs]pm' rpm2cpio
_comp__init_install_xspec '!*.aux' bibtex
_comp__init_install_xspec '!*.po' poedit gtranslator kbabel lokalize
_comp__init_install_xspec '!*.@([Pp][Rr][Gg]|[Cc][Ll][Pp])' harbour gharbour hbpp
_comp__init_install_xspec '!*.[Hh][Rr][Bb]' hbrun
_comp__init_install_xspec '!*.ly' lilypond ly2dvi
_comp__init_install_xspec '!*.@(dif?(f)|?(d)patch)?(.@([gx]z|bz2|lzma))' cdiff
_comp__init_install_xspec '!@(*.@(ks|jks|jceks|p12|pfx|bks|ubr|gkr|cer|crt|cert|p7b|pkipath|pem|p10|csr|crl)|cacerts)' portecle
_comp__init_install_xspec '!*.@(mp[234c]|og[ag]|@(fl|a)ac|m4[abp]|spx|tta|w?(a)v|wma|aif?(f)|asf|ape)' kid3 kid3-qt
unset -f _comp__init_install_xspec

# Minimal completion to use as fallback in _comp_complete_load.
# TODO:API: rename per conventions
_comp_complete_minimal()
{
    local cur prev words cword comp_args
    _comp_initialize -- "$@" || return
    compopt -o bashdefault -o default
}
# Complete the empty string to allow completion of '>', '>>', and '<' on < 4.3
# https://lists.gnu.org/archive/html/bug-bash/2012-01/msg00045.html
complete -F _comp_complete_minimal ''

# Initialize the variable "_comp__base_directory"
# @var[out] _comp__base_directory
_comp__init_base_directory()
{
    local REPLY
    _comp_abspath "${BASH_SOURCE[0]-./bash_completion}"
    _comp__base_directory=${REPLY%/*}
    [[ $_comp__base_directory ]] || _comp__base_directory=/
    unset -f "$FUNCNAME"
}
_comp__init_base_directory

# @since 2.12
_comp_load()
{
    local flag_fallback_default="" IFS=$' \t\n'
    local OPTIND=1 OPTARG="" OPTERR=0 opt
    while getopts ':D' opt "$@"; do
        case $opt in
            D) flag_fallback_default=set ;;
            *)
                echo "bash_completion: $FUNCNAME: usage error" >&2
                return 2
                ;;
        esac
    done
    shift "$((OPTIND - 1))"

    local cmd=$1 cmdname=${1##*/} dir compfile
    local -a paths
    [[ $cmdname ]] || return 1

    local backslash=
    if [[ $cmd == \\* ]]; then
        cmd=${cmd:1}
        # If we already have a completion for the "real" command, use it
        $(complete -p -- "$cmd" 2>/dev/null || echo false) "\\$cmd" && return 0
        backslash=\\
    fi

    # Resolve absolute path to $cmd
    local REPLY pathcmd origcmd=$cmd
    if pathcmd=$(type -P -- "$cmd"); then
        _comp_abspath "$pathcmd"
        cmd=$REPLY
    fi

    local -a dirs=()

    # Lookup order:
    # 1) From BASH_COMPLETION_USER_DIR (e.g. ~/.local/share/bash-completion):
    # User installed completions.
    if [[ ${BASH_COMPLETION_USER_DIR-} ]]; then
        _comp_split -F : paths "$BASH_COMPLETION_USER_DIR" &&
            dirs+=("${paths[@]/%//completions}")
    else
        dirs=("${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions")
    fi

    # 2) From the location of bash_completion: Completions relative to the main
    # script. This is primarily for run-in-place-from-git-clone setups, where
    # we want to prefer in-tree completions over ones possibly coming with a
    # system installed bash-completion. (Due to usual install layouts, this
    # often hits the correct completions in system installations, too.)
    dirs+=("$_comp__base_directory/completions")

    # 3) From bin directories extracted from the specified path to the command,
    # the real path to the command, and $PATH
    paths=()
    [[ $cmd == /* ]] && paths+=("${cmd%/*}")
    _comp_realcommand "$cmd" && paths+=("${REPLY%/*}")
    _comp_split -aF : paths "$PATH"
    for dir in "${paths[@]%/}"; do
        [[ $dir == ?*/@(bin|sbin) ]] &&
            dirs+=("${dir%/*}/share/bash-completion/completions")
    done

    # 4) From XDG_DATA_DIRS or system dirs (e.g. /usr/share, /usr/local/share):
    # Completions in the system data dirs.
    _comp_split -F : paths "${XDG_DATA_DIRS:-/usr/local/share:/usr/share}" &&
        dirs+=("${paths[@]/%//bash-completion/completions}")

    # Set up default $IFS in case loaded completions depend on it,
    # as well as for $compspec invocation below.
    local IFS=$' \t\n'

    # Look up and source
    shift
    local i prefix compspec
    for prefix in "" _; do # Regular from all dirs first, then fallbacks
        for i in ${!dirs[*]}; do
            dir=${dirs[i]}
            if [[ ! -d $dir ]]; then
                unset -v 'dirs[i]'
                continue
            fi
            for compfile in "$prefix$cmdname" "$prefix$cmdname.bash"; do
                compfile="$dir/$compfile"
                # Avoid trying to source dirs as long as we support bash < 4.3
                # to avoid an fd leak; https://bugzilla.redhat.com/903540
                if [[ -d $compfile ]]; then
                    # Do not warn with . or .. (especially the former is common)
                    [[ $compfile == */.?(.) ]] ||
                        echo "bash_completion: $compfile: is a directory" >&2
                elif [[ -e $compfile ]] && . "$compfile" "$cmd" "$@"; then
                    # At least $cmd is expected to have a completion set when
                    # we return successfully; see if it already does
                    if compspec=$(complete -p -- "$cmd" 2>/dev/null); then
                        # $cmd is the case in which we do backslash processing
                        [[ $backslash ]] && eval "$compspec \"\$backslash\$cmd\""
                        # If invoked without path, that one should be set, too
                        # ...but let's not overwrite an existing one, if any
                        [[ $origcmd != */* ]] &&
                            ! complete -p -- "$origcmd" &>/dev/null &&
                            eval "$compspec \"\$origcmd\""
                        return 0
                    fi
                    # If not, see if we got one for $cmdname
                    if [[ $cmdname != "$cmd" ]] && compspec=$(complete -p -- "$cmdname" 2>/dev/null); then
                        # Use that for $cmd too, if we have a full path to it
                        [[ $cmd == /* ]] && eval "$compspec \"\$cmd\""
                        return 0
                    fi
                    # Nothing expected was set, continue lookup
                fi
            done
        done
    done

    # Look up simple "xspec" completions
    [[ -v _comp_xspecs[$cmdname] || -v _xspecs[$cmdname] ]] &&
        complete -F _comp_complete_filedir_xspec "$cmdname" "$backslash$cmdname" && return 0

    if [[ $flag_fallback_default ]]; then
        complete -F _comp_complete_minimal -- "$origcmd" && return 0
    fi

    return 1
}

# set up dynamic completion loading
# @since 2.12
_comp_complete_load()
{
    # $1=_EmptycmD_ already for empty cmds in bash 4.3, set to it for earlier
    local cmd=${1:-_EmptycmD_}

    # Pass -D to define *something*, or otherwise there will be no completion
    # at all.
    _comp_load -D -- "$cmd" && return 124
} &&
    complete -D -F _comp_complete_load

# Function for loading and calling functions from dynamically loaded
# completion files that may not have been sourced yet.
# @param $1 completion file to load function from in case it is missing
# @param $2 the xfunc name.  When it does not start with `_',
#   `_comp_xfunc_${1//[^a-zA-Z0-9_]/_}_$2' is used for the actual name of the
#   shell function.
# @param $3... if any, specifies the arguments that are passed to the xfunc.
# @since 2.12
_comp_xfunc()
{
    local xfunc_name=$2
    [[ $xfunc_name == _* ]] ||
        xfunc_name=_comp_xfunc_${1//[^a-zA-Z0-9_]/_}_$xfunc_name
    declare -F -- "$xfunc_name" &>/dev/null || _comp_load -- "$1"
    "$xfunc_name" "${@:3}"
}

# Call a POSIX-compatible awk.  Solaris awk is not POSIX-compliant, but Solaris
# provides a POSIX-compatible version through /usr/xpg4/bin/awk.  We switch the
# implementation to /usr/xpg4/bin/awk in Solaris if any.
# @since 2.12
if [[ $OSTYPE == *solaris* && -x /usr/xpg4/bin/awk ]]; then
    _comp_awk()
    {
        /usr/xpg4/bin/awk "$@"
    }
else
    _comp_awk()
    {
        command awk "$@"
    }
fi

# List custom/extra completion files to source on the startup
## @param $1 path Path to "bash_completion"
## @var[out] _comp__init_startup_configs
_comp__init_collect_startup_configs()
{
    local base_path=${1:-${BASH_SOURCE[1]}}
    _comp__init_startup_configs=()

    # source compat completion directory definitions
    local -a compat_dirs=()
    local compat_dir
    if [[ ${BASH_COMPLETION_COMPAT_DIR-} ]]; then
        compat_dirs+=("$BASH_COMPLETION_COMPAT_DIR")
    else
        compat_dirs+=(/etc/bash_completion.d)
        # Similarly as for the "completions" dir, look up from relative to
        # bash_completion, primarily for installed-with-prefix and
        # run-in-place-from-git-clone setups.  Notably we do it after the
        # system location here, in order to prefer in-tree variables and
        # functions.
        if [[ $_comp__base_directory == */share/bash-completion ]]; then
            compat_dir=${_comp__base_directory%/share/bash-completion}/etc/bash_completion.d
        else
            compat_dir=$_comp__base_directory/bash_completion.d
        fi
        [[ ${compat_dirs[0]} == "$compat_dir" ]] ||
            compat_dirs+=("$compat_dir")
    fi
    for compat_dir in "${compat_dirs[@]}"; do
        [[ -d $compat_dir && -r $compat_dir && -x $compat_dir ]] || continue
        local compat_files
        _comp_expand_glob compat_files '"$compat_dir"/*'
        local compat_file
        for compat_file in "${compat_files[@]}"; do
            [[ ${compat_file##*/} != @($_comp_backup_glob|Makefile*|${BASH_COMPLETION_COMPAT_IGNORE-}) &&
                -f $compat_file && -r $compat_file ]] &&
                _comp__init_startup_configs+=("$compat_file")
        done
    done

    # source user completion file
    #
    # Remark: We explicitly check that $user_completion is not '/dev/null'
    #   since /dev/null may be a regular file in broken systems and can contain
    #   arbitrary garbages of suppressed command outputs.
    local user_file=${BASH_COMPLETION_USER_FILE:-~/.bash_completion}
    [[ $user_file != "$base_path" && $user_file != /dev/null && -r $user_file && -f $user_file ]] &&
        _comp__init_startup_configs+=("$user_file")

    unset -f "$FUNCNAME"
}
_comp__init_collect_startup_configs "$BASH_SOURCE"
# shellcheck disable=SC2154
for _comp_init_startup_config in "${_comp__init_startup_configs[@]}"; do
    . "$_comp_init_startup_config"
done
unset -v _comp__init_startup_configs _comp_init_startup_config
unset -f have
unset -v have

set $_comp__init_original_set_v
unset -v _comp__init_original_set_v

# ex: filetype=sh


_mise() {
    if ! command -v usage &> /dev/null; then
        echo >&2
        echo "Error: usage CLI not found. This is required for completions to work in mise." >&2
        echo "See https://usage.jdx.dev for more information." >&2
        return 1
    fi

    if [[ -z ${_usage_spec_mise_2024_12_6:-} ]]; then
        _usage_spec_mise_2024_12_6="$(mise usage)"
    fi

	local cur prev words cword was_split comp_args
    _comp_initialize -n : -- "$@" || return
    # shellcheck disable=SC2207
	_comp_compgen -- -W "$(@usage complete-word --shell bash -s "${spec_variable}" --cword="$cword" -- "${words[@]}")"
	_comp_ltrim_colon_completions "$cur"
    # shellcheck disable=SC2181
    if [[ $? -ne 0 ]]; then
        unset COMPREPLY
    fi
    return 0
}

shopt -u hostcomplete && complete -o nospace -o bashdefault -o nosort -F _mise mise
# vim: noet ci pi sts=0 sw=4 ts=4 ft=sh
