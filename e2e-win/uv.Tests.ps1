
Describe 'uv' {
  BeforeEach {
    $originalPath = Get-Location
    Set-Location TestDrive:
  }

  AfterEach {
    mise trust --untrust --yes .mise.toml
    Set-Location $originalPath | Out-Null
  }

  It 'executes from the venv Scripts directory' {
    @(
    '[env._.python]',
    'venv = {path = "my_venv", create=true}',
    '[tools]',
    'python = "3.12.3"',
    'uv = "0.8.21"',
    '[settings]',
    'python.uv_venv_auto = true'
    ) | Set-Content .mise.toml
    mise trust --yes .mise.toml

    # Define the character set
    $chars = (48..57) + (65..90) + (97..122) # 0-9, A-Z, a-z

    # Generate a random string of a specific length (e.g., 10 characters)
    $randomString = -join ($chars | Get-Random -Count 10 | ForEach-Object {[char]$_})

    mise x -- uv init .
    mise x -- uv add --active --link-mode=copy cowsay

    mise i

    mise x -- cowsay -t $randomString | Out-String | Should -BeLikeExactly "*$randomString*"

    mise x -- python -c "import sys; print(sys.executable)" | Should -BeLikeExactly "*my_venv\Scripts\python.exe"
  }
}
