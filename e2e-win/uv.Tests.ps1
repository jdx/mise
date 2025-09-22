
Describe 'uv' {
  It 'executes from the venv Scripts directory' {
    @(
    '[env._.python]',
    'venv = {path = "my_venv", create=true}',
    '[tools]',
    'python = "3.12.3"',
    'uv = "0.5.4"',
    ) | Set-Content .mise.toml

    mise i
    mise x -- Get-Command python | Should -BeLike "$PWD\my_venv\Scripts\python"
  }
}
