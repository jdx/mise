# shellcheck shell=bash
[[ -n "${ZSH_VERSION:-}" ]] ||
{
  function cd()    { __zsh_like_cd cd    "$@" ; }
  function popd()  { __zsh_like_cd popd  "$@" ; }
  function pushd() { __zsh_like_cd pushd "$@" ; }
}
