Describe 'helm' {
    It 'installs helm 3.14.3' {
        mise x helm@3.14.3 -- helm version --short | Should -BeLike "v3.14.3*"
    }
} 
