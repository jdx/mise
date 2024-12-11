---
editLink: false
---

# Registry

In general, the preferred backend to use for new tools is the following:

- [aqua](./dev-tools/backends/aqua.html) - offers the most features and security while not requiring plugins
- [ubi](./dev-tools/backends/ubi.html) - very simple to use
- [pipx](./dev-tools/backends/pipx.html) - only for python tools, requires python to be installed but this generally would always be the case for python tools
- [npm](./dev-tools/backends/npm.html) - only for node tools, requires node to be installed but this generally would always be the case for node tools
- [vfox](./dev-tools/backends/vfox.html) - only for tools that have unique installation requirements or need to modify env vars
- [asdf](./dev-tools/backends/asdf.html) - only for tools that have unique installation requirements or need to modify env vars, doesn't support windows
- [go](./dev-tools/backends/go.html) - only for go tools, requires go to be installed to compile. Because go tools can be distributed as a single binary, aqua/ubi are definitely preferred.
- [cargo](./dev-tools/backends/cargo.html) - only for rust tools, requires rust to be installed to compile. Because rust tools can be distributed as a single binary, aqua/ubi are definitely preferred.

However, each tool can define its own priority if it has more than 1 backend it supports. You can disable a backend with `mise settings disable_backends=asdf`.
And it will be skipped. See [Aliases](/dev-tools/aliases.html) for a way to set a default backend for a tool.

You can also specify the full name for a tool using `mise use aqua:1password/cli` if you want to use a specific backend.

| Short | Full |
| ----------- | --------------- |
| 1password | [asdf:NeoHsu/asdf-1password-cli](https://github.com/NeoHsu/asdf-1password-cli) [aqua:1password/cli](https://github.com/1password/cli) |
| aapt2 | [asdf:ronnnnn/asdf-aapt2](https://github.com/ronnnnn/asdf-aapt2) |
| act | [aqua:nektos/act](https://github.com/nektos/act) [ubi:nektos/act](https://github.com/nektos/act) [asdf:gr1m0h/asdf-act](https://github.com/gr1m0h/asdf-act) |
| action-validator | [aqua:mpalmer/action-validator](https://github.com/mpalmer/action-validator) [ubi:mpalmer/action-validator](https://github.com/mpalmer/action-validator) [asdf:mpalmer/action-validator](https://github.com/mpalmer/action-validator) |
| actionlint | [ubi:rhysd/actionlint](https://github.com/rhysd/actionlint) [asdf:crazy-matt/asdf-actionlint](https://github.com/crazy-matt/asdf-actionlint) |
| adr-tools | [aqua:npryce/adr-tools](https://github.com/npryce/adr-tools) [asdf:https://gitlab.com/td7x/asdf/adr-tools](https://gitlab.com/td7x/asdf/adr-tools) |
| ag | [asdf:koketani/asdf-ag](https://github.com/koketani/asdf-ag) |
| age | [aqua:FiloSottile/age](https://github.com/FiloSottile/age) [asdf:threkk/asdf-age](https://github.com/threkk/asdf-age) |
| age-plugin-yubikey | [ubi:str4d/age-plugin-yubikey](https://github.com/str4d/age-plugin-yubikey) [asdf:joke/asdf-age-plugin-yubikey](https://github.com/joke/asdf-age-plugin-yubikey) |
| agebox | [ubi:slok/agebox](https://github.com/slok/agebox) [asdf:slok/asdf-agebox](https://github.com/slok/asdf-agebox) |
| air | [aqua:air-verse/air](https://github.com/air-verse/air) [asdf:pdemagny/asdf-air](https://github.com/pdemagny/asdf-air) |
| aks-engine | [aqua:Azure/aks-engine](https://github.com/Azure/aks-engine) [asdf:robsonpeixoto/asdf-aks-engine](https://github.com/robsonpeixoto/asdf-aks-engine) |
| allure | [asdf:comdotlinux/asdf-allure](https://github.com/comdotlinux/asdf-allure) |
| alp | [aqua:tkuchiki/alp](https://github.com/tkuchiki/alp) [asdf:asdf-community/asdf-alp](https://github.com/asdf-community/asdf-alp) |
| amass | [ubi:owasp-amass/amass](https://github.com/owasp-amass/amass) [asdf:dhoeric/asdf-amass](https://github.com/dhoeric/asdf-amass) |
| amazon-ecr-credential-helper | [aqua:awslabs/amazon-ecr-credential-helper](https://github.com/awslabs/amazon-ecr-credential-helper) [asdf:dex4er/asdf-amazon-ecr-credential-helper](https://github.com/dex4er/asdf-amazon-ecr-credential-helper) |
| ansible-base | [asdf:amrox/asdf-pyapp](https://github.com/amrox/asdf-pyapp) |
| ant | [asdf:jackboespflug/asdf-ant](https://github.com/jackboespflug/asdf-ant) |
| apko | [aqua:chainguard-dev/apko](https://github.com/chainguard-dev/apko) [ubi:chainguard-dev/apko](https://github.com/chainguard-dev/apko) [asdf:omissis/asdf-apko](https://github.com/omissis/asdf-apko) |
| apollo-ios | [asdf:MacPaw/asdf-apollo-ios-cli](https://github.com/MacPaw/asdf-apollo-ios-cli) |
| apollo-router | [ubi:apollographql/router](https://github.com/apollographql/router) [asdf:safx/asdf-apollo-router](https://github.com/safx/asdf-apollo-router) |
| apollo-rover | [ubi:apollographql/rover](https://github.com/apollographql/rover) |
| arduino | [aqua:arduino/arduino-cli](https://github.com/arduino/arduino-cli) [asdf:egnor/asdf-arduino-cli](https://github.com/egnor/asdf-arduino-cli) |
| argc | [ubi:sigoden/argc](https://github.com/sigoden/argc) |
| argo | [aqua:argoproj/argo-workflows](https://github.com/argoproj/argo-workflows) [asdf:sudermanjr/asdf-argo](https://github.com/sudermanjr/asdf-argo) |
| argo-rollouts | [aqua:argoproj/argo-rollouts](https://github.com/argoproj/argo-rollouts) [asdf:abatilo/asdf-argo-rollouts](https://github.com/abatilo/asdf-argo-rollouts) |
| argocd | [ubi:argoproj/argo-cd](https://github.com/argoproj/argo-cd) [asdf:beardix/asdf-argocd](https://github.com/beardix/asdf-argocd) |
| asciidoctorj | [asdf:gliwka/asdf-asciidoctorj](https://github.com/gliwka/asdf-asciidoctorj) |
| assh | [asdf:zekker6/asdf-assh](https://github.com/zekker6/asdf-assh) |
| atlas | [aqua:ariga/atlas](https://github.com/ariga/atlas) [asdf:komi1230/asdf-atlas](https://github.com/komi1230/asdf-atlas) |
| atmos | [aqua:cloudposse/atmos](https://github.com/cloudposse/atmos) [asdf:cloudposse/asdf-atmos](https://github.com/cloudposse/asdf-atmos) |
| auto-doc | [asdf:looztra/asdf-auto-doc](https://github.com/looztra/asdf-auto-doc) |
| aws-amplify | [ubi:aws-amplify/amplify-cli](https://github.com/aws-amplify/amplify-cli) [asdf:LozanoMatheus/asdf-aws-amplify-cli](https://github.com/LozanoMatheus/asdf-aws-amplify-cli) |
| aws-cli | [aqua:aws/aws-cli](https://github.com/aws/aws-cli) [asdf:MetricMike/asdf-awscli](https://github.com/MetricMike/asdf-awscli) |
| aws-copilot | [aqua:aws/copilot-cli](https://github.com/aws/copilot-cli) [asdf:NeoHsu/asdf-copilot](https://github.com/NeoHsu/asdf-copilot) |
| aws-iam-authenticator | [aqua:kubernetes-sigs/aws-iam-authenticator](https://github.com/kubernetes-sigs/aws-iam-authenticator) [asdf:zekker6/asdf-aws-iam-authenticator](https://github.com/zekker6/asdf-aws-iam-authenticator) |
| aws-nuke | [aqua:rebuy-de/aws-nuke](https://github.com/rebuy-de/aws-nuke) [asdf:bersalazar/asdf-aws-nuke](https://github.com/bersalazar/asdf-aws-nuke) |
| aws-sam | [pipx:aws-sam-cli](https://pypi.org/project/aws-sam-cli) [asdf:amrox/asdf-pyapp](https://github.com/amrox/asdf-pyapp) |
| aws-sso | [aqua:synfinatic/aws-sso-cli](https://github.com/synfinatic/aws-sso-cli) [asdf:adamcrews/asdf-aws-sso-cli](https://github.com/adamcrews/asdf-aws-sso-cli) |
| aws-vault | [aqua:99designs/aws-vault](https://github.com/99designs/aws-vault) [asdf:karancode/asdf-aws-vault](https://github.com/karancode/asdf-aws-vault) |
| awscli-local | [asdf:paulo-ferraz-oliveira/asdf-awscli-local](https://github.com/paulo-ferraz-oliveira/asdf-awscli-local) |
| awsebcli | [pipx:awsebcli](https://pypi.org/project/awsebcli) [asdf:amrox/asdf-pyapp](https://github.com/amrox/asdf-pyapp) |
| awsls | [ubi:jckuester/awsls](https://github.com/jckuester/awsls) [asdf:chessmango/asdf-awsls](https://github.com/chessmango/asdf-awsls) |
| awsrm | [ubi:jckuester/awsrm](https://github.com/jckuester/awsrm) [asdf:chessmango/asdf-awsrm](https://github.com/chessmango/asdf-awsrm) |
| awsweeper | [ubi:jckuester/awsweeper](https://github.com/jckuester/awsweeper) [asdf:chessmango/asdf-awsweeper](https://github.com/chessmango/asdf-awsweeper) |
| azure | [asdf:EcoMind/asdf-azure-cli](https://github.com/EcoMind/asdf-azure-cli) |
| azure-functions-core-tools | [asdf:daveneeley/asdf-azure-functions-core-tools](https://github.com/daveneeley/asdf-azure-functions-core-tools) |
| babashka | [ubi:babashka/babashka](https://github.com/babashka/babashka) [asdf:pitch-io/asdf-babashka](https://github.com/pitch-io/asdf-babashka) |
| balena | [ubi:balena-io/balena-cli](https://github.com/balena-io/balena-cli) [asdf:boatkit-io/asdf-balena-cli](https://github.com/boatkit-io/asdf-balena-cli) |
| bashbot | [aqua:mathew-fleisch/bashbot](https://github.com/mathew-fleisch/bashbot) [asdf:mathew-fleisch/asdf-bashbot](https://github.com/mathew-fleisch/asdf-bashbot) |
| bashly | [asdf:pcrockett/asdf-bashly](https://github.com/pcrockett/asdf-bashly) |
| bat | [aqua:sharkdp/bat](https://github.com/sharkdp/bat) [ubi:sharkdp/bat](https://github.com/sharkdp/bat) [asdf:https://gitlab.com/wt0f/asdf-bat](https://gitlab.com/wt0f/asdf-bat) |
| bat-extras | [asdf:vhdirk/asdf-bat-extras](https://github.com/vhdirk/asdf-bat-extras) |
| bats | [aqua:bats-core/bats-core](https://github.com/bats-core/bats-core) [asdf:timgluz/asdf-bats](https://github.com/timgluz/asdf-bats) |
| bazel | [ubi:bazelbuild/bazel](https://github.com/bazelbuild/bazel) [asdf:rajatvig/asdf-bazel](https://github.com/rajatvig/asdf-bazel) |
| bazelisk | [aqua:bazelbuild/bazelisk](https://github.com/bazelbuild/bazelisk) [asdf:josephtate/asdf-bazelisk](https://github.com/josephtate/asdf-bazelisk) |
| bbr-s3-config-validator | [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| benthos | [asdf:benthosdev/benthos-asdf](https://github.com/benthosdev/benthos-asdf) |
| bfs | [asdf:virtualroot/asdf-bfs](https://github.com/virtualroot/asdf-bfs) |
| binnacle | [aqua:Traackr/binnacle](https://github.com/Traackr/binnacle) [asdf:Traackr/asdf-binnacle](https://github.com/Traackr/asdf-binnacle) |
| bitwarden | [aqua:bitwarden/clients](https://github.com/bitwarden/clients) [asdf:vixus0/asdf-bitwarden](https://github.com/vixus0/asdf-bitwarden) |
| bitwarden-secrets-manager | [ubi:bitwarden/sdk](https://github.com/bitwarden/sdk) [asdf:asdf-community/asdf-bitwarden-secrets-manager](https://github.com/asdf-community/asdf-bitwarden-secrets-manager) |
| black | [aqua:psf/black](https://github.com/psf/black) |
| bombardier | [aqua:codesenberg/bombardier](https://github.com/codesenberg/bombardier) [asdf:NeoHsu/asdf-bombardier](https://github.com/NeoHsu/asdf-bombardier) |
| borg | [aqua:borgbackup/borg](https://github.com/borgbackup/borg) [asdf:lwiechec/asdf-borg](https://github.com/lwiechec/asdf-borg) |
| bosh | [aqua:cloudfoundry/bosh-cli](https://github.com/cloudfoundry/bosh-cli) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| bosh-backup-and-restore | [ubi:cloudfoundry-incubator/bosh-backup-and-restore](https://github.com/cloudfoundry-incubator/bosh-backup-and-restore) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| bottom | [aqua:ClementTsang/bottom](https://github.com/ClementTsang/bottom) [asdf:carbonteq/asdf-btm](https://github.com/carbonteq/asdf-btm) |
| boundary | [aqua:hashicorp/boundary](https://github.com/hashicorp/boundary) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| bpkg | [asdf:bpkg/asdf-bpkg](https://github.com/bpkg/asdf-bpkg) |
| brig | [aqua:brigadecore/brigade](https://github.com/brigadecore/brigade) [asdf:Ibotta/asdf-brig](https://github.com/Ibotta/asdf-brig) |
| btrace | [asdf:joschi/asdf-btrace](https://github.com/joschi/asdf-btrace) |
| buf | [aqua:bufbuild/buf](https://github.com/bufbuild/buf) [ubi:bufbuild/buf](https://github.com/bufbuild/buf) [asdf:truepay/asdf-buf](https://github.com/truepay/asdf-buf) |
| buildpack | [aqua:buildpacks/pack](https://github.com/buildpacks/pack) [asdf:johnlayton/asdf-buildpack](https://github.com/johnlayton/asdf-buildpack) |
| bun | [core:bun](https://mise.jdx.dev/lang/bun.html) |
| cabal | [aqua:haskell/cabal/cabal-install](https://github.com/haskell/cabal/cabal-install) |
| caddy | [aqua:caddyserver/caddy](https://github.com/caddyserver/caddy) [asdf:salasrod/asdf-caddy](https://github.com/salasrod/asdf-caddy) |
| calendarsync | [ubi:inovex/CalendarSync](https://github.com/inovex/CalendarSync) [asdf:FeryET/asdf-calendarsync](https://github.com/FeryET/asdf-calendarsync) |
| calicoctl | [aqua:projectcalico/calico/calicoctl](https://github.com/projectcalico/calico/calicoctl) [asdf:TheCubicleJockey/asdf-calicoctl](https://github.com/TheCubicleJockey/asdf-calicoctl) |
| cargo-binstall | [aqua:cargo-bins/cargo-binstall](https://github.com/cargo-bins/cargo-binstall) [ubi:cargo-bins/cargo-binstall](https://github.com/cargo-bins/cargo-binstall) [cargo:cargo-binstall](https://crates.io/crates/cargo-binstall) |
| cargo-insta | [ubi:mitsuhiko/insta](https://github.com/mitsuhiko/insta) |
| cargo-make | [ubi:sagiegurari/cargo-make](https://github.com/sagiegurari/cargo-make) [asdf:mise-plugins/asdf-cargo-make](https://github.com/mise-plugins/asdf-cargo-make) |
| carp | [ubi:carp-lang/Carp](https://github.com/carp-lang/Carp) [asdf:susurri/asdf-carp](https://github.com/susurri/asdf-carp) |
| carthage | [asdf:younke/asdf-carthage](https://github.com/younke/asdf-carthage) |
| ccache | [ubi:ccache/ccache](https://github.com/ccache/ccache) [asdf:asdf-community/asdf-ccache](https://github.com/asdf-community/asdf-ccache) |
| certstrap | [ubi:square/certstrap](https://github.com/square/certstrap) [asdf:carnei-ro/asdf-certstrap](https://github.com/carnei-ro/asdf-certstrap) |
| cf | [asdf:mattysweeps/asdf-cf](https://github.com/mattysweeps/asdf-cf) |
| cfssl | [aqua:cloudflare/cfssl/cfssl](https://github.com/cloudflare/cfssl/cfssl) [asdf:mathew-fleisch/asdf-cfssl](https://github.com/mathew-fleisch/asdf-cfssl) |
| chamber | [ubi:segmentio/chamber](https://github.com/segmentio/chamber) [asdf:mintel/asdf-chamber](https://github.com/mintel/asdf-chamber) |
| changie | [ubi:miniscruff/changie](https://github.com/miniscruff/changie) [asdf:pdemagny/asdf-changie](https://github.com/pdemagny/asdf-changie) |
| cheat | [aqua:cheat/cheat](https://github.com/cheat/cheat) [asdf:jmoratilla/asdf-cheat-plugin](https://github.com/jmoratilla/asdf-cheat-plugin) |
| checkov | [ubi:bridgecrewio/checkov](https://github.com/bridgecrewio/checkov) [asdf:bosmak/asdf-checkov](https://github.com/bosmak/asdf-checkov) |
| chezmoi | [ubi:twpayne/chezmoi](https://github.com/twpayne/chezmoi) [asdf:joke/asdf-chezmoi](https://github.com/joke/asdf-chezmoi) |
| chezscheme | [asdf:asdf-community/asdf-chezscheme](https://github.com/asdf-community/asdf-chezscheme) |
| chicken | [asdf:evhan/asdf-chicken](https://github.com/evhan/asdf-chicken) |
| chisel | [ubi:jpillora/chisel](https://github.com/jpillora/chisel) [go:github.com/jpillora/chisel](https://pkg.go.dev/github.com/jpillora/chisel) [asdf:lwiechec/asdf-chisel](https://github.com/lwiechec/asdf-chisel) |
| choose | [ubi:theryangeary/choose](https://github.com/theryangeary/choose) [cargo:choose](https://crates.io/crates/choose) [asdf:carbonteq/asdf-choose](https://github.com/carbonteq/asdf-choose) |
| chromedriver | [asdf:schinckel/asdf-chromedriver](https://github.com/schinckel/asdf-chromedriver) |
| cidr-merger | [ubi:zhanhb/cidr-merger](https://github.com/zhanhb/cidr-merger) [asdf:ORCID/asdf-cidr-merger](https://github.com/ORCID/asdf-cidr-merger) |
| cidrchk | [ubi:mhausenblas/cidrchk](https://github.com/mhausenblas/cidrchk) [asdf:ORCID/asdf-cidrchk](https://github.com/ORCID/asdf-cidrchk) |
| cilium-cli | [ubi:cilium/cilium-cli](https://github.com/cilium/cilium-cli) [asdf:carnei-ro/asdf-cilium-cli](https://github.com/carnei-ro/asdf-cilium-cli) |
| cilium-hubble | [ubi:cilium/hubble](https://github.com/cilium/hubble) [asdf:NitriKx/asdf-cilium-hubble](https://github.com/NitriKx/asdf-cilium-hubble) |
| circleci | [ubi:CircleCI-Public/circleci-cli](https://github.com/CircleCI-Public/circleci-cli) [asdf:ucpr/asdf-circleci-cli](https://github.com/ucpr/asdf-circleci-cli) |
| clang | [asdf:higebu/asdf-llvm](https://github.com/higebu/asdf-llvm) [vfox:jdx/vfox-clang](https://github.com/jdx/vfox-clang) |
| clang-format | [asdf:higebu/asdf-llvm](https://github.com/higebu/asdf-llvm) |
| clangd | [asdf:higebu/asdf-llvm](https://github.com/higebu/asdf-llvm) |
| clarinet | [ubi:hirosystems/clarinet](https://github.com/hirosystems/clarinet) [asdf:alexgo-io/asdf-clarinet](https://github.com/alexgo-io/asdf-clarinet) |
| clickhouse | [asdf:tinybirdco/asdf-clickhouse](https://github.com/tinybirdco/asdf-clickhouse) |
| clj-kondo | [ubi:clj-kondo/clj-kondo](https://github.com/clj-kondo/clj-kondo) [asdf:rynkowsg/asdf-clj-kondo](https://github.com/rynkowsg/asdf-clj-kondo) |
| cljstyle | [ubi:greglook/cljstyle](https://github.com/greglook/cljstyle) [asdf:abogoyavlensky/asdf-cljstyle](https://github.com/abogoyavlensky/asdf-cljstyle) |
| clojure | [asdf:asdf-community/asdf-clojure](https://github.com/asdf-community/asdf-clojure) |
| cloud-sql-proxy | [aqua:GoogleCloudPlatform/cloud-sql-proxy](https://github.com/GoogleCloudPlatform/cloud-sql-proxy) [asdf:pbr0ck3r/asdf-cloud-sql-proxy](https://github.com/pbr0ck3r/asdf-cloud-sql-proxy) |
| cloudflared | [aqua:cloudflare/cloudflared](https://github.com/cloudflare/cloudflared) [asdf:threkk/asdf-cloudflared](https://github.com/threkk/asdf-cloudflared) |
| clusterawsadm | [ubi:kubernetes-sigs/cluster-api-provider-aws](https://github.com/kubernetes-sigs/cluster-api-provider-aws) [asdf:kahun/asdf-clusterawsadm](https://github.com/kahun/asdf-clusterawsadm) |
| clusterctl | [aqua:kubernetes-sigs/cluster-api](https://github.com/kubernetes-sigs/cluster-api) [asdf:pfnet-research/asdf-clusterctl](https://github.com/pfnet-research/asdf-clusterctl) |
| cmake | [asdf:asdf-community/asdf-cmake](https://github.com/asdf-community/asdf-cmake) [vfox:version-fox/vfox-cmake](https://github.com/version-fox/vfox-cmake) |
| cmctl | [aqua:cert-manager/cmctl](https://github.com/cert-manager/cmctl) [asdf:asdf-community/asdf-cmctl](https://github.com/asdf-community/asdf-cmctl) |
| cockroach | [aqua:cockroachdb/cockroach](https://github.com/cockroachdb/cockroach) [asdf:salasrod/asdf-cockroach](https://github.com/salasrod/asdf-cockroach) |
| cocoapods | [asdf:ronnnnn/asdf-cocoapods](https://github.com/ronnnnn/asdf-cocoapods) |
| codefresh | [ubi:codefresh-io/cli](https://github.com/codefresh-io/cli) [asdf:gurukulkarni/asdf-codefresh](https://github.com/gurukulkarni/asdf-codefresh) |
| codeql | [asdf:bored-engineer/asdf-codeql](https://github.com/bored-engineer/asdf-codeql) |
| coder | [aqua:coder/coder](https://github.com/coder/coder) [asdf:mise-plugins/asdf-coder](https://github.com/mise-plugins/asdf-coder) |
| colima | [ubi:abiosoft/colima](https://github.com/abiosoft/colima) [asdf:CrouchingMuppet/asdf-colima](https://github.com/CrouchingMuppet/asdf-colima) |
| committed | [aqua:crate-ci/committed](https://github.com/crate-ci/committed) |
| conan | [pipx:conan](https://pypi.org/project/conan) [asdf:amrox/asdf-pyapp](https://github.com/amrox/asdf-pyapp) |
| concourse | [aqua:concourse/concourse/concourse](https://github.com/concourse/concourse/concourse) [asdf:mattysweeps/asdf-concourse](https://github.com/mattysweeps/asdf-concourse) |
| conduit | [ubi:ConduitIO/conduit](https://github.com/ConduitIO/conduit) [asdf:gmcabrita/asdf-conduit](https://github.com/gmcabrita/asdf-conduit) |
| conform | [aqua:siderolabs/conform](https://github.com/siderolabs/conform) [asdf:skyzyx/asdf-conform](https://github.com/skyzyx/asdf-conform) |
| conftest | [aqua:open-policy-agent/conftest](https://github.com/open-policy-agent/conftest) [asdf:looztra/asdf-conftest](https://github.com/looztra/asdf-conftest) |
| consul | [aqua:hashicorp/consul](https://github.com/hashicorp/consul) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| container-structure-test | [aqua:GoogleContainerTools/container-structure-test](https://github.com/GoogleContainerTools/container-structure-test) [asdf:FeryET/asdf-container-structure-test](https://github.com/FeryET/asdf-container-structure-test) |
| cookiecutter | [pipx:cookiecutter](https://pypi.org/project/cookiecutter) [asdf:shawon-crosen/asdf-cookiecutter](https://github.com/shawon-crosen/asdf-cookiecutter) |
| copper | [ubi:cloud66-oss/copper](https://github.com/cloud66-oss/copper) [asdf:vladlosev/asdf-copper](https://github.com/vladlosev/asdf-copper) |
| coq | [asdf:gingerhot/asdf-coq](https://github.com/gingerhot/asdf-coq) |
| coredns | [ubi:coredns/coredns](https://github.com/coredns/coredns) [asdf:s3than/asdf-coredns](https://github.com/s3than/asdf-coredns) |
| cosign | [aqua:sigstore/cosign](https://github.com/sigstore/cosign) [asdf:https://gitlab.com/wt0f/asdf-cosign](https://gitlab.com/wt0f/asdf-cosign) |
| coursier | [ubi:coursier/coursier](https://github.com/coursier/coursier) [asdf:jiahuili430/asdf-coursier](https://github.com/jiahuili430/asdf-coursier) |
| cowsay | [npm:cowsay](https://www.npmjs.com/package/cowsay) |
| crane | [ubi:google/go-containerregistry](https://github.com/google/go-containerregistry) [asdf:dmpe/asdf-crane](https://github.com/dmpe/asdf-crane) |
| crc | [asdf:sqtran/asdf-crc](https://github.com/sqtran/asdf-crc) |
| credhub | [aqua:cloudfoundry/credhub-cli](https://github.com/cloudfoundry/credhub-cli) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| crictl | [aqua:kubernetes-sigs/cri-tools/crictl](https://github.com/kubernetes-sigs/cri-tools/crictl) [asdf:FairwindsOps/asdf-crictl](https://github.com/FairwindsOps/asdf-crictl) |
| crossplane | [aqua:crossplane/crossplane](https://github.com/crossplane/crossplane) [asdf:joke/asdf-crossplane-cli](https://github.com/joke/asdf-crossplane-cli) |
| crystal | [asdf:asdf-community/asdf-crystal](https://github.com/asdf-community/asdf-crystal) [vfox:yanecc/vfox-crystal](https://github.com/yanecc/vfox-crystal) |
| ctlptl | [aqua:tilt-dev/ctlptl](https://github.com/tilt-dev/ctlptl) [asdf:ezcater/asdf-ctlptl](https://github.com/ezcater/asdf-ctlptl) |
| ctop | [ubi:bcicen/ctop](https://github.com/bcicen/ctop) [asdf:NeoHsu/asdf-ctop](https://github.com/NeoHsu/asdf-ctop) |
| cue | [aqua:cue-lang/cue](https://github.com/cue-lang/cue) [asdf:asdf-community/asdf-cue](https://github.com/asdf-community/asdf-cue) |
| cyclonedx | [aqua:CycloneDX/cyclonedx-cli](https://github.com/CycloneDX/cyclonedx-cli) [asdf:xeedio/asdf-cyclonedx](https://github.com/xeedio/asdf-cyclonedx) |
| dagger | [aqua:dagger/dagger](https://github.com/dagger/dagger) [asdf:virtualstaticvoid/asdf-dagger](https://github.com/virtualstaticvoid/asdf-dagger) |
| danger-js | [asdf:MontakOleg/asdf-danger-js](https://github.com/MontakOleg/asdf-danger-js) |
| danger-swift | [spm:danger/swift](https://github.com/danger/swift) |
| dapr | [aqua:dapr/cli](https://github.com/dapr/cli) [asdf:asdf-community/asdf-dapr-cli](https://github.com/asdf-community/asdf-dapr-cli) |
| dart | [asdf:PatOConnor43/asdf-dart](https://github.com/PatOConnor43/asdf-dart) [vfox:version-fox/vfox-dart](https://github.com/version-fox/vfox-dart) |
| dasel | [aqua:TomWright/dasel](https://github.com/TomWright/dasel) [asdf:asdf-community/asdf-dasel](https://github.com/asdf-community/asdf-dasel) |
| datree | [aqua:datreeio/datree](https://github.com/datreeio/datree) [asdf:lukeab/asdf-datree](https://github.com/lukeab/asdf-datree) |
| daytona | [asdf:CrouchingMuppet/asdf-daytona](https://github.com/CrouchingMuppet/asdf-daytona) |
| dbmate | [aqua:amacneil/dbmate](https://github.com/amacneil/dbmate) [asdf:juusujanar/asdf-dbmate](https://github.com/juusujanar/asdf-dbmate) |
| deck | [aqua:Kong/deck](https://github.com/Kong/deck) [asdf:nutellinoit/asdf-deck](https://github.com/nutellinoit/asdf-deck) |
| delta | [ubi:dandavison/delta](https://github.com/dandavison/delta) [asdf:andweeb/asdf-delta](https://github.com/andweeb/asdf-delta) |
| deno | [core:deno](https://mise.jdx.dev/lang/deno.html) |
| depot | [ubi:depot/cli](https://github.com/depot/cli) [asdf:depot/asdf-depot](https://github.com/depot/asdf-depot) |
| desk | [aqua:jamesob/desk](https://github.com/jamesob/desk) [asdf:endorama/asdf-desk](https://github.com/endorama/asdf-desk) |
| devspace | [aqua:devspace-sh/devspace](https://github.com/devspace-sh/devspace) [asdf:NeoHsu/asdf-devspace](https://github.com/NeoHsu/asdf-devspace) |
| dhall | [asdf:aaaaninja/asdf-dhall](https://github.com/aaaaninja/asdf-dhall) |
| difftastic | [ubi:wilfred/difftastic](https://github.com/wilfred/difftastic) [asdf:volf52/asdf-difftastic](https://github.com/volf52/asdf-difftastic) |
| digdag | [asdf:jtakakura/asdf-digdag](https://github.com/jtakakura/asdf-digdag) |
| direnv | [aqua:direnv/direnv](https://github.com/direnv/direnv) [asdf:asdf-community/asdf-direnv](https://github.com/asdf-community/asdf-direnv) |
| dive | [ubi:wagoodman/dive](https://github.com/wagoodman/dive) [asdf:looztra/asdf-dive](https://github.com/looztra/asdf-dive) |
| djinni | [ubi:cross-language-cpp/djinni-generator](https://github.com/cross-language-cpp/djinni-generator) [asdf:cross-language-cpp/asdf-djinni](https://github.com/cross-language-cpp/asdf-djinni) |
| dmd | [asdf:sylph01/asdf-dmd](https://github.com/sylph01/asdf-dmd) |
| docker-compose | [aqua:docker/compose](https://github.com/docker/compose) |
| docker-slim | [ubi:slimtoolkit/slim](https://github.com/slimtoolkit/slim) [asdf:xataz/asdf-docker-slim](https://github.com/xataz/asdf-docker-slim) |
| dockle | [aqua:goodwithtech/dockle](https://github.com/goodwithtech/dockle) [asdf:mathew-fleisch/asdf-dockle](https://github.com/mathew-fleisch/asdf-dockle) |
| doctl | [ubi:digitalocean/doctl](https://github.com/digitalocean/doctl) [asdf:maristgeek/asdf-doctl](https://github.com/maristgeek/asdf-doctl) |
| doctoolchain | [asdf:joschi/asdf-doctoolchain](https://github.com/joschi/asdf-doctoolchain) |
| docuum | [ubi:stepchowfun/docuum](https://github.com/stepchowfun/docuum) [cargo:docuum](https://crates.io/crates/docuum) [asdf:bradym/asdf-docuum](https://github.com/bradym/asdf-docuum) |
| dome | [asdf:jtakakura/asdf-dome](https://github.com/jtakakura/asdf-dome) |
| doppler | [ubi:DopplerHQ/cli](https://github.com/DopplerHQ/cli) [asdf:takutakahashi/asdf-doppler](https://github.com/takutakahashi/asdf-doppler) |
| dotenv-linter | [ubi:dotenv-linter/dotenv-linter](https://github.com/dotenv-linter/dotenv-linter) [asdf:wesleimp/asdf-dotenv-linter](https://github.com/wesleimp/asdf-dotenv-linter) |
| dotnet | [asdf:hensou/asdf-dotnet](https://github.com/hensou/asdf-dotnet) [vfox:version-fox/vfox-dotnet](https://github.com/version-fox/vfox-dotnet) |
| dotnet-core | [asdf:emersonsoares/asdf-dotnet-core](https://github.com/emersonsoares/asdf-dotnet-core) |
| dotty | [asdf:asdf-community/asdf-dotty](https://github.com/asdf-community/asdf-dotty) |
| dprint | [aqua:dprint/dprint](https://github.com/dprint/dprint) [asdf:asdf-community/asdf-dprint](https://github.com/asdf-community/asdf-dprint) |
| draft | [aqua:Azure/draft](https://github.com/Azure/draft) [asdf:kristoflemmens/asdf-draft](https://github.com/kristoflemmens/asdf-draft) |
| driftctl | [aqua:snyk/driftctl](https://github.com/snyk/driftctl) [asdf:nlamirault/asdf-driftctl](https://github.com/nlamirault/asdf-driftctl) |
| drone | [ubi:harness/drone-cli](https://github.com/harness/drone-cli) [asdf:virtualstaticvoid/asdf-drone](https://github.com/virtualstaticvoid/asdf-drone) |
| dt | [aqua:so-dang-cool/dt](https://github.com/so-dang-cool/dt) [asdf:so-dang-cool/asdf-dt](https://github.com/so-dang-cool/asdf-dt) |
| dtm | [ubi:devstream-io/devstream](https://github.com/devstream-io/devstream) [asdf:zhenyuanlau/asdf-dtm](https://github.com/zhenyuanlau/asdf-dtm) |
| duf | [aqua:muesli/duf](https://github.com/muesli/duf) [asdf:NeoHsu/asdf-duf](https://github.com/NeoHsu/asdf-duf) |
| dust | [ubi:bootandy/dust](https://github.com/bootandy/dust) [asdf:looztra/asdf-dust](https://github.com/looztra/asdf-dust) |
| dvc | [asdf:fwfurtado/asdf-dvc](https://github.com/fwfurtado/asdf-dvc) |
| dyff | [aqua:homeport/dyff](https://github.com/homeport/dyff) [asdf:https://gitlab.com/wt0f/asdf-dyff](https://gitlab.com/wt0f/asdf-dyff) |
| dynatrace-monaco | [ubi:Dynatrace/dynatrace-configuration-as-code](https://github.com/Dynatrace/dynatrace-configuration-as-code) [asdf:nsaputro/asdf-monaco](https://github.com/nsaputro/asdf-monaco) |
| earthly | [aqua:earthly/earthly](https://github.com/earthly/earthly) [asdf:YR-ZR0/asdf-earthly](https://github.com/YR-ZR0/asdf-earthly) |
| ecspresso | [aqua:kayac/ecspresso](https://github.com/kayac/ecspresso) [asdf:kayac/asdf-ecspresso](https://github.com/kayac/asdf-ecspresso) |
| editorconfig-checker | [aqua:editorconfig-checker/editorconfig-checker](https://github.com/editorconfig-checker/editorconfig-checker) [asdf:gabitchov/asdf-editorconfig-checker](https://github.com/gabitchov/asdf-editorconfig-checker) |
| ejson | [aqua:Shopify/ejson](https://github.com/Shopify/ejson) [asdf:cipherstash/asdf-ejson](https://github.com/cipherstash/asdf-ejson) |
| eksctl | [aqua:eksctl-io/eksctl](https://github.com/eksctl-io/eksctl) [asdf:elementalvoid/asdf-eksctl](https://github.com/elementalvoid/asdf-eksctl) |
| elasticsearch | [asdf:asdf-community/asdf-elasticsearch](https://github.com/asdf-community/asdf-elasticsearch) |
| elixir | [asdf:mise-plugins/mise-elixir](https://github.com/mise-plugins/mise-elixir) [vfox:version-fox/vfox-elixir](https://github.com/version-fox/vfox-elixir) |
| elixir-ls | [asdf:juantascon/asdf-elixir-ls](https://github.com/juantascon/asdf-elixir-ls) |
| elm | [ubi:elm/compiler](https://github.com/elm/compiler) [asdf:asdf-community/asdf-elm](https://github.com/asdf-community/asdf-elm) |
| emsdk | [asdf:RobLoach/asdf-emsdk](https://github.com/RobLoach/asdf-emsdk) |
| envcli | [ubi:EnvCLI/EnvCLI](https://github.com/EnvCLI/EnvCLI) [asdf:zekker6/asdf-envcli](https://github.com/zekker6/asdf-envcli) |
| envsubst | [aqua:a8m/envsubst](https://github.com/a8m/envsubst) [asdf:dex4er/asdf-envsubst](https://github.com/dex4er/asdf-envsubst) |
| ephemeral-postgres | [asdf:smashedtoatoms/asdf-ephemeral-postgres](https://github.com/smashedtoatoms/asdf-ephemeral-postgres) |
| erlang | [core:erlang](https://mise.jdx.dev/lang/erlang.html) |
| esc | [ubi:pulumi/esc](https://github.com/pulumi/esc) [asdf:fxsalazar/asdf-esc](https://github.com/fxsalazar/asdf-esc) |
| esy | [asdf:asdf-community/asdf-esy](https://github.com/asdf-community/asdf-esy) |
| etcd | [aqua:etcd-io/etcd](https://github.com/etcd-io/etcd) [asdf:particledecay/asdf-etcd](https://github.com/particledecay/asdf-etcd) [vfox:version-fox/vfox-etcd](https://github.com/version-fox/vfox-etcd) |
| evans | [aqua:ktr0731/evans](https://github.com/ktr0731/evans) [asdf:goki90210/asdf-evans](https://github.com/goki90210/asdf-evans) |
| eza | [asdf:lwiechec/asdf-eza](https://github.com/lwiechec/asdf-eza) |
| fd | [aqua:sharkdp/fd](https://github.com/sharkdp/fd) [ubi:sharkdp/fd](https://github.com/sharkdp/fd) [asdf:https://gitlab.com/wt0f/asdf-fd](https://gitlab.com/wt0f/asdf-fd) |
| ffmpeg | [asdf:acj/asdf-ffmpeg](https://github.com/acj/asdf-ffmpeg) |
| figma-export | [ubi:RedMadRobot/figma-export](https://github.com/RedMadRobot/figma-export) [asdf:younke/asdf-figma-export](https://github.com/younke/asdf-figma-export) |
| fillin | [aqua:itchyny/fillin](https://github.com/itchyny/fillin) [asdf:ouest/asdf-fillin](https://github.com/ouest/asdf-fillin) |
| firebase | [aqua:firebase/firebase-tools](https://github.com/firebase/firebase-tools) [asdf:jthegedus/asdf-firebase](https://github.com/jthegedus/asdf-firebase) |
| fission | [aqua:fission/fission](https://github.com/fission/fission) [asdf:virtualstaticvoid/asdf-fission](https://github.com/virtualstaticvoid/asdf-fission) |
| flamingo | [ubi:flux-subsystem-argo/flamingo](https://github.com/flux-subsystem-argo/flamingo) [asdf:log2/asdf-flamingo](https://github.com/log2/asdf-flamingo) |
| flarectl | [ubi:cloudflare/cloudflare-go](https://github.com/cloudflare/cloudflare-go) [asdf:mise-plugins/asdf-flarectl](https://github.com/mise-plugins/asdf-flarectl) |
| flatc | [ubi:google/flatbuffers](https://github.com/google/flatbuffers) [asdf:TheOpenDictionary/asdf-flatc](https://github.com/TheOpenDictionary/asdf-flatc) |
| flutter | [asdf:oae/asdf-flutter](https://github.com/oae/asdf-flutter) [vfox:version-fox/vfox-flutter](https://github.com/version-fox/vfox-flutter) |
| fluttergen | [ubi:FlutterGen/flutter_gen](https://github.com/FlutterGen/flutter_gen) [asdf:FlutterGen/asdf-fluttergen](https://github.com/FlutterGen/asdf-fluttergen) |
| flux2 | [aqua:fluxcd/flux2](https://github.com/fluxcd/flux2) [asdf:tablexi/asdf-flux2](https://github.com/tablexi/asdf-flux2) |
| fly | [aqua:concourse/concourse/fly](https://github.com/concourse/concourse/fly) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| flyctl | [aqua:superfly/flyctl](https://github.com/superfly/flyctl) [ubi:superfly/flyctl](https://github.com/superfly/flyctl) [asdf:chessmango/asdf-flyctl](https://github.com/chessmango/asdf-flyctl) |
| flyway | [asdf:junminahn/asdf-flyway](https://github.com/junminahn/asdf-flyway) |
| func-e | [asdf:carnei-ro/asdf-func-e](https://github.com/carnei-ro/asdf-func-e) |
| furyctl | [ubi:sighupio/furyctl](https://github.com/sighupio/furyctl) [asdf:sighupio/asdf-furyctl](https://github.com/sighupio/asdf-furyctl) |
| fx | [aqua:antonmedv/fx](https://github.com/antonmedv/fx) [asdf:https://gitlab.com/wt0f/asdf-fx](https://gitlab.com/wt0f/asdf-fx) |
| fzf | [aqua:junegunn/fzf](https://github.com/junegunn/fzf) [ubi:junegunn/fzf](https://github.com/junegunn/fzf) [asdf:kompiro/asdf-fzf](https://github.com/kompiro/asdf-fzf) |
| gallery-dl | [asdf:iul1an/asdf-gallery-dl](https://github.com/iul1an/asdf-gallery-dl) |
| gam | [ubi:GAM-team/GAM](https://github.com/GAM-team/GAM) [asdf:offbyone/asdf-gam](https://github.com/offbyone/asdf-gam) |
| gator | [ubi:open-policy-agent/gatekeeper](https://github.com/open-policy-agent/gatekeeper) [asdf:MxNxPx/asdf-gator](https://github.com/MxNxPx/asdf-gator) |
| gauche | [asdf:sakuro/asdf-gauche](https://github.com/sakuro/asdf-gauche) |
| gcc-arm-none-eabi | [asdf:dlech/asdf-gcc-arm-none-eabi](https://github.com/dlech/asdf-gcc-arm-none-eabi) |
| gcloud | [asdf:jthegedus/asdf-gcloud](https://github.com/jthegedus/asdf-gcloud) |
| getenvoy | [asdf:asdf-community/asdf-getenvoy](https://github.com/asdf-community/asdf-getenvoy) |
| ghc | [asdf:sestrella/asdf-ghcup](https://github.com/sestrella/asdf-ghcup) |
| ghcup | [ubi:haskell/ghcup-hs](https://github.com/haskell/ghcup-hs) [asdf:sestrella/asdf-ghcup](https://github.com/sestrella/asdf-ghcup) |
| ghidra | [asdf:Honeypot95/asdf-ghidra](https://github.com/Honeypot95/asdf-ghidra) |
| ghorg | [aqua:gabrie30/ghorg](https://github.com/gabrie30/ghorg) [asdf:gbloquel/asdf-ghorg](https://github.com/gbloquel/asdf-ghorg) |
| ghq | [aqua:x-motemen/ghq](https://github.com/x-motemen/ghq) [asdf:kajisha/asdf-ghq](https://github.com/kajisha/asdf-ghq) |
| ginkgo | [asdf:jimmidyson/asdf-ginkgo](https://github.com/jimmidyson/asdf-ginkgo) |
| git-chglog | [aqua:git-chglog/git-chglog](https://github.com/git-chglog/git-chglog) [asdf:GoodwayGroup/asdf-git-chglog](https://github.com/GoodwayGroup/asdf-git-chglog) |
| git-cliff | [aqua:orhun/git-cliff](https://github.com/orhun/git-cliff) [asdf:jylenhof/asdf-git-cliff](https://github.com/jylenhof/asdf-git-cliff) |
| gitconfig | [ubi:0ghny/gitconfig](https://github.com/0ghny/gitconfig) [asdf:0ghny/asdf-gitconfig](https://github.com/0ghny/asdf-gitconfig) |
| github-cli | [aqua:cli/cli](https://github.com/cli/cli) [ubi:cli/cli](https://github.com/cli/cli) [asdf:bartlomiejdanek/asdf-github-cli](https://github.com/bartlomiejdanek/asdf-github-cli) |
| github-markdown-toc | [aqua:ekalinin/github-markdown-toc](https://github.com/ekalinin/github-markdown-toc) [asdf:skyzyx/asdf-github-markdown-toc](https://github.com/skyzyx/asdf-github-markdown-toc) |
| gitleaks | [aqua:gitleaks/gitleaks](https://github.com/gitleaks/gitleaks) [asdf:jmcvetta/asdf-gitleaks](https://github.com/jmcvetta/asdf-gitleaks) |
| gitsign | [aqua:sigstore/gitsign](https://github.com/sigstore/gitsign) [asdf:spencergilbert/asdf-gitsign](https://github.com/spencergilbert/asdf-gitsign) |
| gitu | [ubi:altsem/gitu](https://github.com/altsem/gitu) [cargo:gitu](https://crates.io/crates/gitu) |
| gitui | [aqua:extrawurst/gitui](https://github.com/extrawurst/gitui) [asdf:looztra/asdf-gitui](https://github.com/looztra/asdf-gitui) |
| glab | [asdf:particledecay/asdf-glab](https://github.com/particledecay/asdf-glab) |
| gleam | [aqua:gleam-lang/gleam](https://github.com/gleam-lang/gleam) [asdf:asdf-community/asdf-gleam](https://github.com/asdf-community/asdf-gleam) |
| glen | [ubi:lingrino/glen](https://github.com/lingrino/glen) [asdf:bradym/asdf-glen](https://github.com/bradym/asdf-glen) |
| glooctl | [ubi:solo-io/gloo](https://github.com/solo-io/gloo) [asdf:halilkaya/asdf-glooctl](https://github.com/halilkaya/asdf-glooctl) |
| glow | [aqua:charmbracelet/glow](https://github.com/charmbracelet/glow) [asdf:mise-plugins/asdf-glow](https://github.com/mise-plugins/asdf-glow) |
| go | [core:go](https://mise.jdx.dev/lang/go.html) |
| go-containerregistry | [aqua:google/go-containerregistry](https://github.com/google/go-containerregistry) [asdf:dex4er/asdf-go-containerregistry](https://github.com/dex4er/asdf-go-containerregistry) |
| go-getter | [asdf:ryodocx/asdf-go-getter](https://github.com/ryodocx/asdf-go-getter) |
| go-jira | [aqua:go-jira/jira](https://github.com/go-jira/jira) [asdf:dguihal/asdf-go-jira](https://github.com/dguihal/asdf-go-jira) |
| go-jsonnet | [aqua:google/go-jsonnet](https://github.com/google/go-jsonnet) [asdf:https://gitlab.com/craigfurman/asdf-go-jsonnet](https://gitlab.com/craigfurman/asdf-go-jsonnet) |
| go-junit-report | [ubi:jstemmer/go-junit-report](https://github.com/jstemmer/go-junit-report) [asdf:jwillker/asdf-go-junit-report](https://github.com/jwillker/asdf-go-junit-report) |
| go-sdk | [asdf:yacchi/asdf-go-sdk](https://github.com/yacchi/asdf-go-sdk) |
| go-swagger | [aqua:go-swagger/go-swagger](https://github.com/go-swagger/go-swagger) [asdf:jfreeland/asdf-go-swagger](https://github.com/jfreeland/asdf-go-swagger) |
| goconvey | [asdf:therounds-contrib/asdf-goconvey](https://github.com/therounds-contrib/asdf-goconvey) |
| gocryptfs | [aqua:rfjakob/gocryptfs](https://github.com/rfjakob/gocryptfs) [ubi:rfjakob/gocryptfs](https://github.com/rfjakob/gocryptfs) |
| gofumpt | [ubi:mvdan/gofumpt](https://github.com/mvdan/gofumpt) [asdf:looztra/asdf-gofumpt](https://github.com/looztra/asdf-gofumpt) |
| gohugo | [ubi:gohugoio/hugo](https://github.com/gohugoio/hugo) [asdf:nklmilojevic/asdf-hugo](https://github.com/nklmilojevic/asdf-hugo) |
| gojq | [aqua:itchyny/gojq](https://github.com/itchyny/gojq) [asdf:jimmidyson/asdf-gojq](https://github.com/jimmidyson/asdf-gojq) |
| golangci-lint | [aqua:golangci/golangci-lint](https://github.com/golangci/golangci-lint) [ubi:golangci/golangci-lint](https://github.com/golangci/golangci-lint) [asdf:hypnoglow/asdf-golangci-lint](https://github.com/hypnoglow/asdf-golangci-lint) |
| golangci-lint-langserver | [ubi:nametake/golangci-lint-langserver](https://github.com/nametake/golangci-lint-langserver) [go:github.com/nametake/golangci-lint-langserver](https://pkg.go.dev/github.com/nametake/golangci-lint-langserver) |
| golines | [ubi:segmentio/golines](https://github.com/segmentio/golines) [go:github.com/segmentio/golines](https://pkg.go.dev/github.com/segmentio/golines) |
| gomigrate | [aqua:golang-migrate/migrate](https://github.com/golang-migrate/migrate) [asdf:joschi/asdf-gomigrate](https://github.com/joschi/asdf-gomigrate) |
| gomplate | [aqua:hairyhenderson/gomplate](https://github.com/hairyhenderson/gomplate) [asdf:sneakybeaky/asdf-gomplate](https://github.com/sneakybeaky/asdf-gomplate) |
| gopass | [aqua:gopasspw/gopass](https://github.com/gopasspw/gopass) [asdf:trallnag/asdf-gopass](https://github.com/trallnag/asdf-gopass) |
| goreleaser | [aqua:goreleaser/goreleaser](https://github.com/goreleaser/goreleaser) [ubi:goreleaser/goreleaser](https://github.com/goreleaser/goreleaser) [asdf:kforsthoevel/asdf-goreleaser](https://github.com/kforsthoevel/asdf-goreleaser) |
| goss | [aqua:goss-org/goss](https://github.com/goss-org/goss) [asdf:raimon49/asdf-goss](https://github.com/raimon49/asdf-goss) |
| gotestsum | [aqua:gotestyourself/gotestsum](https://github.com/gotestyourself/gotestsum) [asdf:pmalek/mise-gotestsum](https://github.com/pmalek/mise-gotestsum) |
| graalvm | [asdf:asdf-community/asdf-graalvm](https://github.com/asdf-community/asdf-graalvm) |
| gradle | [asdf:rfrancis/asdf-gradle](https://github.com/rfrancis/asdf-gradle) [vfox:version-fox/vfox-gradle](https://github.com/version-fox/vfox-gradle) |
| gradle-profiler | [asdf:joschi/asdf-gradle-profiler](https://github.com/joschi/asdf-gradle-profiler) |
| grails | [asdf:weibemoura/asdf-grails](https://github.com/weibemoura/asdf-grails) |
| grain | [asdf:cometkim/asdf-grain](https://github.com/cometkim/asdf-grain) |
| granted | [aqua:common-fate/granted](https://github.com/common-fate/granted) [asdf:dex4er/asdf-granted](https://github.com/dex4er/asdf-granted) |
| grex | [asdf:ouest/asdf-grex](https://github.com/ouest/asdf-grex) |
| groovy | [asdf:weibemoura/asdf-groovy](https://github.com/weibemoura/asdf-groovy) [vfox:version-fox/vfox-groovy](https://github.com/version-fox/vfox-groovy) |
| grpc-health-probe | [aqua:grpc-ecosystem/grpc-health-probe](https://github.com/grpc-ecosystem/grpc-health-probe) [asdf:zufardhiyaulhaq/asdf-grpc-health-probe](https://github.com/zufardhiyaulhaq/asdf-grpc-health-probe) |
| grpcurl | [aqua:fullstorydev/grpcurl](https://github.com/fullstorydev/grpcurl) [asdf:asdf-community/asdf-grpcurl](https://github.com/asdf-community/asdf-grpcurl) |
| grype | [ubi:anchore/grype](https://github.com/anchore/grype) [asdf:poikilotherm/asdf-grype](https://github.com/poikilotherm/asdf-grype) |
| guile | [asdf:indiebrain/asdf-guile](https://github.com/indiebrain/asdf-guile) |
| gum | [aqua:charmbracelet/gum](https://github.com/charmbracelet/gum) [asdf:lwiechec/asdf-gum](https://github.com/lwiechec/asdf-gum) |
| gwvault | [aqua:GoodwayGroup/gwvault](https://github.com/GoodwayGroup/gwvault) [asdf:GoodwayGroup/asdf-gwvault](https://github.com/GoodwayGroup/asdf-gwvault) |
| hadolint | [ubi:hadolint/hadolint](https://github.com/hadolint/hadolint) [asdf:devlincashman/asdf-hadolint](https://github.com/devlincashman/asdf-hadolint) |
| hamler | [asdf:scudelletti/asdf-hamler](https://github.com/scudelletti/asdf-hamler) |
| has | [aqua:kdabir/has](https://github.com/kdabir/has) [asdf:sylvainmetayer/asdf-has](https://github.com/sylvainmetayer/asdf-has) |
| haskell | [asdf:asdf-community/asdf-haskell](https://github.com/asdf-community/asdf-haskell) |
| hasura-cli | [asdf:gurukulkarni/asdf-hasura](https://github.com/gurukulkarni/asdf-hasura) |
| haxe | [asdf:asdf-community/asdf-haxe](https://github.com/asdf-community/asdf-haxe) |
| hcl2json | [aqua:tmccombs/hcl2json](https://github.com/tmccombs/hcl2json) [asdf:dex4er/asdf-hcl2json](https://github.com/dex4er/asdf-hcl2json) |
| hcloud | [asdf:chessmango/asdf-hcloud](https://github.com/chessmango/asdf-hcloud) |
| helm | [aqua:helm/helm](https://github.com/helm/helm) [asdf:Antiarchitect/asdf-helm](https://github.com/Antiarchitect/asdf-helm) |
| helm-cr | [asdf:Antiarchitect/asdf-helm-cr](https://github.com/Antiarchitect/asdf-helm-cr) |
| helm-ct | [asdf:tablexi/asdf-helm-ct](https://github.com/tablexi/asdf-helm-ct) |
| helm-diff | [asdf:dex4er/asdf-helm-diff](https://github.com/dex4er/asdf-helm-diff) |
| helm-docs | [aqua:norwoodj/helm-docs](https://github.com/norwoodj/helm-docs) [asdf:sudermanjr/asdf-helm-docs](https://github.com/sudermanjr/asdf-helm-docs) |
| helmfile | [ubi:helmfile/helmfile](https://github.com/helmfile/helmfile) [asdf:feniix/asdf-helmfile](https://github.com/feniix/asdf-helmfile) |
| helmsman | [ubi:Praqma/helmsman](https://github.com/Praqma/helmsman) [asdf:luisdavim/asdf-helmsman](https://github.com/luisdavim/asdf-helmsman) |
| heroku | [asdf:mise-plugins/mise-heroku-cli](https://github.com/mise-plugins/mise-heroku-cli) |
| hey | [asdf:raimon49/asdf-hey](https://github.com/raimon49/asdf-hey) |
| hishtory | [asdf:asdf-community/asdf-hishtory](https://github.com/asdf-community/asdf-hishtory) |
| hivemind | [ubi:DarthSim/hivemind](https://github.com/DarthSim/hivemind) [go:github.com/DarthSim/hivemind](https://pkg.go.dev/github.com/DarthSim/hivemind) |
| hledger | [asdf:airtonix/asdf-hledger](https://github.com/airtonix/asdf-hledger) |
| hledger-flow | [asdf:airtonix/asdf-hledger-flow](https://github.com/airtonix/asdf-hledger-flow) |
| hls | [asdf:sestrella/asdf-ghcup](https://github.com/sestrella/asdf-ghcup) |
| hostctl | [aqua:guumaster/hostctl](https://github.com/guumaster/hostctl) [asdf:svenluijten/asdf-hostctl](https://github.com/svenluijten/asdf-hostctl) |
| httpie-go | [aqua:nojima/httpie-go](https://github.com/nojima/httpie-go) [asdf:abatilo/asdf-httpie-go](https://github.com/abatilo/asdf-httpie-go) |
| hub | [aqua:mislav/hub](https://github.com/mislav/hub) [asdf:mise-plugins/asdf-hub](https://github.com/mise-plugins/asdf-hub) |
| hugo | [aqua:gohugoio/hugo](https://github.com/gohugoio/hugo) [asdf:NeoHsu/asdf-hugo](https://github.com/NeoHsu/asdf-hugo) |
| hurl | [aqua:Orange-OpenSource/hurl](https://github.com/Orange-OpenSource/hurl) [asdf:raimon49/asdf-hurl](https://github.com/raimon49/asdf-hurl) |
| hwatch | [ubi:blacknon/hwatch](https://github.com/blacknon/hwatch) [asdf:chessmango/asdf-hwatch](https://github.com/chessmango/asdf-hwatch) |
| hygen | [asdf:brentjanderson/asdf-hygen](https://github.com/brentjanderson/asdf-hygen) |
| hyperfine | [ubi:sharkdp/hyperfine](https://github.com/sharkdp/hyperfine) [asdf:volf52/asdf-hyperfine](https://github.com/volf52/asdf-hyperfine) |
| iam-policy-json-to-terraform | [aqua:flosell/iam-policy-json-to-terraform](https://github.com/flosell/iam-policy-json-to-terraform) [asdf:carlduevel/asdf-iam-policy-json-to-terraform](https://github.com/carlduevel/asdf-iam-policy-json-to-terraform) |
| iamlive | [aqua:iann0036/iamlive](https://github.com/iann0036/iamlive) [asdf:chessmango/asdf-iamlive](https://github.com/chessmango/asdf-iamlive) |
| ibmcloud | [asdf:triangletodd/asdf-ibmcloud](https://github.com/triangletodd/asdf-ibmcloud) |
| idris | [asdf:asdf-community/asdf-idris](https://github.com/asdf-community/asdf-idris) |
| idris2 | [asdf:asdf-community/asdf-idris2](https://github.com/asdf-community/asdf-idris2) |
| imagemagick | [asdf:mangalakader/asdf-imagemagick](https://github.com/mangalakader/asdf-imagemagick) |
| imgpkg | [aqua:carvel-dev/imgpkg](https://github.com/carvel-dev/imgpkg) [asdf:vmware-tanzu/asdf-carvel](https://github.com/vmware-tanzu/asdf-carvel) |
| infracost | [aqua:infracost/infracost](https://github.com/infracost/infracost) [asdf:dex4er/asdf-infracost](https://github.com/dex4er/asdf-infracost) |
| inlets | [asdf:nlamirault/asdf-inlets](https://github.com/nlamirault/asdf-inlets) |
| io | [asdf:mracos/asdf-io](https://github.com/mracos/asdf-io) |
| istioctl | [aqua:istio/istio/istioctl](https://github.com/istio/istio/istioctl) [asdf:virtualstaticvoid/asdf-istioctl](https://github.com/virtualstaticvoid/asdf-istioctl) |
| janet | [asdf:Jakski/asdf-janet](https://github.com/Jakski/asdf-janet) |
| java | [core:java](https://mise.jdx.dev/lang/java.html) |
| jb | [asdf:beardix/asdf-jb](https://github.com/beardix/asdf-jb) |
| jbang | [asdf:jbangdev/jbang-asdf](https://github.com/jbangdev/jbang-asdf) |
| jfrog-cli | [asdf:LozanoMatheus/asdf-jfrog-cli](https://github.com/LozanoMatheus/asdf-jfrog-cli) |
| jib | [asdf:joschi/asdf-jib](https://github.com/joschi/asdf-jib) |
| jiq | [aqua:fiatjaf/jiq](https://github.com/fiatjaf/jiq) [asdf:chessmango/asdf-jiq](https://github.com/chessmango/asdf-jiq) |
| jless | [aqua:PaulJuliusMartinez/jless](https://github.com/PaulJuliusMartinez/jless) [asdf:jc00ke/asdf-jless](https://github.com/jc00ke/asdf-jless) |
| jmespath | [asdf:skyzyx/asdf-jmespath](https://github.com/skyzyx/asdf-jmespath) |
| jmeter | [asdf:comdotlinux/asdf-jmeter](https://github.com/comdotlinux/asdf-jmeter) |
| jnv | [aqua:ynqa/jnv](https://github.com/ynqa/jnv) [asdf:raimon49/asdf-jnv](https://github.com/raimon49/asdf-jnv) |
| jq | [aqua:jqlang/jq](https://github.com/jqlang/jq) [asdf:mise-plugins/asdf-jq](https://github.com/mise-plugins/asdf-jq) |
| jqp | [aqua:noahgorstein/jqp](https://github.com/noahgorstein/jqp) [asdf:https://gitlab.com/wt0f/asdf-jqp](https://gitlab.com/wt0f/asdf-jqp) |
| jreleaser | [aqua:jreleaser/jreleaser](https://github.com/jreleaser/jreleaser) [asdf:joschi/asdf-jreleaser](https://github.com/joschi/asdf-jreleaser) |
| jsonnet | [asdf:Banno/asdf-jsonnet](https://github.com/Banno/asdf-jsonnet) |
| julia | [asdf:rkyleg/asdf-julia](https://github.com/rkyleg/asdf-julia) |
| just | [ubi:casey/just](https://github.com/casey/just) [asdf:olofvndrhr/asdf-just](https://github.com/olofvndrhr/asdf-just) |
| jwt | [ubi:mike-engel/jwt-cli](https://github.com/mike-engel/jwt-cli) [cargo:jwt-cli](https://crates.io/crates/jwt-cli) |
| jwtui | [ubi:jwt-rs/jwt-ui](https://github.com/jwt-rs/jwt-ui) [cargo:jwt-ui](https://crates.io/crates/jwt-ui) |
| jx | [ubi:jenkins-x/jx](https://github.com/jenkins-x/jx) [asdf:vbehar/asdf-jx](https://github.com/vbehar/asdf-jx) |
| k0sctl | [ubi:k0sproject/k0sctl](https://github.com/k0sproject/k0sctl) [asdf:Its-Alex/asdf-plugin-k0sctl](https://github.com/Its-Alex/asdf-plugin-k0sctl) |
| k14s | [asdf:k14s/asdf-k14s](https://github.com/k14s/asdf-k14s) |
| k2tf | [ubi:sl1pm4t/k2tf](https://github.com/sl1pm4t/k2tf) [asdf:carlduevel/asdf-k2tf](https://github.com/carlduevel/asdf-k2tf) |
| k3d | [ubi:k3d-io/k3d](https://github.com/k3d-io/k3d) [asdf:spencergilbert/asdf-k3d](https://github.com/spencergilbert/asdf-k3d) |
| k3kcli | [asdf:xanmanning/asdf-k3kcli](https://github.com/xanmanning/asdf-k3kcli) |
| k3s | [asdf:dmpe/asdf-k3s](https://github.com/dmpe/asdf-k3s) |
| k3sup | [aqua:alexellis/k3sup](https://github.com/alexellis/k3sup) [asdf:cgroschupp/asdf-k3sup](https://github.com/cgroschupp/asdf-k3sup) |
| k6 | [ubi:grafana/k6](https://github.com/grafana/k6) [asdf:gr1m0h/asdf-k6](https://github.com/gr1m0h/asdf-k6) |
| k9s | [ubi:derailed/k9s](https://github.com/derailed/k9s) [asdf:looztra/asdf-k9s](https://github.com/looztra/asdf-k9s) |
| kafka | [asdf:ueisele/asdf-kafka](https://github.com/ueisele/asdf-kafka) |
| kafkactl | [aqua:deviceinsight/kafkactl](https://github.com/deviceinsight/kafkactl) [asdf:anweber/asdf-kafkactl](https://github.com/anweber/asdf-kafkactl) |
| kapp | [aqua:carvel-dev/kapp](https://github.com/carvel-dev/kapp) [asdf:vmware-tanzu/asdf-carvel](https://github.com/vmware-tanzu/asdf-carvel) |
| kbld | [aqua:carvel-dev/kbld](https://github.com/carvel-dev/kbld) [asdf:vmware-tanzu/asdf-carvel](https://github.com/vmware-tanzu/asdf-carvel) |
| kcat | [asdf:douglasdgoulart/asdf-kcat](https://github.com/douglasdgoulart/asdf-kcat) |
| kcctl | [asdf:joschi/asdf-kcctl](https://github.com/joschi/asdf-kcctl) |
| kcl | [asdf:starkers/asdf-kcl](https://github.com/starkers/asdf-kcl) |
| kconf | [aqua:particledecay/kconf](https://github.com/particledecay/kconf) [asdf:particledecay/asdf-kconf](https://github.com/particledecay/asdf-kconf) |
| ki | [asdf:comdotlinux/asdf-ki](https://github.com/comdotlinux/asdf-ki) |
| killport | [ubi:jkfran/killport](https://github.com/jkfran/killport) |
| kind | [ubi:kubernetes-sigs/kind](https://github.com/kubernetes-sigs/kind) [asdf:johnlayton/asdf-kind](https://github.com/johnlayton/asdf-kind) |
| kiota | [aqua:microsoft/kiota](https://github.com/microsoft/kiota) [asdf:asdf-community/asdf-kiota](https://github.com/asdf-community/asdf-kiota) |
| kn | [asdf:joke/asdf-kn](https://github.com/joke/asdf-kn) |
| ko | [aqua:ko-build/ko](https://github.com/ko-build/ko) [asdf:zasdaym/asdf-ko](https://github.com/zasdaym/asdf-ko) |
| koka | [asdf:susurri/asdf-koka](https://github.com/susurri/asdf-koka) |
| kompose | [ubi:kubernetes/kompose](https://github.com/kubernetes/kompose) [asdf:technikhil314/asdf-kompose](https://github.com/technikhil314/asdf-kompose) |
| kops | [aqua:kubernetes/kops](https://github.com/kubernetes/kops) [asdf:Antiarchitect/asdf-kops](https://github.com/Antiarchitect/asdf-kops) |
| kotlin | [asdf:asdf-community/asdf-kotlin](https://github.com/asdf-community/asdf-kotlin) [vfox:version-fox/vfox-kotlin](https://github.com/version-fox/vfox-kotlin) |
| kpack | [ubi:vmware-tanzu/kpack-cli](https://github.com/vmware-tanzu/kpack-cli) [asdf:asdf-community/asdf-kpack-cli](https://github.com/asdf-community/asdf-kpack-cli) |
| kpt | [aqua:kptdev/kpt](https://github.com/kptdev/kpt) [asdf:nlamirault/asdf-kpt](https://github.com/nlamirault/asdf-kpt) |
| krab | [asdf:ohkrab/asdf-krab](https://github.com/ohkrab/asdf-krab) |
| krew | [aqua:kubernetes-sigs/krew](https://github.com/kubernetes-sigs/krew) [asdf:bjw-s/asdf-krew](https://github.com/bjw-s/asdf-krew) |
| kscript | [asdf:edgelevel/asdf-kscript](https://github.com/edgelevel/asdf-kscript) |
| ksonnet | [asdf:Banno/asdf-ksonnet](https://github.com/Banno/asdf-ksonnet) |
| ksops | [asdf:janpieper/asdf-ksops](https://github.com/janpieper/asdf-ksops) |
| ktlint | [asdf:esensar/asdf-ktlint](https://github.com/esensar/asdf-ktlint) |
| kube-capacity | [aqua:robscott/kube-capacity](https://github.com/robscott/kube-capacity) [asdf:looztra/asdf-kube-capacity](https://github.com/looztra/asdf-kube-capacity) |
| kube-code-generator | [asdf:jimmidyson/asdf-kube-code-generator](https://github.com/jimmidyson/asdf-kube-code-generator) |
| kube-controller-tools | [asdf:jimmidyson/asdf-kube-controller-tools](https://github.com/jimmidyson/asdf-kube-controller-tools) |
| kube-credential-cache | [aqua:ryodocx/kube-credential-cache](https://github.com/ryodocx/kube-credential-cache) [asdf:ryodocx/kube-credential-cache](https://github.com/ryodocx/kube-credential-cache) |
| kube-linter | [aqua:stackrox/kube-linter](https://github.com/stackrox/kube-linter) [asdf:devlincashman/asdf-kube-linter](https://github.com/devlincashman/asdf-kube-linter) |
| kube-score | [aqua:zegl/kube-score](https://github.com/zegl/kube-score) [asdf:bageljp/asdf-kube-score](https://github.com/bageljp/asdf-kube-score) |
| kubebuilder | [aqua:kubernetes-sigs/kubebuilder](https://github.com/kubernetes-sigs/kubebuilder) [asdf:virtualstaticvoid/asdf-kubebuilder](https://github.com/virtualstaticvoid/asdf-kubebuilder) |
| kubecm | [aqua:sunny0826/kubecm](https://github.com/sunny0826/kubecm) [asdf:samhvw8/asdf-kubecm](https://github.com/samhvw8/asdf-kubecm) |
| kubecolor | [aqua:hidetatz/kubecolor](https://github.com/hidetatz/kubecolor) [asdf:dex4er/asdf-kubecolor](https://github.com/dex4er/asdf-kubecolor) |
| kubeconform | [aqua:yannh/kubeconform](https://github.com/yannh/kubeconform) [asdf:lirlia/asdf-kubeconform](https://github.com/lirlia/asdf-kubeconform) |
| kubectl | [aqua:kubernetes/kubectl](https://github.com/kubernetes/kubectl) [asdf:asdf-community/asdf-kubectl](https://github.com/asdf-community/asdf-kubectl) |
| kubectl-bindrole | [asdf:looztra/asdf-kubectl-bindrole](https://github.com/looztra/asdf-kubectl-bindrole) |
| kubectl-buildkit | [asdf:ezcater/asdf-kubectl-buildkit](https://github.com/ezcater/asdf-kubectl-buildkit) |
| kubectl-convert | [aqua:kubernetes/kubectl-convert](https://github.com/kubernetes/kubectl-convert) [asdf:iul1an/asdf-kubectl-convert](https://github.com/iul1an/asdf-kubectl-convert) |
| kubectl-kots | [asdf:ganta/asdf-kubectl-kots](https://github.com/ganta/asdf-kubectl-kots) |
| kubectl-kuttl | [aqua:kudobuilder/kuttl](https://github.com/kudobuilder/kuttl) [asdf:jimmidyson/asdf-kuttl](https://github.com/jimmidyson/asdf-kuttl) |
| kubectx | [aqua:ahmetb/kubectx](https://github.com/ahmetb/kubectx) [asdf:https://gitlab.com/wt0f/asdf-kubectx](https://gitlab.com/wt0f/asdf-kubectx) |
| kubefedctl | [asdf:kvokka/asdf-kubefedctl](https://github.com/kvokka/asdf-kubefedctl) |
| kubefirst | [asdf:Claywd/asdf-kubefirst](https://github.com/Claywd/asdf-kubefirst) |
| kubelogin | [aqua:Azure/kubelogin](https://github.com/Azure/kubelogin) [asdf:sechmann/asdf-kubelogin](https://github.com/sechmann/asdf-kubelogin) |
| kubemqctl | [aqua:kubemq-io/kubemqctl](https://github.com/kubemq-io/kubemqctl) [asdf:johnlayton/asdf-kubemqctl](https://github.com/johnlayton/asdf-kubemqctl) |
| kubent | [asdf:virtualstaticvoid/asdf-kubent](https://github.com/virtualstaticvoid/asdf-kubent) |
| kubergrunt | [aqua:gruntwork-io/kubergrunt](https://github.com/gruntwork-io/kubergrunt) [asdf:NeoHsu/asdf-kubergrunt](https://github.com/NeoHsu/asdf-kubergrunt) |
| kubeseal | [asdf:stefansedich/asdf-kubeseal](https://github.com/stefansedich/asdf-kubeseal) |
| kubesec | [aqua:controlplaneio/kubesec](https://github.com/controlplaneio/kubesec) [asdf:vitalis/asdf-kubesec](https://github.com/vitalis/asdf-kubesec) |
| kubeshark | [aqua:kubeshark/kubeshark](https://github.com/kubeshark/kubeshark) [asdf:carnei-ro/asdf-kubeshark](https://github.com/carnei-ro/asdf-kubeshark) |
| kubespy | [aqua:pulumi/kubespy](https://github.com/pulumi/kubespy) [asdf:jfreeland/asdf-kubespy](https://github.com/jfreeland/asdf-kubespy) |
| kubeval | [aqua:instrumenta/kubeval](https://github.com/instrumenta/kubeval) [asdf:stefansedich/asdf-kubeval](https://github.com/stefansedich/asdf-kubeval) |
| kubevela | [aqua:kubevela/kubevela](https://github.com/kubevela/kubevela) [asdf:gustavclausen/asdf-kubevela](https://github.com/gustavclausen/asdf-kubevela) |
| kubie | [aqua:sbstp/kubie](https://github.com/sbstp/kubie) [asdf:johnhamelink/asdf-kubie](https://github.com/johnhamelink/asdf-kubie) |
| kustomize | [aqua:kubernetes-sigs/kustomize](https://github.com/kubernetes-sigs/kustomize) [asdf:Banno/asdf-kustomize](https://github.com/Banno/asdf-kustomize) |
| kwt | [aqua:carvel-dev/kwt](https://github.com/carvel-dev/kwt) [asdf:vmware-tanzu/asdf-carvel](https://github.com/vmware-tanzu/asdf-carvel) |
| kyverno | [aqua:kyverno/kyverno](https://github.com/kyverno/kyverno) [asdf:https://github.com/hobaen/asdf-kyverno-cli.git](https://github.com/hobaen/asdf-kyverno-cli) |
| lab | [aqua:zaquestion/lab](https://github.com/zaquestion/lab) [asdf:particledecay/asdf-lab](https://github.com/particledecay/asdf-lab) |
| lane | [asdf:CodeReaper/asdf-lane](https://github.com/CodeReaper/asdf-lane) |
| lazygit | [aqua:jesseduffield/lazygit](https://github.com/jesseduffield/lazygit) [asdf:nklmilojevic/asdf-lazygit](https://github.com/nklmilojevic/asdf-lazygit) |
| lean | [asdf:asdf-community/asdf-lean](https://github.com/asdf-community/asdf-lean) |
| lefthook | [aqua:evilmartians/lefthook](https://github.com/evilmartians/lefthook) [ubi:evilmartians/lefthook](https://github.com/evilmartians/lefthook) [asdf:jtzero/asdf-lefthook](https://github.com/jtzero/asdf-lefthook) |
| leiningen | [asdf:miorimmax/asdf-lein](https://github.com/miorimmax/asdf-lein) |
| levant | [aqua:hashicorp/levant](https://github.com/hashicorp/levant) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| lfe | [asdf:asdf-community/asdf-lfe](https://github.com/asdf-community/asdf-lfe) |
| libsql-server | [asdf:jonasb/asdf-libsql-server](https://github.com/jonasb/asdf-libsql-server) |
| license-plist | [asdf:MacPaw/asdf-license-plist](https://github.com/MacPaw/asdf-license-plist) |
| lima | [aqua:lima-vm/lima](https://github.com/lima-vm/lima) [asdf:CrouchingMuppet/asdf-lima](https://github.com/CrouchingMuppet/asdf-lima) |
| link | [asdf:asdf-community/asdf-link](https://github.com/asdf-community/asdf-link) |
| linkerd | [asdf:kforsthoevel/asdf-linkerd](https://github.com/kforsthoevel/asdf-linkerd) |
| liqoctl | [asdf:pdemagny/asdf-liqoctl](https://github.com/pdemagny/asdf-liqoctl) |
| liquibase | [asdf:saliougaye/asdf-liquibase](https://github.com/saliougaye/asdf-liquibase) |
| litestream | [aqua:benbjohnson/litestream](https://github.com/benbjohnson/litestream) [asdf:threkk/asdf-litestream](https://github.com/threkk/asdf-litestream) |
| llvm-objcopy | [asdf:higebu/asdf-llvm](https://github.com/higebu/asdf-llvm) |
| llvm-objdump | [asdf:higebu/asdf-llvm](https://github.com/higebu/asdf-llvm) |
| logtalk | [asdf:LogtalkDotOrg/asdf-logtalk](https://github.com/LogtalkDotOrg/asdf-logtalk) |
| loki-logcli | [asdf:comdotlinux/asdf-loki-logcli](https://github.com/comdotlinux/asdf-loki-logcli) |
| ls-lint | [aqua:loeffel-io/ls-lint](https://github.com/loeffel-io/ls-lint) [asdf:Ameausoone/asdf-ls-lint](https://github.com/Ameausoone/asdf-ls-lint) |
| lsd | [aqua:lsd-rs/lsd](https://github.com/lsd-rs/lsd) [asdf:mise-plugins/asdf-lsd](https://github.com/mise-plugins/asdf-lsd) |
| lua | [asdf:Stratus3D/asdf-lua](https://github.com/Stratus3D/asdf-lua) |
| lua-language-server | [aqua:LuaLS/lua-language-server](https://github.com/LuaLS/lua-language-server) [asdf:bellini666/asdf-lua-language-server](https://github.com/bellini666/asdf-lua-language-server) |
| luajit | [asdf:smashedtoatoms/asdf-luaJIT](https://github.com/smashedtoatoms/asdf-luaJIT) |
| lucy | [asdf:cometkim/asdf-lucy](https://github.com/cometkim/asdf-lucy) |
| maestro | [asdf:dotanuki-labs/asdf-maestro](https://github.com/dotanuki-labs/asdf-maestro) |
| mage | [aqua:magefile/mage](https://github.com/magefile/mage) [asdf:mathew-fleisch/asdf-mage](https://github.com/mathew-fleisch/asdf-mage) |
| make | [asdf:yacchi/asdf-make](https://github.com/yacchi/asdf-make) |
| mani | [asdf:anweber/asdf-mani](https://github.com/anweber/asdf-mani) |
| mark | [asdf:jfreeland/asdf-mark](https://github.com/jfreeland/asdf-mark) |
| markdownlint-cli2 | [npm:markdownlint-cli2](https://www.npmjs.com/package/markdownlint-cli2) [asdf:paulo-ferraz-oliveira/asdf-markdownlint-cli2](https://github.com/paulo-ferraz-oliveira/asdf-markdownlint-cli2) |
| marp-cli | [aqua:marp-team/marp-cli](https://github.com/marp-team/marp-cli) [asdf:xataz/asdf-marp-cli](https://github.com/xataz/asdf-marp-cli) |
| mask | [aqua:jacobdeichert/mask](https://github.com/jacobdeichert/mask) [asdf:aaaaninja/asdf-mask](https://github.com/aaaaninja/asdf-mask) |
| maven | [asdf:mise-plugins/asdf-maven](https://github.com/mise-plugins/asdf-maven) [vfox:version-fox/vfox-maven](https://github.com/version-fox/vfox-maven) |
| mc | [asdf:penpyt/asdf-mc](https://github.com/penpyt/asdf-mc) |
| mdbook | [asdf:cipherstash/asdf-mdbook](https://github.com/cipherstash/asdf-mdbook) |
| mdbook-linkcheck | [asdf:cipherstash/asdf-mdbook-linkcheck](https://github.com/cipherstash/asdf-mdbook-linkcheck) |
| melange | [aqua:chainguard-dev/melange](https://github.com/chainguard-dev/melange) [asdf:omissis/asdf-melange](https://github.com/omissis/asdf-melange) |
| melt | [asdf:chessmango/asdf-melt](https://github.com/chessmango/asdf-melt) |
| memcached | [asdf:furkanural/asdf-memcached](https://github.com/furkanural/asdf-memcached) |
| mercury | [asdf:susurri/asdf-mercury](https://github.com/susurri/asdf-mercury) |
| meson | [asdf:asdf-community/asdf-meson](https://github.com/asdf-community/asdf-meson) |
| micronaut | [asdf:weibemoura/asdf-micronaut](https://github.com/weibemoura/asdf-micronaut) |
| mill | [asdf:asdf-community/asdf-mill](https://github.com/asdf-community/asdf-mill) |
| mimirtool | [aqua:grafana/mimir/mimirtool](https://github.com/grafana/mimir/mimirtool) [asdf:asdf-community/asdf-mimirtool](https://github.com/asdf-community/asdf-mimirtool) |
| minify | [aqua:tdewolff/minify](https://github.com/tdewolff/minify) [asdf:axilleas/asdf-minify](https://github.com/axilleas/asdf-minify) |
| minikube | [aqua:kubernetes/minikube](https://github.com/kubernetes/minikube) [asdf:alvarobp/asdf-minikube](https://github.com/alvarobp/asdf-minikube) |
| minio | [asdf:aeons/asdf-minio](https://github.com/aeons/asdf-minio) |
| minishift | [aqua:minishift/minishift](https://github.com/minishift/minishift) [asdf:sqtran/asdf-minishift](https://github.com/sqtran/asdf-minishift) |
| mint | [asdf:mint-lang/asdf-mint](https://github.com/mint-lang/asdf-mint) |
| mirrord | [asdf:metalbear-co/asdf-mirrord](https://github.com/metalbear-co/asdf-mirrord) |
| mitmproxy | [asdf:NeoHsu/asdf-mitmproxy](https://github.com/NeoHsu/asdf-mitmproxy) |
| mkcert | [ubi:FiloSottile/mkcert](https://github.com/FiloSottile/mkcert) [asdf:salasrod/asdf-mkcert](https://github.com/salasrod/asdf-mkcert) |
| mlton | [asdf:asdf-community/asdf-mlton](https://github.com/asdf-community/asdf-mlton) |
| mockery | [aqua:vektra/mockery](https://github.com/vektra/mockery) [asdf:cabify/asdf-mockery](https://github.com/cabify/asdf-mockery) |
| mockolo | [asdf:MontakOleg/asdf-mockolo](https://github.com/MontakOleg/asdf-mockolo) |
| mold | [ubi:rui314/mold](https://github.com/rui314/mold) |
| monarch | [asdf:nyuyuyu/asdf-monarch](https://github.com/nyuyuyu/asdf-monarch) |
| mongo-tools | [asdf:itspngu/asdf-mongo-tools](https://github.com/itspngu/asdf-mongo-tools) |
| mongodb | [asdf:sylph01/asdf-mongodb](https://github.com/sylph01/asdf-mongodb) |
| mongosh | [asdf:itspngu/asdf-mongosh](https://github.com/itspngu/asdf-mongosh) |
| mprocs | [ubi:pvolok/mprocs](https://github.com/pvolok/mprocs) |
| mutanus | [asdf:SoriUR/asdf-mutanus](https://github.com/SoriUR/asdf-mutanus) |
| mvnd | [asdf:joschi/asdf-mvnd](https://github.com/joschi/asdf-mvnd) |
| mysql | [asdf:iroddis/asdf-mysql](https://github.com/iroddis/asdf-mysql) |
| nancy | [aqua:sonatype-nexus-community/nancy](https://github.com/sonatype-nexus-community/nancy) [asdf:iilyak/asdf-nancy](https://github.com/iilyak/asdf-nancy) |
| nano | [asdf:mfakane/asdf-nano](https://github.com/mfakane/asdf-nano) |
| nasm | [asdf:Dpbm/asdf-nasm](https://github.com/Dpbm/asdf-nasm) |
| neko | [asdf:asdf-community/asdf-neko](https://github.com/asdf-community/asdf-neko) |
| neovim | [aqua:neovim/neovim](https://github.com/neovim/neovim) [asdf:richin13/asdf-neovim](https://github.com/richin13/asdf-neovim) |
| nerdctl | [asdf:dmpe/asdf-nerdctl](https://github.com/dmpe/asdf-nerdctl) |
| newrelic | [ubi:newrelic/newrelic-cli](https://github.com/newrelic/newrelic-cli) [asdf:NeoHsu/asdf-newrelic-cli](https://github.com/NeoHsu/asdf-newrelic-cli) |
| nfpm | [aqua:goreleaser/nfpm](https://github.com/goreleaser/nfpm) [ubi:goreleaser/nfpm](https://github.com/goreleaser/nfpm) [asdf:ORCID/asdf-nfpm](https://github.com/ORCID/asdf-nfpm) |
| nim | [asdf:mise-plugins/mise-nim](https://github.com/mise-plugins/mise-nim) |
| ninja | [aqua:ninja-build/ninja](https://github.com/ninja-build/ninja) [asdf:asdf-community/asdf-ninja](https://github.com/asdf-community/asdf-ninja) |
| node | [core:node](https://mise.jdx.dev/lang/node.html) |
| nomad | [aqua:hashicorp/levant](https://github.com/hashicorp/levant) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| nomad-pack | [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| notation | [aqua:notaryproject/notation](https://github.com/notaryproject/notation) [asdf:bodgit/asdf-notation](https://github.com/bodgit/asdf-notation) |
| nova | [aqua:FairwindsOps/nova](https://github.com/FairwindsOps/nova) [asdf:elementalvoid/asdf-nova](https://github.com/elementalvoid/asdf-nova) |
| nsc | [asdf:dex4er/asdf-nsc](https://github.com/dex4er/asdf-nsc) |
| oapi-codegen | [asdf:dylanrayboss/asdf-oapi-codegen](https://github.com/dylanrayboss/asdf-oapi-codegen) |
| oc | [asdf:sqtran/asdf-oc](https://github.com/sqtran/asdf-oc) |
| ocaml | [asdf:asdf-community/asdf-ocaml](https://github.com/asdf-community/asdf-ocaml) |
| oci | [asdf:yasn77/asdf-oci](https://github.com/yasn77/asdf-oci) |
| odin | [asdf:jtakakura/asdf-odin](https://github.com/jtakakura/asdf-odin) |
| odo | [aqua:redhat-developer/odo](https://github.com/redhat-developer/odo) [asdf:rm3l/asdf-odo](https://github.com/rm3l/asdf-odo) |
| okta-aws | [aqua:okta/okta-aws-cli](https://github.com/okta/okta-aws-cli) [asdf:bennythejudge/asdf-plugin-okta-aws-cli](https://github.com/bennythejudge/asdf-plugin-okta-aws-cli) |
| okteto | [aqua:okteto/okteto](https://github.com/okteto/okteto) [asdf:BradenM/asdf-okteto](https://github.com/BradenM/asdf-okteto) |
| ollama | [aqua:ollama/ollama](https://github.com/ollama/ollama) [asdf:virtualstaticvoid/asdf-ollama](https://github.com/virtualstaticvoid/asdf-ollama) |
| om | [aqua:pivotal-cf/om](https://github.com/pivotal-cf/om) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| onyx | [asdf:jtakakura/asdf-onyx](https://github.com/jtakakura/asdf-onyx) |
| opa | [aqua:open-policy-agent/opa](https://github.com/open-policy-agent/opa) [asdf:tochukwuvictor/asdf-opa](https://github.com/tochukwuvictor/asdf-opa) |
| opam | [asdf:asdf-community/asdf-opam](https://github.com/asdf-community/asdf-opam) |
| openbao | [ubi:openbao/openbao](https://github.com/openbao/openbao) |
| openfaas-faas | [asdf:zekker6/asdf-faas-cli](https://github.com/zekker6/asdf-faas-cli) |
| openresty | [asdf:smashedtoatoms/asdf-openresty](https://github.com/smashedtoatoms/asdf-openresty) |
| opensearch | [asdf:randikabanura/asdf-opensearch](https://github.com/randikabanura/asdf-opensearch) |
| opensearch-cli | [asdf:iul1an/asdf-opensearch-cli](https://github.com/iul1an/asdf-opensearch-cli) |
| openshift-install | [asdf:hhemied/asdf-openshift-install](https://github.com/hhemied/asdf-openshift-install) |
| opentofu | [ubi:opentofu/opentofu](https://github.com/opentofu/opentofu) [asdf:virtualroot/asdf-opentofu](https://github.com/virtualroot/asdf-opentofu) |
| operator-sdk | [aqua:operator-framework/operator-sdk](https://github.com/operator-framework/operator-sdk) [asdf:Medium/asdf-operator-sdk](https://github.com/Medium/asdf-operator-sdk) |
| opsgenie-lamp | [asdf:ORCID/asdf-opsgenie-lamp](https://github.com/ORCID/asdf-opsgenie-lamp) |
| oras | [aqua:oras-project/oras](https://github.com/oras-project/oras) [asdf:bodgit/asdf-oras](https://github.com/bodgit/asdf-oras) |
| osm | [asdf:nlamirault/asdf-osm](https://github.com/nlamirault/asdf-osm) |
| osqueryi | [asdf:davidecavestro/asdf-osqueryi](https://github.com/davidecavestro/asdf-osqueryi) |
| overmind | [ubi:DarthSim/overmind](https://github.com/DarthSim/overmind) [go:github.com/DarthSim/overmind/v2](https://pkg.go.dev/github.com/DarthSim/overmind/v2) |
| pachctl | [asdf:abatilo/asdf-pachctl](https://github.com/abatilo/asdf-pachctl) |
| packer | [aqua:hashicorp/packer](https://github.com/hashicorp/packer) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| pandoc | [asdf:Fbrisset/asdf-pandoc](https://github.com/Fbrisset/asdf-pandoc) |
| patat | [asdf:airtonix/asdf-patat](https://github.com/airtonix/asdf-patat) |
| pdm | [asdf:1oglop1/asdf-pdm](https://github.com/1oglop1/asdf-pdm) |
| peco | [aqua:peco/peco](https://github.com/peco/peco) [asdf:asdf-community/asdf-peco](https://github.com/asdf-community/asdf-peco) |
| periphery | [asdf:MontakOleg/asdf-periphery](https://github.com/MontakOleg/asdf-periphery) |
| perl | [asdf:ouest/asdf-perl](https://github.com/ouest/asdf-perl) |
| php | [asdf:asdf-community/asdf-php](https://github.com/asdf-community/asdf-php) [vfox:version-fox/vfox-php](https://github.com/version-fox/vfox-php) |
| pint | [aqua:cloudflare/pint](https://github.com/cloudflare/pint) [asdf:sam-burrell/asdf-pint](https://github.com/sam-burrell/asdf-pint) |
| pipectl | [aqua:pipe-cd/pipecd/pipectl](https://github.com/pipe-cd/pipecd/pipectl) [asdf:pipe-cd/asdf-pipectl](https://github.com/pipe-cd/asdf-pipectl) |
| pipelight | [asdf:kogeletey/asdf-pipelight](https://github.com/kogeletey/asdf-pipelight) |
| pipenv | [asdf:mise-plugins/mise-pipenv](https://github.com/mise-plugins/mise-pipenv) |
| pipx | [asdf:yozachar/asdf-pipx](https://github.com/yozachar/asdf-pipx) |
| pivnet | [aqua:pivotal-cf/pivnet-cli](https://github.com/pivotal-cf/pivnet-cli) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| pkl | [aqua:apple/pkl](https://github.com/apple/pkl) [asdf:mise-plugins/asdf-pkl](https://github.com/mise-plugins/asdf-pkl) |
| please | [aqua:thought-machine/please](https://github.com/thought-machine/please) [asdf:asdf-community/asdf-please](https://github.com/asdf-community/asdf-please) |
| pluto | [ubi:FairwindsOps/pluto](https://github.com/FairwindsOps/pluto) [asdf:FairwindsOps/asdf-pluto](https://github.com/FairwindsOps/asdf-pluto) |
| pnpm | [aqua:pnpm/pnpm](https://github.com/pnpm/pnpm) [asdf:jonathanmorley/asdf-pnpm](https://github.com/jonathanmorley/asdf-pnpm) |
| podman | [asdf:tvon/asdf-podman](https://github.com/tvon/asdf-podman) |
| poetry | [asdf:mise-plugins/mise-poetry](https://github.com/mise-plugins/mise-poetry) |
| polaris | [aqua:FairwindsOps/polaris](https://github.com/FairwindsOps/polaris) [asdf:particledecay/asdf-polaris](https://github.com/particledecay/asdf-polaris) |
| popeye | [aqua:derailed/popeye](https://github.com/derailed/popeye) [asdf:nlamirault/asdf-popeye](https://github.com/nlamirault/asdf-popeye) |
| postgis | [asdf:knu/asdf-postgis](https://github.com/knu/asdf-postgis) |
| postgres | [asdf:smashedtoatoms/asdf-postgres](https://github.com/smashedtoatoms/asdf-postgres) |
| powerline-go | [asdf:dex4er/asdf-powerline-go](https://github.com/dex4er/asdf-powerline-go) |
| powerpipe | [aqua:turbot/powerpipe](https://github.com/turbot/powerpipe) [asdf:jc00ke/asdf-powerpipe](https://github.com/jc00ke/asdf-powerpipe) |
| powershell-core | [asdf:daveneeley/asdf-powershell-core](https://github.com/daveneeley/asdf-powershell-core) |
| pre-commit | [aqua:pre-commit/pre-commit](https://github.com/pre-commit/pre-commit) [asdf:jonathanmorley/asdf-pre-commit](https://github.com/jonathanmorley/asdf-pre-commit) |
| promtool | [asdf:asdf-community/asdf-promtool](https://github.com/asdf-community/asdf-promtool) |
| protobuf | [vfox:ahai-code/vfox-protobuf](https://github.com/ahai-code/vfox-protobuf) |
| protoc | [aqua:protocolbuffers/protobuf/protoc](https://github.com/protocolbuffers/protobuf/protoc) [asdf:paxosglobal/asdf-protoc](https://github.com/paxosglobal/asdf-protoc) |
| protoc-gen-connect-go | [asdf:dylanrayboss/asdf-protoc-gen-connect-go](https://github.com/dylanrayboss/asdf-protoc-gen-connect-go) |
| protoc-gen-go | [aqua:protocolbuffers/protobuf-go/protoc-gen-go](https://github.com/protocolbuffers/protobuf-go/protoc-gen-go) [asdf:pbr0ck3r/asdf-protoc-gen-go](https://github.com/pbr0ck3r/asdf-protoc-gen-go) |
| protoc-gen-go-grpc | [aqua:grpc/grpc-go/protoc-gen-go-grpc](https://github.com/grpc/grpc-go/protoc-gen-go-grpc) [asdf:pbr0ck3r/asdf-protoc-gen-go-grpc](https://github.com/pbr0ck3r/asdf-protoc-gen-go-grpc) |
| protoc-gen-grpc-web | [asdf:pbr0ck3r/asdf-protoc-gen-grpc-web](https://github.com/pbr0ck3r/asdf-protoc-gen-grpc-web) |
| protoc-gen-js | [asdf:pbr0ck3r/asdf-protoc-gen-js](https://github.com/pbr0ck3r/asdf-protoc-gen-js) |
| protolint | [aqua:yoheimuta/protolint](https://github.com/yoheimuta/protolint) [asdf:spencergilbert/asdf-protolint](https://github.com/spencergilbert/asdf-protolint) |
| protonge | [asdf:augustobmoura/asdf-protonge](https://github.com/augustobmoura/asdf-protonge) |
| psc-package | [asdf:nsaunders/asdf-psc-package](https://github.com/nsaunders/asdf-psc-package) |
| pulumi | [aqua:pulumi/pulumi](https://github.com/pulumi/pulumi) [asdf:canha/asdf-pulumi](https://github.com/canha/asdf-pulumi) |
| purerl | [asdf:GoNZooo/asdf-purerl](https://github.com/GoNZooo/asdf-purerl) |
| purescript | [asdf:jrrom/asdf-purescript](https://github.com/jrrom/asdf-purescript) |
| purty | [asdf:nsaunders/asdf-purty](https://github.com/nsaunders/asdf-purty) |
| python | [core:python](https://mise.jdx.dev/lang/python.html) |
| qdns | [asdf:moritz-makandra/asdf-plugin-qdns](https://github.com/moritz-makandra/asdf-plugin-qdns) |
| quarkus | [asdf:asdf-community/asdf-quarkus](https://github.com/asdf-community/asdf-quarkus) |
| r | [asdf:asdf-community/asdf-r](https://github.com/asdf-community/asdf-r) |
| rabbitmq | [asdf:mise-plugins/asdf-rabbitmq](https://github.com/mise-plugins/asdf-rabbitmq) |
| racket | [asdf:asdf-community/asdf-racket](https://github.com/asdf-community/asdf-racket) |
| raku | [asdf:m-dango/asdf-raku](https://github.com/m-dango/asdf-raku) |
| rancher | [asdf:abinet/asdf-rancher](https://github.com/abinet/asdf-rancher) |
| rbac-lookup | [aqua:FairwindsOps/rbac-lookup](https://github.com/FairwindsOps/rbac-lookup) [asdf:looztra/asdf-rbac-lookup](https://github.com/looztra/asdf-rbac-lookup) |
| rclone | [ubi:rclone/rclone](https://github.com/rclone/rclone) [asdf:johnlayton/asdf-rclone](https://github.com/johnlayton/asdf-rclone) |
| rebar | [asdf:Stratus3D/asdf-rebar](https://github.com/Stratus3D/asdf-rebar) |
| reckoner | [asdf:FairwindsOps/asdf-reckoner](https://github.com/FairwindsOps/asdf-reckoner) |
| redis | [asdf:smashedtoatoms/asdf-redis](https://github.com/smashedtoatoms/asdf-redis) |
| redis-cli | [asdf:NeoHsu/asdf-redis-cli](https://github.com/NeoHsu/asdf-redis-cli) |
| redo | [asdf:chessmango/asdf-redo](https://github.com/chessmango/asdf-redo) |
| redskyctl | [asdf:sudermanjr/asdf-redskyctl](https://github.com/sudermanjr/asdf-redskyctl) |
| reg | [aqua:genuinetools/reg](https://github.com/genuinetools/reg) [asdf:looztra/asdf-reg](https://github.com/looztra/asdf-reg) |
| regal | [aqua:StyraInc/regal](https://github.com/StyraInc/regal) [asdf:asdf-community/asdf-regal](https://github.com/asdf-community/asdf-regal) |
| regctl | [aqua:regclient/regclient/regctl](https://github.com/regclient/regclient/regctl) [asdf:ORCID/asdf-regctl](https://github.com/ORCID/asdf-regctl) |
| regsync | [aqua:regclient/regclient/regsync](https://github.com/regclient/regclient/regsync) [asdf:rsrchboy/asdf-regsync](https://github.com/rsrchboy/asdf-regsync) |
| restic | [aqua:restic/restic](https://github.com/restic/restic) [asdf:xataz/asdf-restic](https://github.com/xataz/asdf-restic) |
| restish | [ubi:danielgtaylor/restish](https://github.com/danielgtaylor/restish) [go:github.com/danielgtaylor/restish](https://pkg.go.dev/github.com/danielgtaylor/restish) |
| revive | [aqua:mgechev/revive](https://github.com/mgechev/revive) [asdf:bjw-s/asdf-revive](https://github.com/bjw-s/asdf-revive) |
| richgo | [aqua:kyoh86/richgo](https://github.com/kyoh86/richgo) [asdf:paxosglobal/asdf-richgo](https://github.com/paxosglobal/asdf-richgo) |
| riff | [asdf:abinet/asdf-riff](https://github.com/abinet/asdf-riff) |
| ripgrep | [aqua:BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) [ubi:BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) [asdf:https://gitlab.com/wt0f/asdf-ripgrep](https://gitlab.com/wt0f/asdf-ripgrep) |
| ripgrep-all | [aqua:phiresky/ripgrep-all](https://github.com/phiresky/ripgrep-all) |
| ripsecret | [aqua:sirwart/ripsecrets](https://github.com/sirwart/ripsecrets) [asdf:https://github.com/boris-smidt-klarrio/asdf-ripsecrets](https://github.com/boris-smidt-klarrio/asdf-ripsecrets) |
| rke | [aqua:rancher/rke](https://github.com/rancher/rke) [asdf:particledecay/asdf-rke](https://github.com/particledecay/asdf-rke) |
| rlwrap | [asdf:asdf-community/asdf-rlwrap](https://github.com/asdf-community/asdf-rlwrap) |
| rome | [asdf:kichiemon/asdf-rome](https://github.com/kichiemon/asdf-rome) |
| rstash | [asdf:carlduevel/asdf-rstash](https://github.com/carlduevel/asdf-rstash) |
| ruby | [core:ruby](https://mise.jdx.dev/lang/ruby.html) |
| ruff | [aqua:astral-sh/ruff](https://github.com/astral-sh/ruff) [ubi:astral-sh/ruff](https://github.com/astral-sh/ruff) [asdf:simhem/asdf-ruff](https://github.com/simhem/asdf-ruff) |
| rust | [core:rust](https://mise.jdx.dev/lang/rust.html) [asdf:code-lever/asdf-rust](https://github.com/code-lever/asdf-rust) |
| rust-analyzer | [aqua:rust-lang/rust-analyzer](https://github.com/rust-lang/rust-analyzer) [asdf:Xyven1/asdf-rust-analyzer](https://github.com/Xyven1/asdf-rust-analyzer) |
| rustic | [ubi:rustic-rs/rustic](https://github.com/rustic-rs/rustic) |
| rye | [aqua:astral-sh/rye](https://github.com/astral-sh/rye) [asdf:Azuki-bar/asdf-rye](https://github.com/Azuki-bar/asdf-rye) |
| saml2aws | [aqua:Versent/saml2aws](https://github.com/Versent/saml2aws) [asdf:elementalvoid/asdf-saml2aws](https://github.com/elementalvoid/asdf-saml2aws) |
| sbcl | [asdf:smashedtoatoms/asdf-sbcl](https://github.com/smashedtoatoms/asdf-sbcl) |
| sbt | [asdf:bram2000/asdf-sbt](https://github.com/bram2000/asdf-sbt) |
| scala | [asdf:asdf-community/asdf-scala](https://github.com/asdf-community/asdf-scala) [vfox:version-fox/vfox-scala](https://github.com/version-fox/vfox-scala) |
| scala-cli | [asdf:asdf-community/asdf-scala-cli](https://github.com/asdf-community/asdf-scala-cli) |
| scaleway | [aqua:scaleway/scaleway-cli](https://github.com/scaleway/scaleway-cli) [asdf:albarralnunez/asdf-plugin-scaleway-cli](https://github.com/albarralnunez/asdf-plugin-scaleway-cli) |
| scalingo-cli | [asdf:brandon-welsch/asdf-scalingo-cli](https://github.com/brandon-welsch/asdf-scalingo-cli) |
| scarb | [asdf:software-mansion/asdf-scarb](https://github.com/software-mansion/asdf-scarb) |
| sccache | [ubi:mozilla/sccache](https://github.com/mozilla/sccache) [asdf:emersonmx/asdf-sccache](https://github.com/emersonmx/asdf-sccache) |
| scenery | [asdf:skyzyx/asdf-scenery](https://github.com/skyzyx/asdf-scenery) |
| schemacrawler | [asdf:davidecavestro/asdf-schemacrawler](https://github.com/davidecavestro/asdf-schemacrawler) |
| scie-pants | [asdf:robzr/asdf-scie-pants](https://github.com/robzr/asdf-scie-pants) |
| seed7 | [asdf:susurri/asdf-seed7](https://github.com/susurri/asdf-seed7) |
| semgrep | [asdf:brentjanderson/asdf-semgrep](https://github.com/brentjanderson/asdf-semgrep) |
| semtag | [asdf:junminahn/asdf-semtag](https://github.com/junminahn/asdf-semtag) |
| semver | [asdf:mathew-fleisch/asdf-semver](https://github.com/mathew-fleisch/asdf-semver) |
| sentinel | [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| sentry | [ubi:getsentry/sentry-cli](https://github.com/getsentry/sentry-cli) |
| serf | [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| serverless | [asdf:pdemagny/asdf-serverless](https://github.com/pdemagny/asdf-serverless) |
| setup-envtest | [asdf:pmalek/mise-setup-envtest](https://github.com/pmalek/mise-setup-envtest) |
| shell2http | [aqua:msoap/shell2http](https://github.com/msoap/shell2http) [asdf:ORCID/asdf-shell2http](https://github.com/ORCID/asdf-shell2http) |
| shellcheck | [aqua:koalaman/shellcheck](https://github.com/koalaman/shellcheck) [ubi:koalaman/shellcheck](https://github.com/koalaman/shellcheck) [asdf:luizm/asdf-shellcheck](https://github.com/luizm/asdf-shellcheck) |
| shellspec | [aqua:shellspec/shellspec](https://github.com/shellspec/shellspec) [asdf:poikilotherm/asdf-shellspec](https://github.com/poikilotherm/asdf-shellspec) |
| shfmt | [aqua:mvdan/sh](https://github.com/mvdan/sh) [asdf:luizm/asdf-shfmt](https://github.com/luizm/asdf-shfmt) |
| shorebird | [asdf:valian-ca/asdf-shorebird](https://github.com/valian-ca/asdf-shorebird) |
| sinker | [aqua:plexsystems/sinker](https://github.com/plexsystems/sinker) [asdf:elementalvoid/asdf-sinker](https://github.com/elementalvoid/asdf-sinker) |
| skaffold | [aqua:GoogleContainerTools/skaffold](https://github.com/GoogleContainerTools/skaffold) [asdf:nklmilojevic/asdf-skaffold](https://github.com/nklmilojevic/asdf-skaffold) |
| skate | [aqua:charmbracelet/skate](https://github.com/charmbracelet/skate) [asdf:chessmango/asdf-skate](https://github.com/chessmango/asdf-skate) |
| sloth | [aqua:slok/sloth](https://github.com/slok/sloth) [asdf:slok/asdf-sloth](https://github.com/slok/asdf-sloth) |
| slsa-verifier | [ubi:slsa-framework/slsa-verifier](https://github.com/slsa-framework/slsa-verifier) |
| smithy | [asdf:aws/asdf-smithy](https://github.com/aws/asdf-smithy) |
| smlnj | [asdf:samontea/asdf-smlnj](https://github.com/samontea/asdf-smlnj) |
| snyk | [aqua:snyk/cli](https://github.com/snyk/cli) [asdf:nirfuchs/asdf-snyk](https://github.com/nirfuchs/asdf-snyk) |
| soft-serve | [asdf:chessmango/asdf-soft-serve](https://github.com/chessmango/asdf-soft-serve) |
| solidity | [asdf:diegodorado/asdf-solidity](https://github.com/diegodorado/asdf-solidity) |
| sonobuoy | [asdf:Nick-Triller/asdf-sonobuoy](https://github.com/Nick-Triller/asdf-sonobuoy) |
| sops | [ubi:getsops/sops](https://github.com/getsops/sops) [asdf:mise-plugins/mise-sops](https://github.com/mise-plugins/mise-sops) |
| sopstool | [aqua:ibotta/sopstool](https://github.com/ibotta/sopstool) [asdf:elementalvoid/asdf-sopstool](https://github.com/elementalvoid/asdf-sopstool) |
| soracom | [asdf:gr1m0h/asdf-soracom](https://github.com/gr1m0h/asdf-soracom) |
| sourcery | [asdf:younke/asdf-sourcery](https://github.com/younke/asdf-sourcery) |
| spacectl | [aqua:spacelift-io/spacectl](https://github.com/spacelift-io/spacectl) [asdf:bodgit/asdf-spacectl](https://github.com/bodgit/asdf-spacectl) |
| spago | [asdf:jrrom/asdf-spago](https://github.com/jrrom/asdf-spago) |
| spark | [asdf:joshuaballoch/asdf-spark](https://github.com/joshuaballoch/asdf-spark) |
| spectral | [aqua:stoplightio/spectral](https://github.com/stoplightio/spectral) [asdf:vbyrd/asdf-spectral](https://github.com/vbyrd/asdf-spectral) |
| spin | [aqua:spinnaker/spin](https://github.com/spinnaker/spin) [asdf:pavloos/asdf-spin](https://github.com/pavloos/asdf-spin) |
| spring-boot | [asdf:joschi/asdf-spring-boot](https://github.com/joschi/asdf-spring-boot) |
| spruce | [aqua:geofffranks/spruce](https://github.com/geofffranks/spruce) [asdf:woneill/asdf-spruce](https://github.com/woneill/asdf-spruce) |
| sqldef | [asdf:cometkim/asdf-sqldef](https://github.com/cometkim/asdf-sqldef) |
| sqlite | [asdf:cLupus/asdf-sqlite](https://github.com/cLupus/asdf-sqlite) |
| sshuttle | [asdf:xanmanning/asdf-sshuttle](https://github.com/xanmanning/asdf-sshuttle) |
| stack | [aqua:commercialhaskell/stack](https://github.com/commercialhaskell/stack) [asdf:sestrella/asdf-ghcup](https://github.com/sestrella/asdf-ghcup) |
| starboard | [aqua:aquasecurity/starboard](https://github.com/aquasecurity/starboard) [asdf:zufardhiyaulhaq/asdf-starboard](https://github.com/zufardhiyaulhaq/asdf-starboard) |
| starknet-foundry | [asdf:foundry-rs/asdf-starknet-foundry](https://github.com/foundry-rs/asdf-starknet-foundry) |
| starport | [asdf:nikever/asdf-starport](https://github.com/nikever/asdf-starport) |
| starship | [ubi:starship/starship](https://github.com/starship/starship) [asdf:gr1m0h/asdf-starship](https://github.com/gr1m0h/asdf-starship) |
| staticcheck | [aqua:dominikh/go-tools/staticcheck](https://github.com/dominikh/go-tools/staticcheck) [asdf:pbr0ck3r/asdf-staticcheck](https://github.com/pbr0ck3r/asdf-staticcheck) |
| steampipe | [aqua:turbot/steampipe](https://github.com/turbot/steampipe) [asdf:carnei-ro/asdf-steampipe](https://github.com/carnei-ro/asdf-steampipe) |
| step | [asdf:log2/asdf-step](https://github.com/log2/asdf-step) |
| stern | [aqua:stern/stern](https://github.com/stern/stern) [asdf:looztra/asdf-stern](https://github.com/looztra/asdf-stern) |
| stripe | [aqua:stripe/stripe-cli](https://github.com/stripe/stripe-cli) [asdf:offbyone/asdf-stripe](https://github.com/offbyone/asdf-stripe) |
| stylua | [aqua:JohnnyMorganz/StyLua](https://github.com/JohnnyMorganz/StyLua) [asdf:jc00ke/asdf-stylua](https://github.com/jc00ke/asdf-stylua) |
| sui | [asdf:placeholder-soft/asdf-sui](https://github.com/placeholder-soft/asdf-sui) |
| superfile | [aqua:yorukot/superfile](https://github.com/yorukot/superfile) |
| sver | [aqua:mitoma/sver](https://github.com/mitoma/sver) [asdf:robzr/asdf-sver](https://github.com/robzr/asdf-sver) |
| svu | [aqua:caarlos0/svu](https://github.com/caarlos0/svu) [asdf:asdf-community/asdf-svu](https://github.com/asdf-community/asdf-svu) |
| swag | [aqua:swaggo/swag](https://github.com/swaggo/swag) [asdf:behoof4mind/asdf-swag](https://github.com/behoof4mind/asdf-swag) |
| swift | [core:swift](https://mise.jdx.dev/lang/swift.html) |
| swift-package-list | [asdf:MacPaw/asdf-swift-package-list](https://github.com/MacPaw/asdf-swift-package-list) |
| swiftformat | [asdf:younke/asdf-swiftformat](https://github.com/younke/asdf-swiftformat) |
| swiftgen | [asdf:younke/asdf-swiftgen](https://github.com/younke/asdf-swiftgen) |
| swiftlint | [asdf:klundberg/asdf-swiftlint](https://github.com/klundberg/asdf-swiftlint) |
| swiprolog | [asdf:mracos/asdf-swiprolog](https://github.com/mracos/asdf-swiprolog) |
| syft | [aqua:anchore/syft](https://github.com/anchore/syft) [asdf:davidgp1701/asdf-syft](https://github.com/davidgp1701/asdf-syft) |
| syncher | [asdf:nwillc/syncher](https://github.com/nwillc/syncher) |
| talhelper | [aqua:budimanjojo/talhelper](https://github.com/budimanjojo/talhelper) [asdf:bjw-s/asdf-talhelper](https://github.com/bjw-s/asdf-talhelper) |
| talos | [ubi:siderolabs/talos](https://github.com/siderolabs/talos) [asdf:particledecay/asdf-talos](https://github.com/particledecay/asdf-talos) |
| talosctl | [ubi:siderolabs/talos](https://github.com/siderolabs/talos) [asdf:bjw-s/asdf-talosctl](https://github.com/bjw-s/asdf-talosctl) |
| tanka | [aqua:grafana/tanka](https://github.com/grafana/tanka) [asdf:trotttrotttrott/asdf-tanka](https://github.com/trotttrotttrott/asdf-tanka) |
| tanzu | [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| taplo | [ubi:tamasfe/taplo](https://github.com/tamasfe/taplo) [cargo:taplo-cli](https://crates.io/crates/taplo-cli) |
| task | [ubi:go-task/task](https://github.com/go-task/task) [asdf:particledecay/asdf-task](https://github.com/particledecay/asdf-task) |
| tctl | [aqua:temporalio/tctl](https://github.com/temporalio/tctl) [asdf:eko/asdf-tctl](https://github.com/eko/asdf-tctl) |
| tekton | [asdf:johnhamelink/asdf-tekton-cli](https://github.com/johnhamelink/asdf-tekton-cli) |
| teleport-community | [asdf:MaloPolese/asdf-teleport-community](https://github.com/MaloPolese/asdf-teleport-community) |
| teleport-ent | [asdf:highb/asdf-teleport-ent](https://github.com/highb/asdf-teleport-ent) |
| telepresence | [aqua:telepresenceio/telepresence](https://github.com/telepresenceio/telepresence) [asdf:pirackr/asdf-telepresence](https://github.com/pirackr/asdf-telepresence) |
| teller | [aqua:tellerops/teller](https://github.com/tellerops/teller) [asdf:pdemagny/asdf-teller](https://github.com/pdemagny/asdf-teller) |
| temporal | [aqua:temporalio/temporal](https://github.com/temporalio/temporal) [asdf:asdf-community/asdf-temporal](https://github.com/asdf-community/asdf-temporal) |
| temporalite | [asdf:eko/asdf-temporalite](https://github.com/eko/asdf-temporalite) |
| terradozer | [aqua:jckuester/terradozer](https://github.com/jckuester/terradozer) [asdf:chessmango/asdf-terradozer](https://github.com/chessmango/asdf-terradozer) |
| terraform | [aqua:hashicorp/terraform](https://github.com/hashicorp/terraform) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) [vfox:enochchau/vfox-terraform](https://github.com/enochchau/vfox-terraform) |
| terraform-docs | [aqua:terraform-docs/terraform-docs](https://github.com/terraform-docs/terraform-docs) [asdf:looztra/asdf-terraform-docs](https://github.com/looztra/asdf-terraform-docs) |
| terraform-ls | [aqua:hashicorp/terraform-ls](https://github.com/hashicorp/terraform-ls) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| terraform-lsp | [aqua:juliosueiras/terraform-lsp](https://github.com/juliosueiras/terraform-lsp) [asdf:bartlomiejdanek/asdf-terraform-lsp](https://github.com/bartlomiejdanek/asdf-terraform-lsp) |
| terraform-validator | [aqua:thazelart/terraform-validator](https://github.com/thazelart/terraform-validator) [asdf:looztra/asdf-terraform-validator](https://github.com/looztra/asdf-terraform-validator) |
| terraformer | [aqua:GoogleCloudPlatform/terraformer](https://github.com/GoogleCloudPlatform/terraformer) [asdf:gr1m0h/asdf-terraformer](https://github.com/gr1m0h/asdf-terraformer) |
| terragrunt | [aqua:gruntwork-io/terragrunt](https://github.com/gruntwork-io/terragrunt) [asdf:gruntwork-io/asdf-terragrunt](https://github.com/gruntwork-io/asdf-terragrunt) |
| terramate | [aqua:terramate-io/terramate](https://github.com/terramate-io/terramate) [asdf:martinlindner/asdf-terramate](https://github.com/martinlindner/asdf-terramate) |
| terrascan | [aqua:tenable/terrascan](https://github.com/tenable/terrascan) [asdf:hpdobrica/asdf-terrascan](https://github.com/hpdobrica/asdf-terrascan) |
| tf-summarize | [aqua:dineshba/tf-summarize](https://github.com/dineshba/tf-summarize) [asdf:adamcrews/asdf-tf-summarize](https://github.com/adamcrews/asdf-tf-summarize) |
| tfc-agent | [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| tfctl | [aqua:flux-iac/tofu-controller/tfctl](https://github.com/flux-iac/tofu-controller/tfctl) [asdf:deas/asdf-tfctl](https://github.com/deas/asdf-tfctl) |
| tfenv | [aqua:tfutils/tfenv](https://github.com/tfutils/tfenv) [asdf:carlduevel/asdf-tfenv](https://github.com/carlduevel/asdf-tfenv) |
| tflint | [aqua:terraform-linters/tflint](https://github.com/terraform-linters/tflint) [ubi:terraform-linters/tflint](https://github.com/terraform-linters/tflint) [asdf:skyzyx/asdf-tflint](https://github.com/skyzyx/asdf-tflint) |
| tfmigrate | [aqua:minamijoyo/tfmigrate](https://github.com/minamijoyo/tfmigrate) [asdf:dex4er/asdf-tfmigrate](https://github.com/dex4er/asdf-tfmigrate) |
| tfnotify | [aqua:mercari/tfnotify](https://github.com/mercari/tfnotify) [asdf:jnavarrof/asdf-tfnotify](https://github.com/jnavarrof/asdf-tfnotify) |
| tfsec | [aqua:aquasecurity/tfsec](https://github.com/aquasecurity/tfsec) [asdf:woneill/asdf-tfsec](https://github.com/woneill/asdf-tfsec) |
| tfstate-lookup | [aqua:fujiwara/tfstate-lookup](https://github.com/fujiwara/tfstate-lookup) [asdf:carnei-ro/asdf-tfstate-lookup](https://github.com/carnei-ro/asdf-tfstate-lookup) |
| tfswitch | [asdf:iul1an/asdf-tfswitch](https://github.com/iul1an/asdf-tfswitch) |
| tfupdate | [aqua:minamijoyo/tfupdate](https://github.com/minamijoyo/tfupdate) [asdf:yuokada/asdf-tfupdate](https://github.com/yuokada/asdf-tfupdate) |
| thrift | [asdf:alisaifee/asdf-thrift](https://github.com/alisaifee/asdf-thrift) |
| tilt | [aqua:tilt-dev/tilt](https://github.com/tilt-dev/tilt) [asdf:eaceaser/asdf-tilt](https://github.com/eaceaser/asdf-tilt) |
| timoni | [aqua:stefanprodan/timoni](https://github.com/stefanprodan/timoni) [asdf:Smana/asdf-timoni](https://github.com/Smana/asdf-timoni) |
| tiny | [asdf:mise-plugins/mise-tiny](https://github.com/mise-plugins/mise-tiny) |
| tinytex | [asdf:Fbrisset/asdf-tinytex](https://github.com/Fbrisset/asdf-tinytex) |
| titan | [asdf:gabitchov/asdf-titan](https://github.com/gabitchov/asdf-titan) |
| tmux | [asdf:Dabolus/asdf-tmux](https://github.com/Dabolus/asdf-tmux) |
| tokei | [ubi:XAMPPRocky/tokei](https://github.com/XAMPPRocky/tokei) [asdf:gasuketsu/asdf-tokei](https://github.com/gasuketsu/asdf-tokei) |
| tomcat | [asdf:mbutov/asdf-tomcat](https://github.com/mbutov/asdf-tomcat) |
| tonnage | [asdf:elementalvoid/asdf-tonnage](https://github.com/elementalvoid/asdf-tonnage) |
| traefik | [asdf:Dabolus/asdf-traefik](https://github.com/Dabolus/asdf-traefik) |
| trdsql | [aqua:noborus/trdsql](https://github.com/noborus/trdsql) [asdf:johnlayton/asdf-trdsql](https://github.com/johnlayton/asdf-trdsql) |
| tree-sitter | [aqua:tree-sitter/tree-sitter](https://github.com/tree-sitter/tree-sitter) [asdf:ivanvc/asdf-tree-sitter](https://github.com/ivanvc/asdf-tree-sitter) |
| tridentctl | [aqua:NetApp/trident/tridentctl](https://github.com/NetApp/trident/tridentctl) [asdf:asdf-community/asdf-tridentctl](https://github.com/asdf-community/asdf-tridentctl) |
| trivy | [aqua:aquasecurity/trivy](https://github.com/aquasecurity/trivy) [asdf:zufardhiyaulhaq/asdf-trivy](https://github.com/zufardhiyaulhaq/asdf-trivy) |
| tsuru | [asdf:virtualstaticvoid/asdf-tsuru](https://github.com/virtualstaticvoid/asdf-tsuru) |
| ttyd | [aqua:tsl0922/ttyd](https://github.com/tsl0922/ttyd) [asdf:ivanvc/asdf-ttyd](https://github.com/ivanvc/asdf-ttyd) |
| tuist | [asdf:asdf-community/asdf-tuist](https://github.com/asdf-community/asdf-tuist) |
| tx | [asdf:ORCID/asdf-transifex](https://github.com/ORCID/asdf-transifex) |
| typos | [aqua:crate-ci/typos](https://github.com/crate-ci/typos) [asdf:aschiavon91/asdf-typos](https://github.com/aschiavon91/asdf-typos) |
| typst | [aqua:typst/typst](https://github.com/typst/typst) [asdf:stephane-klein/asdf-typst](https://github.com/stephane-klein/asdf-typst) |
| uaa | [ubi:cloudfoundry/uaa-cli](https://github.com/cloudfoundry/uaa-cli) [asdf:vmware-tanzu/tanzu-plug-in-for-asdf](https://github.com/vmware-tanzu/tanzu-plug-in-for-asdf) |
| ubi | [ubi:houseabsolute/ubi](https://github.com/houseabsolute/ubi) |
| unison | [asdf:susurri/asdf-unison](https://github.com/susurri/asdf-unison) |
| upctl | [aqua:UpCloudLtd/upcloud-cli](https://github.com/UpCloudLtd/upcloud-cli) |
| updatecli | [aqua:updatecli/updatecli](https://github.com/updatecli/updatecli) [asdf:updatecli/asdf-updatecli](https://github.com/updatecli/asdf-updatecli) |
| upt | [asdf:ORCID/asdf-upt](https://github.com/ORCID/asdf-upt) |
| upx | [asdf:jimmidyson/asdf-upx](https://github.com/jimmidyson/asdf-upx) |
| usage | [ubi:jdx/usage](https://github.com/jdx/usage) [asdf:jdx/mise-usage](https://github.com/jdx/mise-usage) |
| usql | [aqua:xo/usql](https://github.com/xo/usql) [asdf:itspngu/asdf-usql](https://github.com/itspngu/asdf-usql) |
| uv | [aqua:astral-sh/uv](https://github.com/astral-sh/uv) [asdf:asdf-community/asdf-uv](https://github.com/asdf-community/asdf-uv) |
| v | [asdf:jthegedus/asdf-v](https://github.com/jthegedus/asdf-v) |
| vacuum | [aqua:daveshanley/vacuum](https://github.com/daveshanley/vacuum) |
| vale | [aqua:errata-ai/vale](https://github.com/errata-ai/vale) [asdf:pdemagny/asdf-vale](https://github.com/pdemagny/asdf-vale) |
| vals | [aqua:helmfile/vals](https://github.com/helmfile/vals) [asdf:dex4er/asdf-vals](https://github.com/dex4er/asdf-vals) |
| vault | [aqua:hashicorp/vault](https://github.com/hashicorp/vault) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| vcluster | [aqua:loft-sh/vcluster](https://github.com/loft-sh/vcluster) [asdf:https://gitlab.com/wt0f/asdf-vcluster](https://gitlab.com/wt0f/asdf-vcluster) |
| vela | [asdf:pdemagny/asdf-vela](https://github.com/pdemagny/asdf-vela) |
| velad | [asdf:pdemagny/asdf-velad](https://github.com/pdemagny/asdf-velad) |
| velero | [aqua:vmware-tanzu/velero](https://github.com/vmware-tanzu/velero) [asdf:looztra/asdf-velero](https://github.com/looztra/asdf-velero) |
| vendir | [aqua:carvel-dev/vendir](https://github.com/carvel-dev/vendir) [asdf:vmware-tanzu/asdf-carvel](https://github.com/vmware-tanzu/asdf-carvel) |
| venom | [aqua:ovh/venom](https://github.com/ovh/venom) [asdf:aabouzaid/asdf-venom](https://github.com/aabouzaid/asdf-venom) |
| vhs | [aqua:charmbracelet/vhs](https://github.com/charmbracelet/vhs) [asdf:chessmango/asdf-vhs](https://github.com/chessmango/asdf-vhs) |
| viddy | [aqua:sachaos/viddy](https://github.com/sachaos/viddy) [asdf:ryodocx/asdf-viddy](https://github.com/ryodocx/asdf-viddy) |
| vim | [asdf:tsuyoshicho/asdf-vim](https://github.com/tsuyoshicho/asdf-vim) |
| virtualos | [asdf:tuist/asdf-virtualos](https://github.com/tuist/asdf-virtualos) |
| vivid | [ubi:sharkdp/vivid](https://github.com/sharkdp/vivid) |
| vlang | [vfox:ahai-code/vfox-vlang](https://github.com/ahai-code/vfox-vlang) |
| vlt | [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| vultr | [ubi:vultr/vultr-cli](https://github.com/vultr/vultr-cli) [asdf:ikuradon/asdf-vultr-cli](https://github.com/ikuradon/asdf-vultr-cli) |
| wait-for-gh-rate-limit | [ubi:jdx/wait-for-gh-rate-limit](https://github.com/jdx/wait-for-gh-rate-limit) |
| wasi-sdk | [asdf:coolreader18/asdf-wasi-sdk](https://github.com/coolreader18/asdf-wasi-sdk) |
| wasm3 | [asdf:tachyonicbytes/asdf-wasm3](https://github.com/tachyonicbytes/asdf-wasm3) |
| wasm4 | [ubi:aduros/wasm4](https://github.com/aduros/wasm4) [asdf:jtakakura/asdf-wasm4](https://github.com/jtakakura/asdf-wasm4) |
| wasmer | [aqua:wasmerio/wasmer](https://github.com/wasmerio/wasmer) [asdf:tachyonicbytes/asdf-wasmer](https://github.com/tachyonicbytes/asdf-wasmer) |
| wasmtime | [aqua:bytecodealliance/wasmtime](https://github.com/bytecodealliance/wasmtime) [asdf:tachyonicbytes/asdf-wasmtime](https://github.com/tachyonicbytes/asdf-wasmtime) |
| watchexec | [ubi:watchexec/watchexec](https://github.com/watchexec/watchexec) [asdf:nyrst/asdf-watchexec](https://github.com/nyrst/asdf-watchexec) |
| waypoint | [aqua:hashicorp/waypoint](https://github.com/hashicorp/waypoint) [asdf:asdf-community/asdf-hashicorp](https://github.com/asdf-community/asdf-hashicorp) |
| weave-gitops | [asdf:deas/asdf-weave-gitops](https://github.com/deas/asdf-weave-gitops) |
| websocat | [aqua:vi/websocat](https://github.com/vi/websocat) [asdf:bdellegrazie/asdf-websocat](https://github.com/bdellegrazie/asdf-websocat) |
| wren | [ubi:wren-lang/wren-cli](https://github.com/wren-lang/wren-cli) [asdf:jtakakura/asdf-wren-cli](https://github.com/jtakakura/asdf-wren-cli) |
| wrk | [asdf:ivanvc/asdf-wrk](https://github.com/ivanvc/asdf-wrk) |
| wtfutil | [asdf:NeoHsu/asdf-wtfutil](https://github.com/NeoHsu/asdf-wtfutil) |
| xc | [aqua:joerdav/xc](https://github.com/joerdav/xc) [asdf:airtonix/asdf-xc](https://github.com/airtonix/asdf-xc) |
| xcbeautify | [asdf:mise-plugins/asdf-xcbeautify](https://github.com/mise-plugins/asdf-xcbeautify) |
| xchtmlreport | [asdf:younke/asdf-xchtmlreport](https://github.com/younke/asdf-xchtmlreport) |
| xcodegen | [asdf:younke/asdf-xcodegen](https://github.com/younke/asdf-xcodegen) |
| xcodes | [asdf:younke/asdf-xcodes](https://github.com/younke/asdf-xcodes) |
| xcresultparser | [asdf:MacPaw/asdf-xcresultparser](https://github.com/MacPaw/asdf-xcresultparser) |
| xh | [aqua:ducaale/xh](https://github.com/ducaale/xh) [ubi:ducaale/xh](https://github.com/ducaale/xh) [asdf:NeoHsu/asdf-xh](https://github.com/NeoHsu/asdf-xh) |
| yadm | [asdf:particledecay/asdf-yadm](https://github.com/particledecay/asdf-yadm) |
| yamlfmt | [aqua:google/yamlfmt](https://github.com/google/yamlfmt) [asdf:mise-plugins/asdf-yamlfmt](https://github.com/mise-plugins/asdf-yamlfmt) |
| yamllint | [pipx:yamllint](https://pypi.org/project/yamllint) [asdf:ericcornelissen/asdf-yamllint](https://github.com/ericcornelissen/asdf-yamllint) |
| yamlscript | [asdf:FeryET/asdf-yamlscript](https://github.com/FeryET/asdf-yamlscript) |
| yarn | [asdf:mise-plugins/asdf-yarn](https://github.com/mise-plugins/asdf-yarn) |
| yay | [asdf:aaaaninja/asdf-yay](https://github.com/aaaaninja/asdf-yay) |
| yazi | [aqua:sxyazi/yazi](https://github.com/sxyazi/yazi) |
| yj | [ubi:sclevine/yj](https://github.com/sclevine/yj) [asdf:ryodocx/asdf-yj](https://github.com/ryodocx/asdf-yj) |
| yor | [aqua:bridgecrewio/yor](https://github.com/bridgecrewio/yor) [asdf:ordinaryexperts/asdf-yor](https://github.com/ordinaryexperts/asdf-yor) |
| youtube-dl | [asdf:iul1an/asdf-youtube-dl](https://github.com/iul1an/asdf-youtube-dl) |
| yq | [ubi:mikefarah/yq](https://github.com/mikefarah/yq) [asdf:sudermanjr/asdf-yq](https://github.com/sudermanjr/asdf-yq) |
| yt-dlp | [asdf:duhow/asdf-yt-dlp](https://github.com/duhow/asdf-yt-dlp) |
| ytt | [aqua:carvel-dev/ytt](https://github.com/carvel-dev/ytt) [asdf:vmware-tanzu/asdf-carvel](https://github.com/vmware-tanzu/asdf-carvel) |
| zbctl | [asdf:camunda-community-hub/asdf-zbctl](https://github.com/camunda-community-hub/asdf-zbctl) |
| zellij | [ubi:zellij-org/zellij](https://github.com/zellij-org/zellij) [asdf:chessmango/asdf-zellij](https://github.com/chessmango/asdf-zellij) |
| zephyr | [asdf:nsaunders/asdf-zephyr](https://github.com/nsaunders/asdf-zephyr) |
| zig | [core:zig](https://mise.jdx.dev/lang/zig.html) |
| zigmod | [ubi:nektro/zigmod](https://github.com/nektro/zigmod) [asdf:mise-plugins/asdf-zigmod](https://github.com/mise-plugins/asdf-zigmod) |
| zls | [aqua:zigtools/zls](https://github.com/zigtools/zls) [ubi:zigtools/zls](https://github.com/zigtools/zls) |
| zola | [ubi:getzola/zola](https://github.com/getzola/zola) [asdf:salasrod/asdf-zola](https://github.com/salasrod/asdf-zola) |
| zoxide | [ubi:ajeetdsouza/zoxide](https://github.com/ajeetdsouza/zoxide) [asdf:nyrst/asdf-zoxide](https://github.com/nyrst/asdf-zoxide) |
| zprint | [asdf:carlduevel/asdf-zprint](https://github.com/carlduevel/asdf-zprint) |
