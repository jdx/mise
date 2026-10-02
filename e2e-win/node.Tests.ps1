
Describe 'node' {
    It 'executes node 22.0.0' {
        mise x node@22.0.0 -- node -v | Should -be "v22.0.0"
    }

    It 'forwards IPC through an exe shim' {
        # libuv records an IPC stdio entry in STARTUPINFO. The parent Node process here launches
        # the copied node.exe shim directly, so the assertion exercises both hops: shim -> mise
        # and mise -> the selected node.exe.
        $originalShimMode = [Environment]::GetEnvironmentVariable('MISE_WINDOWS_SHIM_MODE', 'Process')
        try {
            $env:MISE_WINDOWS_SHIM_MODE = 'exe'
            mise reshim --force | Out-Null
            $nodeShim = Join-Path $env:MISE_DATA_DIR 'shims\node.exe'
            Test-Path $nodeShim -PathType Leaf | Should -BeTrue

            $parent = @'
const cp = require('node:child_process');
const shim = process.argv[1];
const worker = `
process.once('message', message => {
  if (!message.ping) process.exit(1);
  process.send({ ping: true }, () => {
    process.disconnect();
    // The target must remain alive until its parent observes IPC disconnect and releases it.
    // If either wrapper still has the IPC handle open, that callback never runs and the test times
    // out rather than accepting an EOF caused by target exit.
    process.stdin.setEncoding('utf8');
    process.stdin.once('data', data => process.exit(data === 'release' ? 0 : 1));
  });
});
`;
const child = cp.spawn(shim, ['-e', worker], {
  stdio: ['pipe', 'ignore', 'inherit', 'ipc'],
  windowsHide: true,
});
const fail = message => {
  console.error(message);
  child.kill();
  process.exit(1);
};
const timeout = setTimeout(() => fail('timed out waiting for shim IPC disconnect'), 10_000);
let received = false;
let disconnected = false;
child.once('error', error => fail(error.stack || error.message));
child.once('message', message => {
  if (!message.ping) fail(`unexpected IPC payload: ${JSON.stringify(message)}`);
  received = true;
});
child.once('disconnect', () => {
  if (!received) fail('IPC disconnected before the reply');
  disconnected = true;
  console.log('ipc-ok');
  child.stdin.end('release');
});
child.once('exit', code => {
  clearTimeout(timeout);
  if (!disconnected) fail(`shim child exited before IPC disconnect: ${code}`);
  process.exit(code);
});
child.send({ ping: true });
'@
            $out = mise x node@24.4.1 -- node -e $parent $nodeShim
            $LASTEXITCODE | Should -Be 0
            $out | Should -Be 'ipc-ok'
        }
        finally {
            if ($null -ne $originalShimMode) {
                $env:MISE_WINDOWS_SHIM_MODE = $originalShimMode
            }
            else {
                Remove-Item Env:\MISE_WINDOWS_SHIM_MODE -ErrorAction SilentlyContinue
            }
            mise reshim --force | Out-Null
        }
    }
}
