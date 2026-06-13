Describe '7z' {
    BeforeAll {
        $cfg = ".\mise.local.toml"
        $content = @"
[tools]
"github:ip7z/7zip" = { version = "25.00", asset_pattern = "*-extra.7z" }
"@
        $content | Out-File $cfg
        Get-Content $cfg
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
    }

    It 'executes 7za 25.00' {
        mise install
        mise x github:ip7z/7zip -- 7za | Out-String | Should -Match "7-Zip \(a\) 25\.00"
    }
}

Describe '7z-strip-components' {
    BeforeAll {
        $cfg = ".\mise.local.toml"
        $content = @"
[tools]
"http:ip7z/7zip" = { version = "25.00", url = "https://mise.en.dev/test-fixtures/7z2500-extra.7z" }
"@
        $content | Out-File $cfg
        Get-Content $cfg
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
    }

    It 'executes 7za 25.00' {
        mise install
        mise x http:ip7z/7zip -- 7za | Out-String | Should -Match "7-Zip \(a\) 25\.00"
    }
}

Describe 'aqua-7z' {
    BeforeAll {
        $cfg = ".\mise.local.toml"
        $registryDir = ".\aqua-registry-local"
        New-Item -ItemType Directory -Force $registryDir | Out-Null
        $registryContent = @"
packages:
  - type: http
    name: example/7zip
    supported_envs:
      - windows
    url: https://mise.en.dev/test-fixtures/7z2500-extra.7z
    files:
      - name: 7za
        src: 7za/bin/7za.exe
"@
        $registryContent | Out-File (Join-Path $registryDir "registry.yml")

        $content = @"
[tools]
"aqua:example/7zip" = "25.00"
"@
        $content | Out-File $cfg
        Get-Content $cfg

        $script:oldAquaBakedRegistry = $env:MISE_AQUA_BAKED_REGISTRY
        $script:oldAquaRegistryUrl = $env:MISE_AQUA_REGISTRY_URL
        $env:MISE_AQUA_BAKED_REGISTRY = "0"
        $registryPath = (Resolve-Path $registryDir).Path
        $env:MISE_AQUA_REGISTRY_URL = ([System.Uri]$registryPath).AbsoluteUri
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
        Remove-Item $registryDir -Recurse -Force -ErrorAction Ignore
        $env:MISE_AQUA_BAKED_REGISTRY = $script:oldAquaBakedRegistry
        $env:MISE_AQUA_REGISTRY_URL = $script:oldAquaRegistryUrl
    }

    It 'executes 7za 25.00 via aqua' {
        mise install
        mise x aqua:example/7zip -- 7za | Out-String | Should -Match "7-Zip \(a\) 25\.00"
    }
}
