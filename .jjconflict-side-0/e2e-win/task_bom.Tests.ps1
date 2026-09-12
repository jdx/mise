Describe 'file tasks written with a UTF-8 byte-order mark' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "mise-tasks") | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        "[tools]" | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        # `Out-File -Encoding utf8` in Windows PowerShell 5.1 writes exactly this, which is how a
        # hand-written task acquires a mark without its author choosing one.
        function Write-Task {
            param([string]$Name, [string]$Body, [bool]$Bom)
            $path = Join-Path $script:TestRoot "mise-tasks\$Name"
            [System.IO.File]::WriteAllText($path, $Body, (New-Object System.Text.UTF8Encoding $Bom))
        }

        function Get-Task {
            param([string]$Name)
            mise tasks --json | ConvertFrom-Json | Where-Object { $_.name -eq $Name }
        }

        # A shebang is what makes an extensionless file a task on Windows, so the mark decided
        # whether these existed at all. `printf` rather than `echo` so the dispatch test below
        # can only pass through bash: cmd has no `printf`, whereas `echo ran` would print `ran`
        # if the old `cmd /c` fallback ever ran the file as a batch script.
        $shebang = "#!/usr/bin/env bash`nprintf 'ran\n'`n"
        Write-Task -Name "marked" -Body $shebang -Bom $true
        Write-Task -Name "unmarked" -Body $shebang -Bom $false
        # `.ps1` is found by its extension either way, so here the mark only reaches the header
        # on line 1 -- where a task without a shebang naturally puts it.
        $header = "#MISE description=`"from the header`"`nWrite-Output ran`n"
        Write-Task -Name "marked_header.ps1" -Body $header -Bom $true
        Write-Task -Name "unmarked_header.ps1" -Body $header -Bom $false

        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'finds a marked shebang task' {
        Get-Task -Name "marked" | Should -Not -BeNullOrEmpty
    }

    It 'finds the unmarked twin too' {
        # Control: the two files differ only by the mark, so this pins the fixture itself.
        Get-Task -Name "unmarked" | Should -Not -BeNullOrEmpty
    }

    It 'runs a marked shebang task rather than falling back to cmd' {
        $output = mise run marked | Out-String
        $output | Should -Match "ran"
    }

    It 'reads a header on line 1 behind the mark' {
        (Get-Task -Name "marked_header").description | Should -Be "from the header"
    }

    It 'reads the unmarked header the same way' {
        (Get-Task -Name "unmarked_header").description | Should -Be "from the header"
    }
}
