Describe 'install layout' {
    # The identity install layout (jdx/mise#13678, behind install_layout = "identity"): an installation lives in
    # i\<label>-<hash> beside the installs directory (a shorter path than installs\, for MAX_PATH),
    # and installs\<short>\<version> plus the runtime aliases (latest, 1.7) are links to it. On Windows they have to be real junctions: a text file standing in for a
    # link is invisible to the IDE SDK selectors and external tools this layout keeps working
    # for, and a junction is what makes Test-Path and Get-ChildItem see through to the install.

    BeforeAll {
        $script:OriginalLocation = Get-Location
        $script:Saved = @{}
        foreach ($name in 'MISE_DATA_DIR', 'MISE_INSTALLS_DIR', 'MISE_INSTALL_STORE_DIR', 'MISE_CONFIG_FILE', 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_EXPERIMENTAL', 'MISE_INSTALL_LAYOUT', 'MISE_YES') {
            $script:Saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

        $script:Root = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:Root | Out-Null
        Set-Location $script:Root
        $env:MISE_DATA_DIR = Join-Path $script:Root 'data'
        # the default installs directory, which is what moves installations to the short store
        Remove-Item -Path Env:\MISE_INSTALLS_DIR, Env:\MISE_INSTALL_STORE_DIR -ErrorAction SilentlyContinue
        $env:MISE_CONFIG_FILE = Join-Path $script:Root 'mise.toml'
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:Root
        $env:MISE_EXPERIMENTAL = '1'
        $env:MISE_INSTALL_LAYOUT = 'identity'
        $env:MISE_YES = '1'
        '' | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM

        $script:Installs = Join-Path $env:MISE_DATA_DIR 'installs'
        $script:Store = Join-Path $env:MISE_DATA_DIR 'i'
        $script:ToolDir = Join-Path $script:Installs 'jq'

        # Whether an entry is there, asked by enumerating the parent rather than by resolving the
        # entry: a dangling junction still has to count as present, or "it was removed" could be
        # satisfied by "it was left dangling".
        function script:EntryPresent([string]$Path) {
            $name = Split-Path $Path -Leaf
            [bool](Get-ChildItem -LiteralPath (Split-Path $Path) -Force -ErrorAction SilentlyContinue |
                    Where-Object { $_.Name -eq $name })
        }

        # A directory link: a junction (or a symlink), never a regular file holding a path.
        function script:IsDirectoryLink([string]$Path) {
            $item = Get-Item -LiteralPath $Path -Force
            $isReparse = [bool]($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)
            $item.PSIsContainer -and $isReparse -and (@('Junction', 'SymbolicLink') -contains $item.LinkType)
        }

        mise install jq@1.7.1 jq@1.8.2 | Out-Null
        $script:InstallExit = $LASTEXITCODE
        $script:Dir171 = (mise where jq@1.7.1 | Select-Object -Last 1).Trim()
        $script:Dir182 = (mise where jq@1.8.2 | Select-Object -Last 1).Trim()
        $script:Name171 = Split-Path $script:Dir171 -Leaf
        $script:Name182 = Split-Path $script:Dir182 -Leaf
    }

    AfterAll {
        Set-Location $script:OriginalLocation
        foreach ($name in $script:Saved.Keys) {
            if ($null -eq $script:Saved[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:Saved[$name], 'Process')
            }
        }
    }

    It 'installs into a hashed directory in the short store beside the installs directory' {
        $script:InstallExit | Should -Be 0
        $script:Name171 | Should -Match '^jq-[a-z2-7]{8}$'
        $script:Name182 | Should -Match '^jq-[a-z2-7]{8}$'
        $script:Name171 | Should -Not -Be $script:Name182
        (Split-Path $script:Dir171 -Parent) | Should -Be $script:Store
        (Split-Path $script:Dir182 -Parent) | Should -Be $script:Store
        # the catalog stays with the links, in the installs directory
        Test-Path -LiteralPath (Join-Path $script:Installs ".mise\names\$($script:Name171)") | Should -BeTrue
        Test-Path -LiteralPath (Join-Path $script:Store '.mise') | Should -BeFalse
    }

    It 'reports a real, existing directory from mise where' {
        Test-Path -LiteralPath $script:Dir171 -PathType Container | Should -BeTrue
        Test-Path -LiteralPath $script:Dir182 -PathType Container | Should -BeTrue
        # the installation itself is not a link: links point at it
        $item = Get-Item -LiteralPath $script:Dir171
        [bool]($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) | Should -BeFalse
        Test-Path -LiteralPath (Join-Path $script:Dir171 'jq.exe') -PathType Leaf | Should -BeTrue
        # the same answer every time, for the version and for a prefix of it
        (mise where jq@1.7.1 | Select-Object -Last 1).Trim() | Should -Be $script:Dir171
        (mise where jq@1.7 | Select-Object -Last 1).Trim() | Should -Be $script:Dir171
    }

    It 'writes a receipt into the installation' {
        $receipt = Join-Path $script:Dir171 '.mise-install.toml'
        Test-Path -LiteralPath $receipt -PathType Leaf | Should -BeTrue
        $text = Get-Content -LiteralPath $receipt -Raw
        $text | Should -Match 'version = "1\.7\.1"'
        $text | Should -Match ('dir = "' + [regex]::Escape($script:Name171) + '"')
        $text | Should -Match 'algorithm = "blake3"'
    }

    It 'keeps the version path as a real directory link to the installation' {
        $link = Join-Path $script:ToolDir '1.7.1'
        script:EntryPresent $link | Should -BeTrue
        script:IsDirectoryLink $link | Should -BeTrue
        # not a text file standing in for a link
        (Get-Item -LiteralPath $link -Force).PSIsContainer | Should -BeTrue
        $target = (Get-Item -LiteralPath $link -Force).Target | Select-Object -First 1
        (Split-Path $target -Leaf) | Should -Be $script:Name171
    }

    It 'sees through the version link with Test-Path and Get-ChildItem' {
        $link = Join-Path $script:ToolDir '1.7.1'
        Test-Path -LiteralPath $link -PathType Container | Should -BeTrue
        Test-Path -LiteralPath (Join-Path $link 'jq.exe') -PathType Leaf | Should -BeTrue
        $names = Get-ChildItem -LiteralPath $link -Force | ForEach-Object { $_.Name }
        $names | Should -Contain 'jq.exe'
        $names | Should -Contain '.mise-install.toml'
        $out = & (Join-Path $link 'jq.exe') --version 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'jq-1\.7\.1'
    }

    It 'makes runtime aliases directory links too, never text files' {
        foreach ($alias in 'latest', '1.7', '1.8') {
            $path = Join-Path $script:ToolDir $alias
            script:EntryPresent $path | Should -BeTrue -Because "$alias should exist"
            script:IsDirectoryLink $path | Should -BeTrue -Because "$alias should be a junction, not a file"
            (Get-Item -LiteralPath $path -Force).PSIsContainer | Should -BeTrue
            Test-Path -LiteralPath (Join-Path $path 'jq.exe') -PathType Leaf | Should -BeTrue
            (Get-ChildItem -LiteralPath $path -Force | ForEach-Object { $_.Name }) | Should -Contain 'jq.exe'
        }
        $out = & (Join-Path $script:ToolDir '1.7\jq.exe') --version 2>&1 | Out-String
        $out | Should -Match 'jq-1\.7\.1'
        $out = & (Join-Path $script:ToolDir 'latest\jq.exe') --version 2>&1 | Out-String
        $out | Should -Match 'jq-1\.8\.2'
    }

    It 'runs the tool through mise from the hashed installation' {
        $out = mise x jq@1.7.1 -- jq --version 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'jq-1\.7\.1'
        $out = mise x jq@1.8.2 -- jq --version 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'jq-1\.8\.2'
    }

    It 'removes the installation, its version link and the aliases that named it on uninstall' {
        mise uninstall jq@1.7.1 2>&1 | Out-Null
        $LASTEXITCODE | Should -Be 0
        Test-Path -LiteralPath $script:Dir171 | Should -BeFalse
        script:EntryPresent (Join-Path $script:ToolDir '1.7.1') | Should -BeFalse
        script:EntryPresent (Join-Path $script:ToolDir '1.7') | Should -BeFalse
        # the other installation and what names it are untouched
        Test-Path -LiteralPath (Join-Path $script:Dir182 '.mise-install.toml') | Should -BeTrue
        script:IsDirectoryLink (Join-Path $script:ToolDir '1.8.2') | Should -BeTrue
        script:IsDirectoryLink (Join-Path $script:ToolDir 'latest') | Should -BeTrue
        Test-Path -LiteralPath (Join-Path $script:ToolDir 'latest\jq.exe') | Should -BeTrue
        # no link under the tool directory is left dangling
        foreach ($entry in Get-ChildItem -LiteralPath $script:ToolDir -Force | Where-Object { $_.LinkType }) {
            Test-Path -LiteralPath (Join-Path $entry.FullName 'jq.exe') |
                Should -BeTrue -Because "$($entry.Name) should still lead to an installation"
        }
        $out = mise ls jq 2>&1 | Out-String
        $out | Should -Not -Match '1\.7\.1'
        $out | Should -Match '1\.8\.2'
    }

    It 'reinstalls into the same directory and restores the links' {
        mise install jq@1.7.1 | Out-Null
        $LASTEXITCODE | Should -Be 0
        (mise where jq@1.7.1 | Select-Object -Last 1).Trim() | Should -Be $script:Dir171
        Test-Path -LiteralPath (Join-Path $script:Dir171 '.mise-install.toml') | Should -BeTrue
        script:IsDirectoryLink (Join-Path $script:ToolDir '1.7.1') | Should -BeTrue
        script:IsDirectoryLink (Join-Path $script:ToolDir '1.7') | Should -BeTrue
        Test-Path -LiteralPath (Join-Path $script:ToolDir '1.7.1\jq.exe') | Should -BeTrue
    }

    It 'removes every installation and version link when the whole tool is uninstalled' {
        mise uninstall --all jq 2>&1 | Out-Null
        $LASTEXITCODE | Should -Be 0
        Test-Path -LiteralPath $script:Dir171 | Should -BeFalse
        Test-Path -LiteralPath $script:Dir182 | Should -BeFalse
        # nothing is left behind in the store
        $left = Get-ChildItem -LiteralPath $script:Store -Force |
            Where-Object { $_.Name -like 'jq-*' } | ForEach-Object { $_.Name }
        $left | Should -BeNullOrEmpty
        foreach ($version in '1.7.1', '1.8.2') {
            script:EntryPresent (Join-Path $script:ToolDir $version) | Should -BeFalse
        }
    }
}
