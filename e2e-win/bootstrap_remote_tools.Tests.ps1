# `mise bootstrap remote` resolves the local `ssh` and `tar` with `file::which_spawnable`.
# Plain `file::which` never finds `ssh.exe` on Windows, so this test crosses the process
# boundary: both host tools are `.exe` fixtures on PATH that log how they were launched.
Describe 'bootstrap remote host tools' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:

        $script:Saved = @{}
        foreach ($name in 'MISE_TRUSTED_CONFIG_PATHS', 'PATH', 'MISE_E2E_TOOL_LOG_DIR', 'MISE_E2E_TOOL_SRC', 'MISE_E2E_TOOL_OUT') {
            $script:Saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

        $script:Tools = Join-Path $TestDrive 'tools'
        $script:Source = Join-Path $TestDrive 'source'
        New-Item -ItemType Directory -Force $script:Tools, $script:Source | Out-Null
        '[tools]' | Out-File -FilePath (Join-Path $script:Source 'mise.toml') -Encoding utf8NoBOM

        # A tiny .NET Framework console program: append the argv to <tool>.log and answer
        # with the staging path mise expects from its first ssh call. Built with Windows
        # PowerShell because pwsh cannot emit console applications.
        $program = @'
using System;
using System.IO;
static class Tool {
    static int Main(string[] args) {
        string name = Path.GetFileNameWithoutExtension(Environment.GetCommandLineArgs()[0]);
        string dir = Environment.GetEnvironmentVariable("MISE_E2E_TOOL_LOG_DIR");
        File.AppendAllText(Path.Combine(dir, name + ".log"), string.Join(" ", args) + "\n");
        Console.Out.Write("/tmp/mise-bootstrap.e2ewin\n");
        return 0;
    }
}
'@
        $programFile = Join-Path $script:Tools 'tool.cs'
        $program | Out-File -FilePath $programFile -Encoding utf8NoBOM
        $ssh = Join-Path $script:Tools 'ssh.exe'
        # Paths travel through the environment rather than the command string, so a
        # TestDrive path containing a quote cannot break the inner command line.
        $env:MISE_E2E_TOOL_SRC = $programFile
        $env:MISE_E2E_TOOL_OUT = $ssh
        powershell.exe -NoProfile -Command 'Add-Type -Path $env:MISE_E2E_TOOL_SRC -OutputAssembly $env:MISE_E2E_TOOL_OUT -OutputType ConsoleApplication'
        $LASTEXITCODE | Should -Be 0
        Copy-Item $ssh (Join-Path $script:Tools 'tar.exe')

        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $env:MISE_E2E_TOOL_LOG_DIR = $script:Tools
        $env:PATH = "$script:Tools;$env:PATH"
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($name in $script:Saved.Keys) {
            if ($null -eq $script:Saved[$name]) {
                Remove-Item "Env:\$name" -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:Saved[$name], 'Process')
            }
        }
    }

    It 'launches ssh.exe and tar.exe found on PATH' {
        $out = mise bootstrap remote --host 'e2e@example.invalid' --source $script:Source --dry-run 2>&1 | Out-String

        $out | Should -Not -BeLike "*required command 'ssh' not found*"
        $out | Should -Not -BeLike "*required command 'tar' not found*"
        $out | Should -BeLike "*$script:Tools\ssh.exe *"

        $sshLog = Get-Content (Join-Path $script:Tools 'ssh.log') -Raw
        $sshLog | Should -BeLike '*e2e@example.invalid*'
        $sshLog | Should -BeLike '*mkdir -p /tmp/mise-bootstrap.e2ewin/project*'

        $tarLog = Get-Content (Join-Path $script:Tools 'tar.log') -Raw
        $tarLog | Should -BeLike '*-czf *source.tar.gz*'
    }
}
