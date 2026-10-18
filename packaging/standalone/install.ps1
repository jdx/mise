# mise installer for Windows PowerShell 5.1+ and PowerShell 7+.
#
#   irm https://mise.run/powershell | iex
#
# Downloads the official release zip, verifies it against the SHASUMS256.txt
# published on GitHub Releases, installs mise.exe and mise-shim.exe side by side
# and (optionally) adds the install directory to the user Path. Safe to re-run.
#
# Environment variables:
#   MISE_VERSION                 install this release (e.g. 2026.10.13) instead of the latest
#   MISE_INSTALL_PATH            path of mise.exe (default: ~\.local\bin\mise.exe)
#   MISE_INSTALL_MODIFY_PATH     set to 0 to leave the user Path alone
#   MISE_INSTALL_SHIMS_PATH      set to 1 to also add the shims directory to the user Path
#   MISE_INSTALL_FROM_GITHUB     set to 1 to download from GitHub Releases, not mise.jdx.dev
#   MISE_INSTALL_ARCH            x64 or arm64 (default: detected)
#   MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE / MISE_MINIMUM_RELEASE_AGE
#                                skip releases newer than this when unpinned (default: 24h)
#   MISE_QUIET                   set to 1 to silence progress messages
#   MISE_DEBUG                   set to 1 for extra output

& {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  Set-StrictMode -Version 3.0

  function Test-Flag([string] $Value) { return $Value -eq '1' -or $Value -eq 'true' }
  $quiet = Test-Flag $env:MISE_QUIET
  $debugOut = Test-Flag $env:MISE_DEBUG

  function Write-Info([string] $Message) { if (-not $quiet) { [Console]::Error.WriteLine($Message) } }
  function Write-Debug2([string] $Message) { if ($debugOut) { [Console]::Error.WriteLine($Message) } }
  function Stop-Install([string] $Message) { throw "mise: $Message" }

  # Windows PowerShell 5.1 defaults to TLS 1.0/1.1 on older systems.
  try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  } catch { Write-Debug2 "could not enable TLS 1.2: $_" }

  if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
    Stop-Install 'this installer is for Windows; use https://mise.run on macOS and Linux'
  }

  function Get-Arch {
    if ($env:MISE_INSTALL_ARCH) { return $env:MISE_INSTALL_ARCH }
    $arch = $null
    try { $arch = [string][Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch { Write-Debug2 "OSArchitecture unavailable: $_" }
    if (-not $arch) {
      $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    }
    switch -Regex ($arch) {
      '^(X64|AMD64)$' { return 'x64' }
      '^ARM64$' { return 'arm64' }
      default {
        Stop-Install "unsupported architecture: $arch`nmise does not provide prebuilt Windows binaries for it. If Rust is available: cargo install --locked mise"
      }
    }
  }

  function Get-Text([string] $Url) {
    Write-Debug2 "> GET $Url"
    $content = (Invoke-WebRequest -UseBasicParsing -Uri $Url).Content
    # Binary content types (GitHub serves release assets as octet-stream) come back as bytes.
    if ($content -is [byte[]]) { return [Text.Encoding]::UTF8.GetString($content) }
    return [string]$content
  }

  # "24h" -> seconds. Mirrors install.sh: an integer followed by s, m, h, d or w.
  function Get-AgeSeconds([string] $Age) {
    if ($Age -notmatch '^([0-9]+)([smhdw])$') {
      Stop-Install "invalid minimum release age: $Age (use an integer with s, m, h, d, or w)"
    }
    $unit = @{ s = 1; m = 60; h = 3600; d = 86400; w = 604800 }[$Matches[2]]
    return [int64]$Matches[1] * $unit
  }

  function Resolve-Release {
    if ($env:MISE_VERSION) { return $env:MISE_VERSION.TrimStart('v') }
    $age = $env:MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE
    if (-not $age) { $age = $env:MISE_MINIMUM_RELEASE_AGE }
    if (-not $age) { $age = '24h' }
    $cutoff = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - (Get-AgeSeconds $age)
    $index = $null
    try { $index = Get-Text 'https://mise.jdx.dev/releases.tsv' } catch { Stop-Install "could not fetch mise release index: $_" }
    $best = $null
    foreach ($line in ($index -split "`r?`n")) {
      if (-not $line.Trim()) { continue }
      $fields = $line -split "`t"
      if ($fields.Count -ne 2 -or $fields[1] -notmatch '^[0-9]+$' -or $fields[0] -notmatch '^v([0-9]+\.[0-9]+\.[0-9]+)$') {
        Stop-Install 'invalid release index'
      }
      $ver = $Matches[1]
      if ([int64]$fields[1] -le $cutoff -and ($null -eq $best -or [version]$ver -gt [version]$best)) { $best = $ver }
    }
    if (-not $best) {
      Stop-Install 'no eligible mise release; lower MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE or set MISE_VERSION'
    }
    Write-Info "mise: selected $best (minimum release age: $age)"
    return $best
  }

  # Prints the version of an installed mise.exe, or $null.
  function Get-InstalledVersion([string] $Exe) {
    if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { return $null }
    try {
      $out = & $Exe version 2>$null | Select-Object -First 1
      if ($out) { return ([string]$out -split ' ')[0].TrimStart('v') }
    } catch { Write-Debug2 "could not run ${Exe}: $_" }
    return $null
  }

  function Get-UserPath {
    $key = Get-Item -LiteralPath 'HKCU:\Environment'
    return [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
  }

  # Prepends $Dirs to the user Path, skipping ones already present. Writes the
  # registry directly so %VARS% already in Path stay unexpanded.
  function Add-ToUserPath([string[]] $Dirs) {
    $raw = Get-UserPath
    $parts = @($raw -split ';' | Where-Object { $_ })
    $norm = @($parts | ForEach-Object { $_.TrimEnd('\') })
    $added = @($Dirs | Where-Object { $norm -notcontains $_.TrimEnd('\') })
    # Update this session first so `mise` works right away, even in a terminal
    # opened before an earlier run saved these directories to the registry.
    foreach ($d in $Dirs) { if (($env:Path -split ';') -notcontains $d) { $env:Path = "$d;$env:Path" } }
    if ($added.Count -eq 0) {
      Write-Info 'mise: user Path already contains the mise directories'
      return
    }
    Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name Path -Type ExpandString -Value (($added + $parts) -join ';')
    foreach ($d in $added) { Write-Info "mise: added to user Path: $d" }
    # Make the change visible to new processes without a logoff.
    try {
      if (-not ('MiseInstall.Native' -as [type])) {
        Add-Type -Namespace MiseInstall -Name Native -MemberDefinition '[DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'
      }
      $result = [UIntPtr]::Zero
      [void][MiseInstall.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
    } catch { Write-Debug2 "could not broadcast the environment change: $_" }
  }

  $version = Resolve-Release
  $arch = Get-Arch
  $installPath = $env:MISE_INSTALL_PATH
  if (-not $installPath) { $installPath = Join-Path $HOME '.local\bin\mise.exe' }
  $installPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($installPath)
  if (Test-Path -LiteralPath $installPath -PathType Container) {
    Stop-Install "MISE_INSTALL_PATH '$installPath' is a directory. Set it to a file path, e.g. '$(Join-Path $installPath 'mise.exe')'."
  }
  $installDir = Split-Path -Parent $installPath

  $existing = Get-InstalledVersion $installPath
  $install = $true
  if ($existing -and -not $env:MISE_VERSION) {
    $cleanExisting = $existing -replace '-DEBUG$', ''
    if ($cleanExisting -match '^[0-9]+\.[0-9]+\.[0-9]+$' -and [version]$cleanExisting -ge [version]$version) {
      Write-Info "mise: $installPath is already at $existing, keeping installed version"
      $install = $false
    }
  } elseif ($existing -eq $version) {
    Write-Info "mise: $installPath is already at version $version, skipping install"
    $install = $false
  }

  if ($install) {
    $asset = "mise-v$version-windows-$arch.zip"
    $githubBase = "https://github.com/jdx/mise/releases/download/v$version"
    $urls = @("$githubBase/$asset")
    if (-not (Test-Flag $env:MISE_INSTALL_FROM_GITHUB)) { $urls = @("https://mise.jdx.dev/v$version/$asset") + $urls }

    $work = Join-Path ([IO.Path]::GetTempPath()) ("mise-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
      Write-Info 'mise: installing mise...'
      $zip = Join-Path $work $asset
      $downloaded = $false
      foreach ($url in $urls) {
        try {
          Write-Debug2 "> download $url"
          Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
          $downloaded = $true
          break
        } catch {
          Write-Info "mise: could not download $url"
          Write-Debug2 "$_"
        }
      }
      if (-not $downloaded) { Stop-Install "failed to download $asset" }

      # The checksum always comes from GitHub Releases, never the host the
      # archive came from, so a compromised mirror cannot serve a matching pair.
      $sums = $null
      try { $sums = Get-Text "$githubBase/SHASUMS256.txt" } catch { Stop-Install "could not fetch checksums for mise $version : $_" }
      $expected = $null
      foreach ($line in ($sums -split "`r?`n")) {
        $f = $line.Trim() -split '\s+'
        if ($f.Count -eq 2 -and $f[0] -match '^[0-9a-fA-F]{64}$') {
          $name = $f[1].TrimStart('*')
          if ($name -eq $asset -or $name -eq "./$asset") { $expected = $f[0]; break }
        }
      }
      if (-not $expected) { Stop-Install "no checksum found for $asset, refusing to install it unverified" }
      $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
      if ($actual -ne $expected.ToUpperInvariant()) { Stop-Install "checksum mismatch for $asset" }
      Write-Debug2 "checksum ok: $actual"

      $extract = Join-Path $work 'x'
      Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force
      $binDir = Join-Path $extract 'mise\bin'
      $newMise = Join-Path $binDir 'mise.exe'
      if (-not (Test-Path -LiteralPath $newMise)) { Stop-Install "$asset does not contain mise\bin\mise.exe" }

      New-Item -ItemType Directory -Path $installDir -Force | Out-Null
      # mise-shim.exe must sit next to mise.exe: windows_shim_mode = "exe" copies it.
      $targets = @{ 'mise.exe' = $installPath }
      foreach ($exe in Get-ChildItem -LiteralPath $binDir -Filter '*.exe') {
        if ($exe.Name -ne 'mise.exe') { $targets[$exe.Name] = Join-Path $installDir $exe.Name }
      }
      foreach ($name in $targets.Keys) {
        $dest = $targets[$name]
        $old = $null
        if (Test-Path -LiteralPath $dest) {
          # A running exe cannot be overwritten but can be renamed. Keep it until
          # the new one is in place so a failed copy cannot leave no mise at all.
          $old = "$dest.old-$PID"
          Move-Item -LiteralPath $dest -Destination $old -Force
        }
        try {
          Copy-Item -LiteralPath (Join-Path $binDir $name) -Destination $dest -Force
        } catch {
          if ($old) { Move-Item -LiteralPath $old -Destination $dest -Force -ErrorAction SilentlyContinue }
          throw
        }
        if ($old) { Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue }
      }
    } finally {
      Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Info "mise: installed successfully to $installPath"
  }

  if ($env:MISE_INSTALL_MODIFY_PATH -ne '0' -and $env:MISE_INSTALL_MODIFY_PATH -ne 'false') {
    $dirs = @($installDir)
    if (Test-Flag $env:MISE_INSTALL_SHIMS_PATH) {
      # Same precedence as mise: MISE_SHIMS_DIR, else <data dir>\shims where the
      # data dir is MISE_DATA_DIR, else %XDG_DATA_HOME%\mise, else %LOCALAPPDATA%\mise.
      if ($env:MISE_SHIMS_DIR) {
        $shims = $env:MISE_SHIMS_DIR
      } else {
        if ($env:MISE_DATA_DIR) { $data = $env:MISE_DATA_DIR }
        elseif ($env:XDG_DATA_HOME) { $data = Join-Path $env:XDG_DATA_HOME 'mise' }
        else { $data = Join-Path $env:LOCALAPPDATA 'mise' }
        $shims = Join-Path $data 'shims'
      }
      $dirs += $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($shims)
    }
    Add-ToUserPath $dirs
  }

  if ($env:MISE_INSTALL_HELP -ne '0') {
    Write-Info 'mise: to activate mise in PowerShell, run:'
    # Double single quotes so a path like C:\Users\O'Brien stays one literal.
    # The path sits in a literal inside the profile line, which is itself inside the
    # single-quoted -Value below, so it needs two layers of escaping.
    $quoted = $installPath.Replace("'", "''").Replace("'", "''")
    Write-Info '  if (-not (Test-Path $PROFILE)) { New-Item -ItemType File -Path $PROFILE -Force | Out-Null }'
    Write-Info "  Add-Content -Path `$PROFILE -Value '(& ''$quoted'' activate pwsh) | Out-String | Invoke-Expression'"
    Write-Info 'mise: run `mise doctor` to verify this is set up correctly'
  }
}
