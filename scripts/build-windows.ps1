Get-ChildItem target\release
New-Item dist\rtx\bin -ItemType Directory -ea 0
Copy-Item target\release\rtx.exe dist\rtx\bin\rtx.exe
$Env:RTX_VERSION = (cargo get version --pretty)
Compress-Archive -Path dist\rtx -DestinationPath dist\rtx-$env:RTX_VERSION-windows-x64.zip
