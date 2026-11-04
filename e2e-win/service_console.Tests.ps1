# Inspect desktop windows rather than the message-only HWND returned by
# GetConsoleWindow under Windows Terminal. Keep configuration and state isolated.
# Select Windows Terminal as the default terminal with automatic tab closing and
# application title changes enabled to exercise the ConPTY case.
Describe 'Windows unattended console' {
    BeforeAll {
        $script:OriginalDirectory = Get-Location
        $script:OriginalConsoleEnvironment = @{}
        foreach ($name in @('MISE_CONFIG_DIR', 'MISE_GLOBAL_CONFIG_FILE', 'MISE_SYSTEM_CONFIG_DIR',
                'MISE_DATA_DIR', 'MISE_CACHE_DIR', 'MISE_STATE_DIR', 'MISE_TRUSTED_CONFIG_PATHS',
                'MISE_SELF_UPDATE_AUTO', 'MISE_QUIET', 'MISE_LOG_FILE', 'MISE_LOG_LEVEL', 'MISE_LOG_FILE_LEVEL',
                'MISE_CONSOLE_TEST_SOURCE', 'MISE_CONSOLE_TEST_BINARY')) {
            $script:OriginalConsoleEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        $script:MiseConsoleExe = [IO.Path]::GetFullPath((Get-Command mise -CommandType Application | Select-Object -First 1).Source)
        Set-Location $TestDrive
        $env:MISE_CONFIG_DIR = Join-Path $TestDrive 'config'
        $env:MISE_GLOBAL_CONFIG_FILE = Join-Path $env:MISE_CONFIG_DIR 'config.toml'
        $env:MISE_SYSTEM_CONFIG_DIR = Join-Path $TestDrive 'system'
        $env:MISE_DATA_DIR = Join-Path $TestDrive 'data'
        $env:MISE_CACHE_DIR = Join-Path $TestDrive 'cache'
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $env:MISE_SELF_UPDATE_AUTO = 'false'
        # Readiness is an INFO file-log message, independent of the invoking shell.
        $env:MISE_QUIET = 'false'
        $env:MISE_LOG_LEVEL = 'info'
        $env:MISE_LOG_FILE_LEVEL = 'info'
        New-Item -ItemType Directory -Path $env:MISE_CONFIG_DIR | Out-Null
        @'
[settings]
self_update.auto = false
tool_update.global_auto = false
'@ | Set-Content -LiteralPath $env:MISE_GLOBAL_CONFIG_FILE -Encoding utf8NoBOM

        # A real private console, with no hidden-window flag. The unique title lets the
        # desktop oracle distinguish this console from terminals opened by the user.
        $native = @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.IO;
using System.Threading;
public static class MiseConsoleWindow {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    struct Startup {
        public int size;
        public string reserved, desktop, title;
        public uint x,y,width,height,columns,rows,fill,flags;
        public ushort show, reservedSize;
        public IntPtr reservedPointer,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct Child { public IntPtr process,thread; public int pid,tid; }
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
    static extern bool CreateProcessW(string application,StringBuilder command,IntPtr a,IntPtr t,
        bool inherit,uint flags,IntPtr env,string cwd,ref Startup startup,out Child child);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    delegate bool Callback(IntPtr window,IntPtr unused);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback,IntPtr unused);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr window,StringBuilder text,int length);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern int GetClassNameW(IntPtr window,StringBuilder text,int length);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr window,int attribute,out int value,int size);
    [DllImport("kernel32.dll")] static extern IntPtr GetConsoleWindow();
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string name);
    [DllImport("kernel32.dll",CharSet=CharSet.Ansi,ExactSpelling=true)] static extern IntPtr GetProcAddress(IntPtr module,string name);
    public static bool SupportsWindowlessConsole() {
        return GetProcAddress(GetModuleHandleW("kernel32.dll"),"AllocConsoleWithOptions")!=IntPtr.Zero;
    }
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool FreeConsole();
    [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenThread(uint access,bool inherit,uint id);
    [DllImport("kernel32.dll")] static extern uint ResumeThread(IntPtr thread);
    static int lastThread;
    public static Process Start(string program,string args,string directory,string title) {
        return Start(program,args,directory,title,0x10);
    }
    static Process Start(string program,string args,string directory,string title,uint flags) {
        var startup=new Startup { size=Marshal.SizeOf(typeof(Startup)),title=title };
        Child child;
        if(!CreateProcessW(program,new StringBuilder("\""+program+"\" "+args),IntPtr.Zero,IntPtr.Zero,
            false,flags,IntPtr.Zero,directory,ref startup,out child))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        lastThread=child.tid;
        try { var process=Process.GetProcessById(child.pid); var handle=process.Handle; return process; }
        finally { CloseHandle(child.thread); CloseHandle(child.process); }
    }
    public static int Main(string[] args) {
        // Inherit an already visible console, then make it private before mise runs.
        // This removes the race between Terminal painting and the old hide call.
        var kind=new StringBuilder(128);
        if(GetClassNameW(GetConsoleWindow(),kind,kind.Capacity)==0)
            throw new Win32Exception(Marshal.GetLastWin32Error());
        File.WriteAllText(Path.Combine(args[1],args[2]+".console"),kind.ToString());
        string gate=Path.Combine(args[1],args[2]+".go");
        for(int n=0;n<1200 && !File.Exists(gate);n++) Thread.Sleep(25);
        if(!File.Exists(gate)) return 98;
        using(var child=Start(args[0],"__tool-update --watch",args[1],args[2],4)) {
            if(!FreeConsole()) throw new Win32Exception(Marshal.GetLastWin32Error());
            IntPtr thread=OpenThread(2,false,(uint)lastThread);
            if(thread==IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
            try { if(ResumeThread(thread)==UInt32.MaxValue) throw new Win32Exception(Marshal.GetLastWin32Error()); }
            finally { CloseHandle(thread); }
            child.WaitForExit();
            return child.ExitCode;
        }
    }
    public static int Visible(string title) {
        int count=0;
        EnumWindows(delegate(IntPtr window,IntPtr unused) {
            if(!IsWindowVisible(window)) return true;
            int cloaked;
            if(DwmGetWindowAttribute(window,14,out cloaked,4)==0 && cloaked!=0) return true;
            var name=new StringBuilder(128); GetClassNameW(window,name,name.Capacity);
            if(name.ToString()!="ConsoleWindowClass" && name.ToString()!="CASCADIA_HOSTING_WINDOW_CLASS") return true;
            var text=new StringBuilder(2048); GetWindowTextW(window,text,text.Capacity);
            if(text.ToString().Contains(title)) count++;
            return true;
        },IntPtr.Zero);
        return count;
    }
}
'@
        $env:MISE_CONSOLE_TEST_SOURCE = Join-Path $TestDrive 'console.cs'
        $env:MISE_CONSOLE_TEST_BINARY = Join-Path $TestDrive 'console.exe'
        $native | Set-Content -LiteralPath $env:MISE_CONSOLE_TEST_SOURCE -Encoding utf8NoBOM
        # Windows PowerShell can emit a .NET Framework console exe; pwsh cannot.
        powershell.exe -NoProfile -Command 'Add-Type -Path $env:MISE_CONSOLE_TEST_SOURCE -OutputAssembly $env:MISE_CONSOLE_TEST_BINARY -OutputType ConsoleApplication'
        $LASTEXITCODE | Should -Be 0
        [void][Reflection.Assembly]::Load([IO.File]::ReadAllBytes($env:MISE_CONSOLE_TEST_BINARY))
        function script:Wait-Console([scriptblock]$Condition, [int]$Seconds = 30) {
            $deadline = (Get-Date).AddSeconds($Seconds)
            do {
                if (& $Condition) { return $true }
                Start-Sleep -Milliseconds 50
            } while ((Get-Date) -lt $deadline)
            return $false
        }
    }

    AfterAll {
        Set-Location $script:OriginalDirectory
        foreach ($name in $script:OriginalConsoleEnvironment.Keys) {
            # PowerShell's method binder turns a saved null into an empty string.
            if ($null -eq $script:OriginalConsoleEnvironment[$name]) {
                Remove-Item ("Env:" + $name) -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:OriginalConsoleEnvironment[$name], 'Process')
            }
        }
    }

    It '<Behavior>' -TestCases @(
        @{ Shared = $true; Behavior = 'leaves a shared shell visible' },
        @{ Shared = $false; Behavior = 'keeps a private Windows Terminal updater off the desktop' }
    ) {
        param($Shared, $Behavior)
        $tag = 'mise-console-' + [guid]::NewGuid().ToString('N')
        $env:MISE_STATE_DIR = Join-Path $TestDrive $tag
        $env:MISE_LOG_FILE = Join-Path $TestDrive ($tag + '.log')
        $program = $env:MISE_CONSOLE_TEST_BINARY
        $arguments = '"' + $script:MiseConsoleExe + '" "' + $TestDrive + '" "' + $tag + '"'
        if ($Shared) {
            $program = $env:ComSpec
            $arguments = '/d /c title ' + $tag + ' & "' + $script:MiseConsoleExe + '" __tool-update --watch'
        }
        $process = [MiseConsoleWindow]::Start($program, $arguments, $TestDrive, $tag)
        try {
            if (-not $Shared) {
                $consoleKindFile = Join-Path $TestDrive ($tag + '.console')
                Wait-Console {
                    (Test-Path -LiteralPath $consoleKindFile) -and
                    (Get-Content -LiteralPath $consoleKindFile -Raw)
                } | Should -BeTrue -Because 'the console host must be identified before the watcher starts'
                $consoleKind = (Get-Content -LiteralPath $consoleKindFile -Raw).Trim()
                if ($consoleKind -eq 'ConsoleWindowClass') {
                    Set-ItResult -Skipped -Because 'select Windows Terminal as the default terminal to exercise ConPTY'
                    return
                }
                $consoleKind | Should -Be 'PseudoConsoleWindow' -Because 'this regression requires a Windows Terminal console'
                if (-not [MiseConsoleWindow]::SupportsWindowlessConsole()) {
                    Set-ItResult -Skipped -Because 'this Windows version cannot replace a private Terminal console'
                    return
                }
                Wait-Console { [MiseConsoleWindow]::Visible($tag) -gt 0 } |
                    Should -BeTrue -Because 'the private console must be visible before the watcher starts'
                New-Item -ItemType File -Path (Join-Path $TestDrive ($tag + '.go')) | Out-Null
            }
            Wait-Console {
                -not $process.HasExited -and (Test-Path -LiteralPath $env:MISE_LOG_FILE) -and
                (Get-Content -LiteralPath $env:MISE_LOG_FILE -Raw) -match 'checking opted-in tools every hour'
            } | Should -BeTrue -Because 'a real watcher must be running before inspecting its window'
            if ($Shared) {
                Wait-Console { [MiseConsoleWindow]::Visible($tag) -gt 0 } |
                    Should -BeTrue -Because 'the shared shell is a positive control for the desktop oracle'
            } else {
                Wait-Console { [MiseConsoleWindow]::Visible($tag) -eq 0 } 10 |
                    Should -BeTrue -Because 'hiding the ConPTY message window must not leave Windows Terminal visible'
                # Terminal can paint after the watcher announces readiness. A single
                # empty snapshot would incorrectly pass before that window appears.
                $deadline = (Get-Date).AddSeconds(5)
                do {
                    [MiseConsoleWindow]::Visible($tag) | Should -Be 0
                    $process.HasExited | Should -BeFalse
                    Start-Sleep -Milliseconds 50
                } while ((Get-Date) -lt $deadline)
            }
        } finally {
            if (-not $process.HasExited) { $process.Kill($true); [void]$process.WaitForExit(10000) }
            $process.Dispose()
        }
    }
}
