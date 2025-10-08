Describe 'tasks_ceiling_paths' {
    BeforeAll {
        # Create test directory structure
        $TestRoot = Get-Location
        $ParentDir = Join-Path $TestRoot "parent"
        $ChildDir = Join-Path $ParentDir "child"
        $GrandchildDir = Join-Path $ChildDir "grandchild"

        New-Item -ItemType Directory -Path $ParentDir -Force | Out-Null
        New-Item -ItemType Directory -Path $ChildDir -Force | Out-Null
        New-Item -ItemType Directory -Path $GrandchildDir -Force | Out-Null

        # Create task directories
        $ParentTasks = Join-Path $ParentDir "mise-tasks"
        $ChildTasks = Join-Path $ChildDir "mise-tasks"
        $GrandchildTasks = Join-Path $GrandchildDir "mise-tasks"

        New-Item -ItemType Directory -Path $ParentTasks -Force | Out-Null
        New-Item -ItemType Directory -Path $ChildTasks -Force | Out-Null
        New-Item -ItemType Directory -Path $GrandchildTasks -Force | Out-Null

        # Create task files
        $ParentTask = Join-Path $ParentTasks "task-parent.ps1"
        $ChildTask = Join-Path $ChildTasks "task-child.ps1"
        $GrandchildTask = Join-Path $GrandchildTasks "task-grandchild.ps1"

        @"
#!/usr/bin/env pwsh
Write-Output "parent"
"@ | Out-File $ParentTask

        @"
#!/usr/bin/env pwsh
Write-Output "child"
"@ | Out-File $ChildTask

        @"
#!/usr/bin/env pwsh
Write-Output "grandchild"
"@ | Out-File $GrandchildTask

        # Change to grandchild directory for tests
        Set-Location $GrandchildDir
    }

    AfterAll {
        # Clean up test directories
        Set-Location $TestRoot
        Remove-Item -Path (Join-Path $TestRoot "parent") -Recurse -Force -ErrorAction Ignore
        Remove-Item Env:MISE_CEILING_PATHS -ErrorAction Ignore
    }

    It 'finds all tasks without ceiling paths' {
        Remove-Item Env:MISE_CEILING_PATHS -ErrorAction Ignore
        $output = mise tasks | Out-String
        $output | Should -Match "task-grandchild"
        $output | Should -Match "task-child"
        $output | Should -Match "task-parent"
    }

    It 'respects ceiling path at child directory' {
        $env:MISE_CEILING_PATHS = Join-Path $TestRoot "parent\child"
        $output = mise tasks | Out-String
        $output | Should -Match "task-grandchild"
        $output | Should -Not -Match "task-child"
        $output | Should -Not -Match "task-parent"
    }

    It 'respects ceiling path at grandchild directory' {
        $env:MISE_CEILING_PATHS = Join-Path $TestRoot "parent\child\grandchild"
        $output = mise tasks | Out-String
        $output | Should -Not -Match "task-grandchild"
        $output | Should -Not -Match "task-child"
        $output | Should -Not -Match "task-parent"
    }

    It 'handles multiple ceiling paths' {
        $ChildPath = Join-Path $TestRoot "parent\child"
        $ParentPath = Join-Path $TestRoot "parent"
        $env:MISE_CEILING_PATHS = "$ChildPath;$ParentPath"
        $output = mise tasks | Out-String
        $output | Should -Match "task-grandchild"
        $output | Should -Not -Match "task-child"
        $output | Should -Not -Match "task-parent"
    }

    It 'handles non-existent ceiling path' {
        $env:MISE_CEILING_PATHS = Join-Path $TestRoot "nonexistent"
        $output = mise tasks | Out-String
        $output | Should -Match "task-grandchild"
        $output | Should -Match "task-child"
        $output | Should -Match "task-parent"
    }
}