Describe 'task stub names' {
    BeforeAll {
        # Outside the shared TestDrive config: this suite needs a project whose only tasks are the
        # ones below, so the stub directory can be asserted file by file.
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:Root = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:Root 'mise-tasks') -Force | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:Root
        '[tools]' | Out-File -FilePath (Join-Path $script:Root 'mise.toml') -Encoding utf8NoBOM
        # A `.bat` file task. Windows will not run the `#!/bin/sh` stub itself, and naming that stub
        # `.bat` used to make cmd.exe run the shebang script as a batch file, echoing each line.
        @'
@echo off
echo from-bat
'@ | Out-File -FilePath (Join-Path $script:Root 'mise-tasks\batTask.bat') -Encoding ascii
        Set-Location $script:Root
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'names the stub after the task, not after the file' {
        mise generate task-stubs | Out-Null
        $LASTEXITCODE | Should -Be 0
        Test-Path 'bin\batTask' | Should -BeTrue
        Test-Path 'bin\batTask.cmd' | Should -BeTrue
        # The negative control: the old spelling is what cmd.exe mis-executed, so its absence is
        # the point rather than a tidiness check.
        Test-Path 'bin\batTask.bat' | Should -BeFalse
    }

    It 'produces a launcher Windows can actually run' {
        # What the old naming cost: `bin\batTask.bat` held `#!/bin/sh` and cmd ran it line by line,
        # reporting `'#!' is not recognized`. The launcher has to reach the task instead.
        $out = & '.\bin\batTask.cmd' 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*from-bat*'
        $out | Should -Not -BeLike '*is not recognized*'
    }
}
