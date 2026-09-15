Describe 'task console interrupt' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        # `mise run` is interrupted by a console control event rather than a signal, and only a
        # process in a group of its own can be sent one without the test host receiving it too.
        # The child's stdin is a pipe nobody writes to: `cmd.exe` stops to ask "Terminate batch
        # job (Y/N)?" when it is interrupted while running a batch file, and a stdin at EOF would
        # answer that question before mise had to deal with it.
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class MiseConsoleCtrl
{
    [StructLayout(LayoutKind.Sequential)]
    private struct STARTUPINFO
    {
        public int cb;
        public IntPtr lpReserved, lpDesktop, lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_INFORMATION
    {
        public IntPtr hProcess, hThread;
        public int dwProcessId, dwThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct SECURITY_ATTRIBUTES
    {
        public int nLength;
        public IntPtr lpSecurityDescriptor;
        public int bInheritHandle;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool CreateProcessW(string applicationName, StringBuilder commandLine,
        IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint creationFlags,
        IntPtr environment, string currentDirectory, ref STARTUPINFO startupInfo,
        out PROCESS_INFORMATION processInformation);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateFileW(string fileName, uint access, uint shareMode,
        ref SECURITY_ATTRIBUTES securityAttributes, uint creationDisposition, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CreatePipe(out IntPtr readPipe, out IntPtr writePipe,
        ref SECURITY_ATTRIBUTES securityAttributes, uint size);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GenerateConsoleCtrlEvent(uint ctrlEvent, uint processGroupId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetConsoleWindow();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AllocConsole();

    private const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
    private const uint CTRL_BREAK_EVENT = 1;
    private const uint STARTF_USESTDHANDLES = 0x00000100;
    private const uint GENERIC_WRITE = 0x40000000;
    private const uint FILE_SHARE_READ_WRITE = 0x00000003;
    private const uint CREATE_ALWAYS = 2;
    private const uint FILE_ATTRIBUTE_NORMAL = 0x00000080;

    // Kept alive for the life of the test host: closing the write end would put the child's
    // stdin at EOF, which is exactly what this test needs not to happen.
    private static IntPtr stdinWriteEnd = IntPtr.Zero;

    public static bool EnsureConsole()
    {
        return GetConsoleWindow() != IntPtr.Zero || AllocConsole();
    }

    public static int Start(string commandLine, string workingDirectory, string outputPath)
    {
        var inheritable = new SECURITY_ATTRIBUTES();
        inheritable.nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
        inheritable.bInheritHandle = 1;

        IntPtr stdinReadEnd;
        if (!CreatePipe(out stdinReadEnd, out stdinWriteEnd, ref inheritable, 0))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        IntPtr output = CreateFileW(outputPath, GENERIC_WRITE, FILE_SHARE_READ_WRITE,
            ref inheritable, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, IntPtr.Zero);
        if (output == new IntPtr(-1))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }

        var startupInfo = new STARTUPINFO();
        startupInfo.cb = Marshal.SizeOf(typeof(STARTUPINFO));
        startupInfo.dwFlags = (int)STARTF_USESTDHANDLES;
        startupInfo.hStdInput = stdinReadEnd;
        startupInfo.hStdOutput = output;
        startupInfo.hStdError = output;

        PROCESS_INFORMATION info;
        if (!CreateProcessW(null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, true,
                CREATE_NEW_PROCESS_GROUP, IntPtr.Zero, workingDirectory, ref startupInfo, out info))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
        CloseHandle(stdinReadEnd);
        CloseHandle(output);
        return info.dwProcessId;
    }

    public static void Interrupt(int processGroupId)
    {
        if (!GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, (uint)processGroupId))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
    }
}
'@

        function script:Get-Descendants([int]$Root) {
            $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
            $found = @()
            $frontier = @($Root)
            while ($frontier.Count -gt 0) {
                $children = $all | Where-Object { $frontier -contains $_.ParentProcessId }
                $frontier = @($children | ForEach-Object { $_.ProcessId })
                $found += $frontier
            }
            return $found
        }

        function script:Wait-Until([scriptblock]$Condition, [int]$TimeoutSeconds) {
            $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
            while ((Get-Date) -lt $deadline) {
                if (& $Condition) { return $true }
                Start-Sleep -Milliseconds 200
            }
            return & $Condition
        }

        function script:Test-Running([int]$ProcessId) {
            return $null -ne (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'supervises the interrupt instead of leaving the task behind' {
        if (-not [MiseConsoleCtrl]::EnsureConsole()) {
            Set-ItResult -Skipped -Because 'the test host has no console to raise a control event on'
            return
        }

        @'
[tasks.sleeper]
run = "sleeper.cmd"
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        # A batch file, not a bare command: `cmd.exe` only stops to ask about terminating a
        # *batch job*, and that prompt is what left an orphan behind (discussion #13215).
        @'
@echo off
:loop
ping -n 2 127.0.0.1 >nul
goto loop
'@ | Out-File -FilePath sleeper.cmd -Encoding ascii

        $mise = (Get-Command mise).Source
        $log = Join-Path $TestDrive 'run.log'
        $misePid = [MiseConsoleCtrl]::Start("`"$mise`" run sleeper", "$TestDrive", $log)
        try {
            (Wait-Until { (Get-Descendants $misePid).Count -gt 0 } 60) | Should -BeTrue -Because 'the task should start'
            $descendants = Get-Descendants $misePid

            [MiseConsoleCtrl]::Interrupt($misePid)

            # The whole point of the fix: mise used to be torn down here by the default console
            # handler, leaving the `cmd.exe` waiting on its prompt holding the terminal.
            Start-Sleep -Seconds 3
            (Test-Running $misePid) | Should -BeTrue -Because 'mise should stay up to shut its task down'
            (Get-Content $log -Raw) | Should -BeLike '*press Ctrl-C again*'

            [MiseConsoleCtrl]::Interrupt($misePid)

            (Wait-Until { -not (Test-Running $misePid) } 60) | Should -BeTrue -Because 'a second interrupt should exit'
            foreach ($descendant in $descendants) {
                (Wait-Until { -not (Test-Running $descendant) } 30) |
                    Should -BeTrue -Because "the task process $descendant should not outlive mise"
            }
        } finally {
            if (Test-Running $misePid) {
                taskkill /F /T /PID $misePid 2>&1 | Out-Null
            }
            foreach ($descendant in (Get-Descendants $misePid)) {
                taskkill /F /PID $descendant 2>&1 | Out-Null
            }
        }
    }
}
