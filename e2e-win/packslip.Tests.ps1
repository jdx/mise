Describe 'packslip' {
    It 'puts a Packslip executable on PATH with its Windows extension' {
        $previousMinimumReleaseAge = $env:MISE_MINIMUM_RELEASE_AGE
        $env:MISE_MINIMUM_RELEASE_AGE = '0'
        try {
            mise x packslip:github.com/jdx/fnox@1.35.1 -- fnox --version |
                Out-String | Should -Match 'fnox 1\.35\.1'

            $installPath = (mise where packslip:github.com/jdx/fnox@1.35.1).Trim()
            Join-Path $installPath '.mise-bins\fnox.exe' | Should -Exist
            Join-Path $installPath '.mise-bins\fnox' | Should -Not -Exist
        }
        finally {
            $env:MISE_MINIMUM_RELEASE_AGE = $previousMinimumReleaseAge
        }
    }
}
