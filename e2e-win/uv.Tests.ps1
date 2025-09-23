
Describe 'uv' {
  It 'executes from the venv Scripts directory' {
    @(
    '[env._.python]',
    'venv = {path = "my_venv", create=true}',
    '[tools]',
    'python = "3.12.3"',
    'uv = "0.5.4"',
    '[settings]',
    'python.uv_venv_auto = true'
    ) | Set-Content .mise.toml

    # Simulate uv project to trigger uv_venv_auto
    $null > uv.lock

    mise i
    mise x -- python -c "import sys; print(sys.executable)" | Should -Be "$PWD\my_venv\Scripts\python.exe"

    # Define the character set
    $chars = (48..57) + (65..90) + (97..122) # 0-9, A-Z, a-z

    # Generate a random string of a specific length (e.g., 10 characters)
    $randomString = -join ($chars | Get-Random -Count 10 | ForEach-Object {[char]$_})

    "Write-Host '$randomString'" | Set-Content "$PWD\my_venv\Scripts\an-test.ps1"
    mise x -- an-test.ps1 | Should -Be "$randomString"
  }
}
