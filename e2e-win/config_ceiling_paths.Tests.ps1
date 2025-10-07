Describe 'config_ceiling_paths' {
    BeforeAll {
        # Create test directory structure
        $TestRoot = Get-Location
        $ParentDir = Join-Path $TestRoot "parent"
        $ChildDir = Join-Path $ParentDir "child"
        $GrandchildDir = Join-Path $ChildDir "grandchild"

        New-Item -ItemType Directory -Path $ParentDir -Force | Out-Null
        New-Item -ItemType Directory -Path $ChildDir -Force | Out-Null
        New-Item -ItemType Directory -Path $GrandchildDir -Force | Out-Null

        # Create config files at different levels
        $ParentConfig = Join-Path $ParentDir ".mise.toml"
        $ChildConfig = Join-Path $ChildDir ".mise.toml"
        $GrandchildConfig = Join-Path $GrandchildDir ".mise.toml"

        @"
[env]
PARENT = "true"
"@ | Out-File $ParentConfig

        @"
[env]
CHILD = "true"
"@ | Out-File $ChildConfig

        @"
[env]
GRANDCHILD = "true"
"@ | Out-File $GrandchildConfig

        # Change to grandchild directory for tests
        Set-Location $GrandchildDir
    }

    AfterAll {
        # Clean up test directories
        Set-Location $TestRoot
        Remove-Item -Path (Join-Path $TestRoot "parent") -Recurse -Force -ErrorAction Ignore
        Remove-Item Env:MISE_CEILING_PATHS -ErrorAction Ignore
    }

    It 'finds all configs without ceiling paths' {
        Remove-Item Env:MISE_CEILING_PATHS -ErrorAction Ignore
        $output = mise env | Out-String
        $output | Should -Match "GRANDCHILD=true"
        $output | Should -Match "CHILD=true"
        $output | Should -Match "PARENT=true"
    }

    It 'respects ceiling path at child directory' {
        $env:MISE_CEILING_PATHS = Join-Path $TestRoot "parent\child"
        $output = mise env | Out-String
        $output | Should -Match "GRANDCHILD=true"
        $output | Should -Not -Match "CHILD=true"
        $output | Should -Not -Match "PARENT=true"
    }

    It 'respects ceiling path at grandchild directory' {
        $env:MISE_CEILING_PATHS = Join-Path $TestRoot "parent\child\grandchild"
        $output = mise env | Out-String
        $output | Should -Not -Match "GRANDCHILD=true"
        $output | Should -Not -Match "CHILD=true"
        $output | Should -Not -Match "PARENT=true"
    }

    It 'handles multiple ceiling paths' {
        $ChildPath = Join-Path $TestRoot "parent\child"
        $ParentPath = Join-Path $TestRoot "parent"
        $env:MISE_CEILING_PATHS = "$ChildPath;$ParentPath"
        $output = mise env | Out-String
        $output | Should -Match "GRANDCHILD=true"
        $output | Should -Not -Match "CHILD=true"
        $output | Should -Not -Match "PARENT=true"
    }

    It 'handles non-existent ceiling path' {
        $env:MISE_CEILING_PATHS = Join-Path $TestRoot "nonexistent"
        $output = mise env | Out-String
        $output | Should -Match "GRANDCHILD=true"
        $output | Should -Match "CHILD=true"
        $output | Should -Match "PARENT=true"
    }
}