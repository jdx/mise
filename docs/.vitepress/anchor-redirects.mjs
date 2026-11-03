// Sections that moved off a page that still exists, from `/page.html#old-id`
// to `/new-page.html#new-id`. Append-only: published links and released mise
// binaries point at the old ids. The browser loads this file only when a page
// opens with a hash that matches no element on it (see theme/index.ts).

/** @type {Record<string, string>} */
export const anchorRedirects = {
  "/about.html#where-to-start": "/getting-started.html",
  "/architecture.html#caching-system": "/architecture.html#where-things-live",
  "/architecture.html#command-layer": "/architecture.html#where-things-live",
  "/architecture.html#core-architecture-components":
    "/architecture.html#where-things-live",
  "/architecture.html#end-to-end-tests": "/contributing.html#e2e-tests",
  "/architecture.html#environment-management":
    "/architecture.html#shell-integration",
  "/architecture.html#plugin-system": "/architecture.html#plugins",
  "/architecture.html#related-architecture-documentation":
    "/architecture.html#where-things-live",
  "/architecture.html#snapshot-testing": "/contributing.html#snapshot-testing",
  "/architecture.html#test-architecture": "/contributing.html#testing",
  "/architecture.html#test-infrastructure-features":
    "/contributing.html#e2e-tests",
  "/architecture.html#unit-tests": "/contributing.html#unit-tests",
  "/architecture.html#windows-testing": "/contributing.html#windows-e2e-tests",
  "/asdf-legacy-plugins.html#best-practices":
    "/asdf-legacy-plugins.html#example-plugin",
  "/asdf-legacy-plugins.html#community-resources":
    "/asdf-legacy-plugins.html#installing-asdf-legacy-plugins",
  "/asdf-legacy-plugins.html#next-steps":
    "/asdf-legacy-plugins.html#migration-path",
  "/asdf-legacy-plugins.html#optional-scripts":
    "/asdf-legacy-plugins.html#plugin-structure",
  "/asdf-legacy-plugins.html#required-scripts":
    "/asdf-legacy-plugins.html#plugin-structure",
  "/asdf-legacy-plugins.html#what-are-asdf-legacy-plugins":
    "/asdf-legacy-plugins.html",
  "/asdf-legacy-plugins.html#when-to-use-asdf-legacy-plugins":
    "/asdf-legacy-plugins.html#migration-path",
  "/backend-plugin-development.html#_1-plugin-structure":
    "/backend-plugin-development.html#quick-start",
  "/backend-plugin-development.html#_2-basic-metadata-lua":
    "/backend-plugin-development.html#metadata-lua",
  "/backend-plugin-development.html#advanced-features":
    "/backend-plugin-development.html#backendinstall",
  "/backend-plugin-development.html#backendexecenv-context":
    "/backend-plugin-development.html#backendexecenv",
  "/backend-plugin-development.html#backendinstall-context":
    "/backend-plugin-development.html#backendinstall",
  "/backend-plugin-development.html#backendlisttools-context":
    "/backend-plugin-development.html#backendlisttools",
  "/backend-plugin-development.html#backendlistversions-context":
    "/backend-plugin-development.html#backendlistversions",
  "/backend-plugin-development.html#backendsearchtools-context":
    "/backend-plugin-development.html#backendsearchtools",
  "/backend-plugin-development.html#backenduninstall-context":
    "/backend-plugin-development.html#backenduninstall",
  "/backend-plugin-development.html#best-practices":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#caching":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#conditional-installation":
    "/backend-plugin-development.html#backendinstall",
  "/backend-plugin-development.html#context-variables":
    "/backend-plugin-development.html#tool-options",
  "/backend-plugin-development.html#creating-a-backend-plugin":
    "/backend-plugin-development.html#quick-start",
  "/backend-plugin-development.html#cross-platform-commands":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#debug-mode":
    "/backend-plugin-development.html#testing-your-plugin",
  "/backend-plugin-development.html#environment-detection":
    "/plugin-lua-modules.html#runtime",
  "/backend-plugin-development.html#error-handling":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#hooks-backend-exec-env-lua":
    "/backend-plugin-development.html#complete-example",
  "/backend-plugin-development.html#hooks-backend-install-lua":
    "/backend-plugin-development.html#complete-example",
  "/backend-plugin-development.html#hooks-backend-list-versions-lua":
    "/backend-plugin-development.html#complete-example",
  "/backend-plugin-development.html#local-development":
    "/backend-plugin-development.html#testing-your-plugin",
  "/backend-plugin-development.html#multiple-environment-variables":
    "/backend-plugin-development.html#backendexecenv",
  "/backend-plugin-development.html#next-steps": "/plugin-publishing.html",
  "/backend-plugin-development.html#path-handling":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#performance-optimization":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#plugin-architecture":
    "/backend-plugin-development.html#backend-methods",
  "/backend-plugin-development.html#real-world-example-vfox-npm":
    "/backend-plugin-development.html#complete-example",
  "/backend-plugin-development.html#regex-parsing":
    "/backend-plugin-development.html#common-mistakes",
  "/backend-plugin-development.html#usage-example":
    "/backend-plugin-development.html#complete-example",
  "/backend-plugin-development.html#using-the-template-repository":
    "/backend-plugin-development.html#quick-start",
  "/backend-plugin-development.html#what-are-backend-plugins":
    "/backend-plugin-development.html#quick-start",
  "/bootstrap.html#a-bootstrap-project":
    "/bootstrap/from-repository.html#a-bootstrap-project",
  "/bootstrap.html#add-a-package": "/cli/bootstrap/packages/use.html",
  "/bootstrap.html#advanced-self-managing-config":
    "/dotfiles/managed.html#self-managing-mise-config",
  "/bootstrap.html#capture-an-edited-dotfile":
    "/dotfiles/managed.html#capturing-changes",
  "/bootstrap.html#common-workflows": "/bootstrap.html#next-steps",
  "/bootstrap.html#define-a-module": "/bootstrap/modules.html#define-a-module",
  "/bootstrap.html#edit-a-managed-dotfile":
    "/dotfiles/managed.html#capturing-changes",
  "/bootstrap.html#example": "/bootstrap.html#quick-start",
  "/bootstrap.html#global-mise-configuration":
    "/bootstrap/from-repository.html#global-mise-configuration",
  "/bootstrap.html#how-modules-combine":
    "/bootstrap/modules.html#how-modules-combine",
  "/bootstrap.html#how-removal-is-planned":
    "/bootstrap/modules.html#how-removal-is-planned",
  "/bootstrap.html#modules": "/bootstrap/modules.html",
  "/bootstrap.html#new-machine": "/bootstrap.html#quick-start",
  "/bootstrap.html#remove-a-module-s-resources":
    "/bootstrap/modules.html#remove-a-module-s-resources",
  "/bootstrap.html#resources-that-need-separate-cleanup":
    "/bootstrap/modules.html#resources-that-need-separate-cleanup",
  "/bootstrap.html#select-and-preview-modules":
    "/bootstrap/modules.html#select-and-preview-modules",
  "/bootstrap.html#shared-dotfile-history":
    "/bootstrap/from-repository.html#shared-dotfile-history",
  "/bootstrap/accounts.html#commands":
    "/bootstrap/accounts.html#preview-and-apply",
  "/bootstrap/accounts.html#removal":
    "/bootstrap/accounts.html#remove-users-and-groups",
  "/bootstrap/compose.html#apply-policy": "/bootstrap/compose.html#reference",
  "/bootstrap/compose.html#preview-the-whole-setup":
    "/bootstrap/compose.html#preview-and-apply",
  "/bootstrap/files.html#commands": "/bootstrap/files.html#preview-and-apply",
  "/bootstrap/files.html#preview-and-inspect":
    "/bootstrap/files.html#preview-and-apply",
  "/bootstrap/firewall.html#policy-and-rules":
    "/bootstrap/firewall.html#write-rules",
  "/bootstrap/firewall.html#preview-the-target-s-policy":
    "/bootstrap/firewall.html#preview-and-apply",
  "/bootstrap/launchd.html#commands":
    "/bootstrap/launchd.html#preview-and-apply",
  "/bootstrap/launchd.html#semantics":
    "/bootstrap/launchd.html#how-configs-combine",
  "/bootstrap/launchd.html#supported-keys": "/bootstrap/launchd.html#reference",
  "/bootstrap/macos-defaults.html#commands":
    "/bootstrap/macos-defaults.html#preview-and-apply",
  "/bootstrap/packages/#adopt-an-existing-app":
    "/bootstrap/packages/macos-app.html#adopt-an-existing-app",
  "/bootstrap/packages/#adopt-an-existing-homebrew-cask-app":
    "/bootstrap/packages/brew-cask.html#adopt-an-existing-app",
  "/bootstrap/packages/#declare-a-download":
    "/bootstrap/packages/macos-app.html#declare-a-download",
  "/bootstrap/packages/#host-packages-or-mise-tools": "/bootstrap/packages/",
  "/bootstrap/packages/#macos-apps-without-a-cask":
    "/bootstrap/packages/macos-app.html",
  "/bootstrap/packages/#update-a-declared-app":
    "/bootstrap/packages/macos-app.html#update-a-declared-app",
  "/bootstrap/packages/apk.html#behavior":
    "/bootstrap/packages/apk.html#what-mise-runs",
  "/bootstrap/packages/apk.html#preview-and-apply":
    "/bootstrap/packages/apk.html",
  "/bootstrap/packages/apt.html#architecture-qualified-packages":
    "/bootstrap/packages/apt.html#package-names",
  "/bootstrap/packages/apt.html#behavior":
    "/bootstrap/packages/apt.html#what-mise-runs",
  "/bootstrap/packages/apt.html#preview-and-apply":
    "/bootstrap/packages/apt.html",
  "/bootstrap/packages/brew.html#casks": "/bootstrap/packages/brew-cask.html",
  "/bootstrap/packages/brew.html#first-install":
    "/bootstrap/packages/brew.html",
  "/bootstrap/packages/brew.html#limitations":
    "/bootstrap/packages/brew.html#troubleshooting",
  "/bootstrap/packages/brew.html#linux-font-casks":
    "/bootstrap/packages/brew-cask.html#linux-font-casks",
  "/bootstrap/packages/brew.html#macos-privacy-security-tcc":
    "/bootstrap/packages/brew-cask.html#macos-privacy-security-tcc",
  "/bootstrap/packages/brew.html#overriding-the-application-directory":
    "/bootstrap/packages/brew-cask.html#overriding-the-application-directory",
  "/bootstrap/packages/brew.html#ownership-and-installed-state":
    "/bootstrap/packages/brew-cask.html#casks-installed-by-homebrew",
  "/bootstrap/packages/brew.html#supported-artifacts-and-lifecycle-actions":
    "/bootstrap/packages/brew-cask.html#supported-artifacts",
  "/bootstrap/packages/dnf.html#behavior":
    "/bootstrap/packages/dnf.html#what-mise-runs",
  "/bootstrap/packages/dnf.html#preview-and-apply":
    "/bootstrap/packages/dnf.html",
  "/bootstrap/packages/dnf.html#version-selection":
    "/bootstrap/packages/dnf.html#version-pins",
  "/bootstrap/packages/flatpak.html#commands":
    "/bootstrap/packages/flatpak.html#what-mise-runs",
  "/bootstrap/packages/mas.html#caveats":
    "/bootstrap/packages/mas.html#prerequisites",
  "/bootstrap/packages/mas.html#commands":
    "/bootstrap/packages/mas.html#what-mise-runs",
  "/bootstrap/packages/mas.html#finding-ids":
    "/bootstrap/packages/mas.html#find-an-app-id",
  "/bootstrap/packages/pacman.html#behavior":
    "/bootstrap/packages/pacman.html#what-mise-runs",
  "/bootstrap/packages/pacman.html#preview-and-apply":
    "/bootstrap/packages/pacman.html",
  "/bootstrap/packages/scoop.html#commands":
    "/bootstrap/packages/scoop.html#what-mise-runs",
  "/bootstrap/packages/winget.html#availability-and-scope":
    "/bootstrap/packages/winget.html#prerequisites",
  "/bootstrap/packages/winget.html#commands":
    "/bootstrap/packages/winget.html#what-mise-runs",
  "/bootstrap/packages/zypper.html#behavior":
    "/bootstrap/packages/zypper.html#what-mise-runs",
  "/bootstrap/packages/zypper.html#preview-and-apply":
    "/bootstrap/packages/zypper.html",
  "/bootstrap/packages/zypper.html#transactional-systems":
    "/bootstrap/packages/zypper.html#prerequisites",
  "/bootstrap/packages/zypper.html#version-selection":
    "/bootstrap/packages/zypper.html#version-pins",
  "/bootstrap/remote.html#borrowing-github-access-for-one-session":
    "/bootstrap/github-relay.html",
  "/bootstrap/remote.html#first-remote-run": "/bootstrap/remote.html#first-run",
  "/bootstrap/remote.html#observing-and-limiting-borrowed-access":
    "/bootstrap/github-relay.html#logging",
  "/bootstrap/repos.html#commands": "/bootstrap/repos.html#preview-and-apply",
  "/bootstrap/repos.html#semantics": "/bootstrap/repos.html#protect-local-work",
  "/bootstrap/repos.html#states": "/bootstrap/repos.html#preview-and-apply",
  "/bootstrap/secrets.html#supply-inputs-and-check-availability":
    "/bootstrap/secrets.html#supply-values",
  "/bootstrap/services.html#status-and-apply":
    "/bootstrap/services.html#preview-and-apply",
  "/bootstrap/setup.html#choose-when-to-sync":
    "/dotfiles/sync.html#choose-a-sync-mode",
  "/bootstrap/setup.html#resolve-a-conflict":
    "/dotfiles/sync.html#resolve-a-conflict",
  "/bootstrap/setup.html#use-a-template-optional":
    "/dotfiles/managed.html#templates",
  "/bootstrap/shell.html#commands": "/bootstrap/shell.html#preview-and-apply",
  "/bootstrap/shell.html#semantics":
    "/bootstrap/shell.html#existing-startup-files",
  "/bootstrap/systemd.html#commands":
    "/bootstrap/systemd.html#preview-and-apply",
  "/bootstrap/systemd.html#semantics":
    "/bootstrap/systemd.html#how-configs-combine",
  "/bootstrap/systemd.html#supported-keys": "/bootstrap/systemd.html#reference",
  "/configuration.html#automatic-tool-updates":
    "/dev-tools/#automatic-tool-updates",
  "/configuration.html#configuration-resolution-process":
    "/configuration.html#configuration-hierarchy",
  "/configuration.html#daemon-groups": "/daemons.html#groups",
  "/configuration.html#daemons": "/daemons.html",
  "/configuration.html#daemons-settings": "/daemons/worktrees.html#namespaces",
  "/configuration.html#enabling-idiomatic-version-files":
    "/dev-tools/versions.html#enabling-idiomatic-version-files",
  "/configuration.html#env-arbitrary-environment-variables": "/environments/",
  "/configuration.html#environment-variables":
    "/configuration/environment-variables.html",
  "/configuration.html#example-merging-tool-versions":
    "/configuration.html#configuration-hierarchy",
  "/configuration.html#global-config-config-mise-config-toml":
    "/configuration.html#global-config",
  "/configuration.html#how-configuration-merging-works":
    "/configuration.html#configuration-hierarchy",
  "/configuration.html#mise-cache-dir": "/directories.html#cache-mise",
  "/configuration.html#mise-ceiling-paths":
    "/configuration/settings.html#ceiling_paths",
  "/configuration.html#mise-data-dir": "/directories.html#local-share-mise",
  "/configuration.html#mise-default-config-filename":
    "/configuration/settings.html#default_config_filename",
  "/configuration.html#mise-env-file": "/configuration/settings.html#env_file",
  "/configuration.html#mise-fish-auto-activate-1":
    "/configuration/environment-variables.html#mise-fish-auto-activate",
  "/configuration.html#mise-global-config-file":
    "/configuration/settings.html#global_config_file",
  "/configuration.html#mise-global-config-root":
    "/configuration/settings.html#global_config_root",
  "/configuration.html#mise-http-timeout":
    "/configuration/settings.html#http_timeout",
  "/configuration.html#mise-log-file-level-trace-debug-info-warn-error":
    "/configuration/environment-variables.html#mise-log-file-level",
  "/configuration.html#mise-log-file-mise-log":
    "/configuration/environment-variables.html#mise-log-file",
  "/configuration.html#mise-log-http-1":
    "/configuration/environment-variables.html#mise-log-http",
  "/configuration.html#mise-log-level-trace-debug-info-warn-error":
    "/configuration/environment-variables.html#mise-log-level",
  "/configuration.html#mise-log-verbose-deps-1":
    "/configuration/environment-variables.html#mise-log-verbose-deps",
  "/configuration.html#mise-quiet-1": "/configuration/settings.html#quiet",
  "/configuration.html#mise-raw-1": "/configuration/settings.html#raw",
  "/configuration.html#mise-system-config-dir":
    "/directories.html#system-config",
  "/configuration.html#mise-term-width":
    "/configuration/environment-variables.html#mise-term-width",
  "/configuration.html#mise-tmp-dir":
    "/configuration/environment-variables.html#directories",
  "/configuration.html#mise-tool-version":
    "/configuration/environment-variables.html#mise-tool-version",
  "/configuration.html#mise-trusted-config-paths":
    "/configuration/settings.html#trusted_config_paths",
  "/configuration.html#monorepo-root": "/tasks/monorepo.html",
  "/configuration.html#plugins-specify-custom-plugin-repository-urls":
    "/configuration.html#plugins",
  "/configuration.html#scopes": "/dev-tools/versions.html#scopes",
  "/configuration.html#secrets": "/environments/secrets/fnox.html",
  "/configuration.html#settings": "/configuration/settings.html",
  "/configuration.html#settings-mise-settings": "/configuration/settings.html",
  "/configuration.html#shell-alias-shell-aliases": "/shell-aliases.html",
  "/configuration.html#system-config-etc-mise-config-toml":
    "/configuration.html#global-config",
  "/configuration.html#tasks": "/tasks/",
  "/configuration.html#tasks-run-files-or-shell-scripts": "/tasks/",
  "/configuration.html#tool-alias-tool-version-aliases":
    "/dev-tools/aliases.html",
  "/configuration.html#tool-config-config-root-scoped-tool-policy":
    "/configuration.html#tool-config",
  "/configuration.html#tool-versions": "/dev-tools/versions.html#tool-versions",
  "/configuration.html#tools-dev-tools": "/dev-tools/#tool-options",
  "/configuration.html#vars-configuration-variables":
    "/configuration/vars.html",
  "/configuration.html#visual-configuration-hierarchy":
    "/configuration.html#configuration-hierarchy",
  "/configuration.html#which-fields-mise-reads":
    "/dev-tools/versions.html#which-fields-mise-reads",
  "/configuration/environments.html#local-overrides":
    "/configuration/environments.html#file-names-and-precedence",
  "/configuration/environments.html#personal-environment-selection":
    "/configuration.html#personal-and-machine-wide-choices",
  "/configuration/environments.html#templates-in-miserc-toml":
    "/configuration.html#templates-in-miserc-toml",
  "/configuration/project-diagnostics.html#check-configuration":
    "/configuration/project-diagnostics.html#check-fields",
  "/configuration/project-diagnostics.html#results-and-automation":
    "/configuration/project-diagnostics.html#results",
  "/continuous-integration.html#running-against-untrusted-config-safe-mode":
    "/continuous-integration.html#safe-mode",
  "/contributing.html#adding-a-new-setting":
    "/contributing.html#adding-a-setting",
  "/contributing.html#available-development-tasks":
    "/contributing.html#project-tasks",
  "/contributing.html#available-linters-in-hk":
    "/contributing.html#linting-and-formatting",
  "/contributing.html#backend-acceptance-tiers":
    "/contributing/registry.html#backend-acceptance-tiers",
  "/contributing.html#backend-priority":
    "/contributing/registry.html#backend-entries",
  "/contributing.html#backend-types": "/dev-tools/backends/",
  "/contributing.html#breaking-change-policy":
    "/contributing.html#deprecations-and-breaking-changes",
  "/contributing.html#breaking-changes":
    "/contributing.html#deprecations-and-breaking-changes",
  "/contributing.html#ci-cd-pull-request-automation":
    "/contributing.html#ci-and-releases",
  "/contributing.html#commit-types":
    "/contributing.html#pull-request-titles-and-descriptions",
  "/contributing.html#common-tasks": "/contributing.html#project-tasks",
  "/contributing.html#continuous-integration":
    "/contributing.html#ci-and-releases",
  "/contributing.html#contribution-expectations":
    "/contributing.html#before-you-open-a-pull-request",
  "/contributing.html#conventional-commits":
    "/contributing.html#pull-request-titles-and-descriptions",
  "/contributing.html#coverage-tests": "/contributing.html#e2e-tests",
  "/contributing.html#dependency-management": "/contributing.html#dependencies",
  "/contributing.html#development-setup":
    "/contributing.html#set-up-a-checkout",
  "/contributing.html#development-shim": "/contributing.html#run-your-build",
  "/contributing.html#development-tips": "/contributing.html#run-your-build",
  "/contributing.html#disable-at-build-time":
    "/packaging.html#disable-at-build-time",
  "/contributing.html#disable-with-a-marker-file":
    "/packaging.html#disable-with-a-marker-file",
  "/contributing.html#documentation": "/contributing.html#adding-backends",
  "/contributing.html#documentation-tasks": "/contributing.html#project-tasks",
  "/contributing.html#examples":
    "/contributing.html#pull-request-titles-and-descriptions",
  "/contributing.html#fedora-dnf": "/packaging.html#fedora-dnf",
  "/contributing.html#formatting-and-linting":
    "/contributing.html#before-you-open-a-pull-request",
  "/contributing.html#generating-readme-and-shell-completion-files":
    "/architecture.html#generated-files",
  "/contributing.html#getting-started": "/contributing.html#clone-and-build",
  "/contributing.html#guidelines-and-requirements":
    "/contributing/registry.html#popularity-bar",
  "/contributing.html#hk-configuration":
    "/contributing.html#linting-and-formatting",
  "/contributing.html#idiomatic-version-files":
    "/contributing/registry.html#idiomatic-version-files",
  "/contributing.html#implementation-examples":
    "/contributing.html#adding-backends",
  "/contributing.html#implementation-steps":
    "/contributing.html#adding-backends",
  "/contributing.html#linting": "/contributing.html#linting-and-formatting",
  "/contributing.html#maximum-backend-versions":
    "/contributing/registry.html#maximum-backend-versions",
  "/contributing.html#minimum-backend-versions":
    "/contributing/registry.html#minimum-backend-versions",
  "/contributing.html#overriding-the-outcome":
    "/packaging.html#overriding-the-outcome",
  "/contributing.html#plugin-tests": "/contributing.html#registry-tool-tests",
  "/contributing.html#pr-title-validation":
    "/contributing.html#pull-request-titles-and-descriptions",
  "/contributing.html#pre-commit-hooks-code-quality":
    "/contributing.html#linting-and-formatting",
  "/contributing.html#prerequisites": "/contributing.html#build-dependencies",
  "/contributing.html#pull-request-checklist":
    "/contributing.html#before-you-open-a-pull-request",
  "/contributing.html#quick-start": "/contributing/registry.html#quick-start",
  "/contributing.html#registry-examples":
    "/contributing/registry.html#registry-format",
  "/contributing.html#registry-format":
    "/contributing/registry.html#registry-format",
  "/contributing.html#release-automation": "/contributing.html#ci-and-releases",
  "/contributing.html#release-tasks": "/contributing.html#project-tasks",
  "/contributing.html#releasing": "/contributing.html#ci-and-releases",
  "/contributing.html#required-attestations":
    "/contributing/registry.html#required-attestations",
  "/contributing.html#rhel-dnf": "/packaging.html#rhel-dnf",
  "/contributing.html#running-checks-manually":
    "/contributing.html#linting-and-formatting",
  "/contributing.html#running-individual-tests": "/contributing.html#testing",
  "/contributing.html#running-specific-test-categories":
    "/contributing.html#testing",
  "/contributing.html#running-the-cli": "/contributing.html#run-your-build",
  "/contributing.html#scopes":
    "/contributing.html#pull-request-titles-and-descriptions",
  "/contributing.html#setup": "/contributing.html#clone-and-build",
  "/contributing.html#ship-update-instructions":
    "/packaging.html#ship-update-instructions",
  "/contributing.html#test-environment-setup": "/contributing.html#e2e-tests",
  "/contributing.html#testing-packaging": "/packaging.html#testing-packaging",
  "/contributing.html#testing-requirements":
    "/contributing.html#adding-backends",
  "/contributing.html#tool-testing": "/contributing/registry.html#tool-testing",
  "/contributing.html#ubuntu-apt": "/packaging.html#ubuntu-apt",
  "/contributing.html#using-hk-in-development":
    "/contributing.html#linting-and-formatting",
  "/core-tools.html#language-guides": "/core-tools.html",
  "/daemons.html#choose-what-to-share":
    "/daemons/sharing.html#use-a-provider-from-a-project",
  "/daemons.html#cockroachdb": "/daemons/presets.html#cockroachdb",
  "/daemons.html#configuration-inheritance":
    "/daemons/worktrees.html#namespace-inheritance",
  "/daemons.html#configure-the-base-port-and-spacing":
    "/daemons/worktrees.html#automatic-ports",
  "/daemons.html#daemons-from-another-project":
    "/daemons/sharing.html#use-a-daemon-from-another-project",
  "/daemons.html#data-and-configuration": "/daemons/data.html#where-data-lives",
  "/daemons.html#database-presets": "/daemons/presets.html",
  "/daemons.html#environment-and-lifecycle":
    "/daemons/sharing.html#environment-and-lifecycle",
  "/daemons.html#environment-and-tool-versions":
    "/daemons/presets.html#connection-variables-and-tool-versions",
  "/daemons.html#example-cockroachdb-spicedb-and-nats":
    "/daemons/presets.html#example-cockroachdb-spicedb-and-nats",
  "/daemons.html#git-worktrees":
    "/daemons/worktrees.html#namespaces-in-git-worktrees",
  "/daemons.html#inspecting-storage": "/daemons/data.html#find-daemon-data",
  "/daemons.html#namespaces": "/daemons/worktrees.html#namespaces",
  "/daemons.html#naming-the-project-and-the-worktree":
    "/daemons/worktrees.html#choose-hostname-labels",
  "/daemons.html#nats": "/daemons/presets.html#nats",
  "/daemons.html#non-interactive-cleanup":
    "/daemons/data.html#non-interactive-cleanup",
  "/daemons.html#paths-and-configuration":
    "/daemons/sharing.html#paths-and-configuration",
  "/daemons.html#per-daemon-proxy-settings":
    "/daemons/worktrees.html#per-daemon-proxy-settings",
  "/daemons.html#port-conflicts": "/daemons/worktrees.html#port-conflicts",
  "/daemons.html#port-environment-variables":
    "/daemons/worktrees.html#port-variables",
  "/daemons.html#ports": "/daemons/worktrees.html#ports",
  "/daemons.html#ports-across-git-worktrees":
    "/daemons/worktrees.html#automatic-ports",
  "/daemons.html#postgresql": "/daemons/presets.html#postgresql",
  "/daemons.html#preset-options": "/daemons/presets.html#preset-options",
  "/daemons.html#project-layout-and-port-stability":
    "/daemons/worktrees.html#project-layout-and-port-stability",
  "/daemons.html#pruning-deleted-projects":
    "/daemons/data.html#clean-up-deleted-projects",
  "/daemons.html#recommended-setup": "/daemons/development-stack.html",
  "/daemons.html#redis": "/daemons/presets.html#redis",
  "/daemons.html#register-for-on-demand-startup":
    "/daemons/worktrees.html#register-for-on-demand-startup",
  "/daemons.html#seeing-the-urls": "/daemons/worktrees.html#list-urls",
  "/daemons.html#service-presets": "/daemons/presets.html",
  "/daemons.html#share-nats-without-sharing-messages":
    "/daemons/sharing.html#share-nats-without-sharing-messages",
  "/daemons.html#shared-server-providers":
    "/daemons/sharing.html#shared-server-providers",
  "/daemons.html#spicedb": "/daemons/presets.html#spicedb",
  "/daemons.html#stable-urls-per-worktree":
    "/daemons/worktrees.html#stable-urls-per-worktree",
  "/daemons.html#stop-idle-daemons":
    "/daemons/worktrees.html#stop-idle-daemons",
  "/daemons.html#when-state-is-kept": "/daemons/data.html#when-state-is-kept",
  "/daemons.html#where-the-scheme-and-port-come-from":
    "/daemons/worktrees.html#where-the-scheme-and-port-come-from",
  "/daemons/development-stack.html#separate-worktrees-by-default":
    "/daemons/development-stack.html#add-a-worktree",
  "/daemons/development-stack.html#share-a-service-deliberately":
    "/daemons/development-stack.html#share-a-service",
  "/daemons/development-stack.html#start-on-request-and-stop-when-idle":
    "/daemons/development-stack.html#open-it-by-url",
  "/dev-tools/#command-not-found-handler-shell-integration":
    "/dev-tools/#auto-install-mechanisms",
  "/dev-tools/#common-commands": "/dev-tools/#choose-the-right-command",
  "/dev-tools/#dotted-notation": "/dev-tools/#table-format",
  "/dev-tools/#existing-version-files": "/dev-tools/#how-tools-are-selected",
  "/dev-tools/#generic-nested-support": "/dev-tools/#table-format",
  "/dev-tools/#mise-exec-mise-x": "/dev-tools/#choose-the-right-command",
  "/dev-tools/#mise-install": "/dev-tools/#choose-the-right-command",
  "/dev-tools/#mise-use": "/dev-tools/#choose-the-right-command",
  "/dev-tools/#on-demand-execution-mise-x-mise-r":
    "/dev-tools/#auto-install-mechanisms",
  "/dev-tools/#os-architecture-combinations": "/dev-tools/#os-specific-tools",
  "/dev-tools/#permissions-and-sudo":
    "/dev-tools/system-installs.html#permissions-and-sudo",
  "/dev-tools/#supported-tools":
    "/dev-tools/system-installs.html#supported-backends",
  "/dev-tools/#table-format-recommended": "/dev-tools/#table-format",
  "/dev-tools/#version-ordering": "/dev-tools/versions.html#version-ordering",
  "/dev-tools/#vfox-plugin-hook-dependencies":
    "/tool-plugin-development.html#depends",
  "/dev-tools/backends/asdf.html#feature-comparison-asdf-vs-vfox":
    "/asdf-legacy-plugins.html#feature-comparison-asdf-vs-vfox",
  "/dev-tools/backends/asdf.html#hook-migration-asdf-to-vfox":
    "/asdf-legacy-plugins.html#hook-migration-asdf-to-vfox",
  "/dev-tools/backends/asdf.html#writing-asdf-legacy-plugins-for-mise":
    "/asdf-legacy-plugins.html#environment-variables",
  "/dev-tools/backends/forgejo.html#asset-autodetection":
    "/dev-tools/backends/github.html#asset-autodetection",
  "/dev-tools/backends/forgejo.html#asset-pattern":
    "/dev-tools/backends/github.html#asset-pattern",
  "/dev-tools/backends/forgejo.html#bin": "/dev-tools/backends/github.html#bin",
  "/dev-tools/backends/forgejo.html#bin-path":
    "/dev-tools/backends/github.html#bin-path",
  "/dev-tools/backends/forgejo.html#checksum":
    "/dev-tools/backends/github.html#checksum",
  "/dev-tools/backends/forgejo.html#credential-command":
    "/dev-tools/github-tokens.html#credential-command",
  "/dev-tools/backends/forgejo.html#debugging-token-resolution":
    "/dev-tools/github-tokens.html#check-which-token-mise-uses",
  "/dev-tools/backends/forgejo.html#environment-variables":
    "/dev-tools/github-tokens.html#environment-variables",
  "/dev-tools/backends/forgejo.html#filter-bins":
    "/dev-tools/backends/github.html#filter-bins",
  "/dev-tools/backends/forgejo.html#fj-cli-integration":
    "/dev-tools/github-tokens.html#cli-logins",
  "/dev-tools/backends/forgejo.html#forgejo.credential_command":
    "/configuration/settings.html#forgejo.credential_command",
  "/dev-tools/backends/forgejo.html#forgejo.fj_cli_tokens":
    "/configuration/settings.html#forgejo.fj_cli_tokens",
  "/dev-tools/backends/forgejo.html#forgejo.use_git_credentials":
    "/configuration/settings.html#forgejo.use_git_credentials",
  "/dev-tools/backends/forgejo.html#git-credential-fill-fallback":
    "/dev-tools/github-tokens.html#git-credential-helpers",
  "/dev-tools/backends/forgejo.html#matching":
    "/dev-tools/backends/github.html#matching",
  "/dev-tools/backends/forgejo.html#matching-regex":
    "/dev-tools/backends/github.html#matching-regex",
  "/dev-tools/backends/forgejo.html#no-app":
    "/dev-tools/backends/github.html#no-app",
  "/dev-tools/backends/forgejo.html#platform-specific-asset-patterns":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/forgejo.html#platform-specific-checksums":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/forgejo.html#prerelease":
    "/dev-tools/backends/github.html#prerelease",
  "/dev-tools/backends/forgejo.html#rename-exe":
    "/dev-tools/backends/github.html#rename-exe",
  "/dev-tools/backends/forgejo.html#self-hosted-forgejo":
    "/dev-tools/backends/forgejo.html#other-forgejo-servers",
  "/dev-tools/backends/forgejo.html#size":
    "/dev-tools/backends/github.html#size",
  "/dev-tools/backends/forgejo.html#strip-components":
    "/dev-tools/backends/github.html#strip-components",
  "/dev-tools/backends/forgejo.html#supported-forgejo-syntax":
    "/dev-tools/backends/forgejo.html#usage",
  "/dev-tools/backends/forgejo.html#token-file-forgejo-tokens-toml":
    "/dev-tools/github-tokens.html#token-files",
  "/dev-tools/backends/forgejo.html#token-priority":
    "/dev-tools/github-tokens.html#token-priority",
  "/dev-tools/backends/forgejo.html#version-prefix":
    "/dev-tools/backends/github.html#version-prefix",
  "/dev-tools/backends/github.html#github.credential_command":
    "/configuration/settings.html#github.credential_command",
  "/dev-tools/backends/github.html#github.gh_cli_tokens":
    "/configuration/settings.html#github.gh_cli_tokens",
  "/dev-tools/backends/github.html#github.oauth_api_url":
    "/configuration/settings.html#github.oauth_api_url",
  "/dev-tools/backends/github.html#github.oauth_auth_url":
    "/configuration/settings.html#github.oauth_auth_url",
  "/dev-tools/backends/github.html#github.oauth_client_id":
    "/configuration/settings.html#github.oauth_client_id",
  "/dev-tools/backends/github.html#github.oauth_export_env":
    "/configuration/settings.html#github.oauth_export_env",
  "/dev-tools/backends/github.html#github.oauth_open_browser":
    "/configuration/settings.html#github.oauth_open_browser",
  "/dev-tools/backends/github.html#github.oauth_scopes":
    "/configuration/settings.html#github.oauth_scopes",
  "/dev-tools/backends/github.html#github.use_git_credentials":
    "/configuration/settings.html#github.use_git_credentials",
  "/dev-tools/backends/github.html#platform-specific-asset-patterns":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/github.html#platform-specific-checksums":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/github.html#supported-github-syntax":
    "/dev-tools/backends/github.html#usage",
  "/dev-tools/backends/gitlab.html#asset-autodetection":
    "/dev-tools/backends/github.html#asset-autodetection",
  "/dev-tools/backends/gitlab.html#asset-pattern":
    "/dev-tools/backends/github.html#asset-pattern",
  "/dev-tools/backends/gitlab.html#bin": "/dev-tools/backends/github.html#bin",
  "/dev-tools/backends/gitlab.html#bin-path":
    "/dev-tools/backends/github.html#bin-path",
  "/dev-tools/backends/gitlab.html#checksum":
    "/dev-tools/backends/github.html#checksum",
  "/dev-tools/backends/gitlab.html#credential-command":
    "/dev-tools/github-tokens.html#credential-command",
  "/dev-tools/backends/gitlab.html#debugging-token-resolution":
    "/dev-tools/github-tokens.html#check-which-token-mise-uses",
  "/dev-tools/backends/gitlab.html#environment-variables":
    "/dev-tools/github-tokens.html#environment-variables",
  "/dev-tools/backends/gitlab.html#filter-bins":
    "/dev-tools/backends/github.html#filter-bins",
  "/dev-tools/backends/gitlab.html#git-credential-fill-fallback":
    "/dev-tools/github-tokens.html#git-credential-helpers",
  "/dev-tools/backends/gitlab.html#gitlab.credential_command":
    "/configuration/settings.html#gitlab.credential_command",
  "/dev-tools/backends/gitlab.html#gitlab.glab_cli_tokens":
    "/configuration/settings.html#gitlab.glab_cli_tokens",
  "/dev-tools/backends/gitlab.html#gitlab.use_git_credentials":
    "/configuration/settings.html#gitlab.use_git_credentials",
  "/dev-tools/backends/gitlab.html#glab-cli-integration":
    "/dev-tools/github-tokens.html#cli-logins",
  "/dev-tools/backends/gitlab.html#matching":
    "/dev-tools/backends/github.html#matching",
  "/dev-tools/backends/gitlab.html#matching-regex":
    "/dev-tools/backends/github.html#matching-regex",
  "/dev-tools/backends/gitlab.html#no-app":
    "/dev-tools/backends/github.html#no-app",
  "/dev-tools/backends/gitlab.html#platform-specific-asset-patterns":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/gitlab.html#platform-specific-checksums":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/gitlab.html#platform-specific-size":
    "/dev-tools/backends/github.html#per-platform-options",
  "/dev-tools/backends/gitlab.html#private-gitlab-repositories":
    "/dev-tools/backends/gitlab.html#authentication",
  "/dev-tools/backends/gitlab.html#rename-exe":
    "/dev-tools/backends/github.html#rename-exe",
  "/dev-tools/backends/gitlab.html#self-hosted-gitlab":
    "/dev-tools/backends/gitlab.html#self-managed-gitlab",
  "/dev-tools/backends/gitlab.html#size":
    "/dev-tools/backends/github.html#size",
  "/dev-tools/backends/gitlab.html#strip-components":
    "/dev-tools/backends/github.html#strip-components",
  "/dev-tools/backends/gitlab.html#supported-gitlab-syntax":
    "/dev-tools/backends/gitlab.html#usage",
  "/dev-tools/backends/gitlab.html#token-file-gitlab-tokens-toml":
    "/dev-tools/github-tokens.html#token-files",
  "/dev-tools/backends/gitlab.html#token-priority":
    "/dev-tools/github-tokens.html#token-priority",
  "/dev-tools/backends/gitlab.html#version-prefix":
    "/dev-tools/backends/github.html#version-prefix",
  "/dev-tools/backends/http.html#cache-key-generation":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#cache-location":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#cache-management":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#cache-metadata":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#caching-behavior":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#installation-and-cleanup":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#platform-specific-checksums":
    "/dev-tools/backends/http.html#platform-specific-urls",
  "/dev-tools/backends/http.html#platform-specific-format":
    "/dev-tools/backends/http.html#platform-specific-urls",
  "/dev-tools/backends/http.html#platform-specific-size":
    "/dev-tools/backends/http.html#platform-specific-urls",
  "/dev-tools/backends/http.html#supported-http-syntax":
    "/dev-tools/backends/http.html#usage",
  "/dev-tools/backends/http.html#symlinked-installations":
    "/dev-tools/backends/http.html#shared-extraction",
  "/dev-tools/backends/http.html#tool-options":
    "/dev-tools/backends/http.html#template-variables",
  "/dev-tools/backends/npm.html#aube-cli":
    "/dev-tools/backends/npm.html#build-scripts-and-supply-chain-checks",
  "/dev-tools/backends/npm.html#aube-default":
    "/dev-tools/backends/npm.html#build-scripts-and-supply-chain-checks",
  "/dev-tools/backends/npm.html#bun":
    "/dev-tools/backends/npm.html#build-scripts-and-supply-chain-checks",
  "/dev-tools/backends/npm.html#npm":
    "/dev-tools/backends/npm.html#build-scripts-and-supply-chain-checks",
  "/dev-tools/backends/npm.html#pnpm":
    "/dev-tools/backends/npm.html#build-scripts-and-supply-chain-checks",
  "/dev-tools/backends/s3.html#bin": "/dev-tools/backends/http.html#bin",
  "/dev-tools/backends/s3.html#bin-path":
    "/dev-tools/backends/http.html#bin-path",
  "/dev-tools/backends/s3.html#checksum":
    "/dev-tools/backends/http.html#checksum",
  "/dev-tools/backends/s3.html#comparison-with-http-backend":
    "/dev-tools/backends/s3.html#s3-backend",
  "/dev-tools/backends/s3.html#format": "/dev-tools/backends/http.html#format",
  "/dev-tools/backends/s3.html#platform-specific-urls":
    "/dev-tools/backends/http.html#platform-specific-urls",
  "/dev-tools/backends/s3.html#rename-exe":
    "/dev-tools/backends/http.html#rename-exe",
  "/dev-tools/backends/s3.html#size": "/dev-tools/backends/http.html#size",
  "/dev-tools/backends/s3.html#strip-components":
    "/dev-tools/backends/http.html#strip-components",
  "/dev-tools/backends/s3.html#version-expr":
    "/dev-tools/backends/http.html#version-expr",
  "/dev-tools/backends/s3.html#version-json-path":
    "/dev-tools/backends/http.html#version-json-path",
  "/dev-tools/backends/spm.html#spm-artifactbundle-only":
    "/dev-tools/backends/spm.html#spm.artifactbundle_only",
  "/dev-tools/backends/ubi.html#api-url":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#bin-path":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#exe":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#extract-all":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#matching":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#matching-regex":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#provider":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#rename-exe":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#self-hosted-github-gitlab":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#supported-ubi-syntax":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#tag-regex":
    "/dev-tools/backends/ubi.html#tool-options",
  "/dev-tools/backends/ubi.html#troubleshooting-ubi":
    "/dev-tools/backends/ubi.html#troubleshooting",
  "/dev-tools/backends/ubi.html#ubi-backend":
    "/dev-tools/backends/ubi.html#migrating-off-the-ubi-backend",
  "/dev-tools/backends/ubi.html#ubi-can-t-find-the-binary-in-the-tarball":
    "/dev-tools/backends/ubi.html#troubleshooting",
  "/dev-tools/backends/ubi.html#ubi-picks-the-wrong-tarball":
    "/dev-tools/backends/ubi.html#troubleshooting",
  "/dev-tools/backends/ubi.html#ubi-resolver-can-t-find-os-arch":
    "/dev-tools/backends/ubi.html#troubleshooting",
  "/dev-tools/backends/ubi.html#ubi-uses-weird-versions":
    "/dev-tools/backends/ubi.html#troubleshooting",
  "/dev-tools/backends/vfox.html#example-plugin-usage": "/plugins.html",
  "/dev-tools/backends/vfox.html#install-from-zip-file": "/plugins.html",
  "/dev-tools/backends/vfox.html#why-vfox":
    "/dev-tools/backends/vfox.html#vfox-backend",
  "/dev-tools/comparison-to-asdf.html#asdf-in-go-0-16":
    "/dev-tools/comparison-to-asdf.html#command-compatibility",
  "/dev-tools/comparison-to-asdf.html#supply-chain-security":
    "/dev-tools/comparison-to-asdf.html#security",
  "/dev-tools/comparison-to-asdf.html#ux":
    "/dev-tools/#add-a-tool-to-a-project",
  "/dev-tools/deps.html#cli-usage": "/cli/deps.html",
  "/dev-tools/deps.html#dependencies":
    "/dev-tools/deps.html#order-and-parallelism",
  "/dev-tools/deps.html#parallel-execution":
    "/dev-tools/deps.html#order-and-parallelism",
  "/dev-tools/github-tokens.html#debugging-token-resolution":
    "/dev-tools/github-tokens.html#check-which-token-mise-uses",
  "/dev-tools/github-tokens.html#gh-cli-integration":
    "/dev-tools/github-tokens.html#cli-logins",
  "/dev-tools/github-tokens.html#setting-a-token-via-environment-variable":
    "/dev-tools/github-tokens.html#set-a-token",
  "/dev-tools/github-tokens.html#start-with-token-diagnostics":
    "/dev-tools/github-tokens.html#check-which-token-mise-uses",
  "/dev-tools/github-tokens.html#token-file-github-tokens-toml":
    "/dev-tools/github-tokens.html#token-files",
  "/dev-tools/mise-lock.html#best-practices":
    "/dev-tools/mise-lock.html#overview",
  "/dev-tools/mise-lock.html#complete-lockfile-generation":
    "/dev-tools/mise-lock-reference.html#complete-lockfile-generation",
  "/dev-tools/mise-lock.html#daily-usage": "/dev-tools/mise-lock.html#workflow",
  "/dev-tools/mise-lock.html#disabling-for-specific-projects":
    "/dev-tools/mise-lock.html#enabling-lockfiles",
  "/dev-tools/mise-lock.html#file-format":
    "/dev-tools/mise-lock-reference.html#file-format",
  "/dev-tools/mise-lock.html#from-asdf": "/dev-tools/comparison-to-asdf.html",
  "/dev-tools/mise-lock.html#from-package-json-engines": "/lang/node.html",
  "/dev-tools/mise-lock.html#how-it-works":
    "/dev-tools/mise-lock-reference.html#runtime-resolution",
  "/dev-tools/mise-lock.html#initial-setup":
    "/dev-tools/mise-lock.html#workflow",
  "/dev-tools/mise-lock.html#inspecting-and-editing-sidecars":
    "/dev-tools/mise-lock-reference.html#inspecting-and-editing-sidecars",
  "/dev-tools/mise-lock.html#listing-sidecars":
    "/dev-tools/mise-lock-reference.html#listing-sidecars",
  "/dev-tools/mise-lock.html#migration-from-other-tools":
    "/dev-tools/comparison-to-asdf.html",
  "/dev-tools/mise-lock.html#native-dependency-sidecars":
    "/dev-tools/mise-lock-reference.html#native-dependency-sidecars",
  "/dev-tools/mise-lock.html#npm-tools":
    "/dev-tools/mise-lock.html#dependency-graphs",
  "/dev-tools/mise-lock.html#platform-information":
    "/dev-tools/mise-lock-reference.html#platform-information",
  "/dev-tools/mise-lock.html#platform-keys":
    "/dev-tools/mise-lock-reference.html#platform-keys",
  "/dev-tools/mise-lock.html#python-dependency-graphs":
    "/dev-tools/mise-lock.html#dependency-graphs",
  "/dev-tools/mise-lock.html#ruby-precompiled-build-revision-releases":
    "/dev-tools/mise-lock-reference.html#ruby-precompiled-build-revision-releases",
  "/dev-tools/mise-lock.html#runtime-resolution":
    "/dev-tools/mise-lock-reference.html#runtime-resolution",
  "/dev-tools/mise-lock.html#see-also": "/dev-tools/mise-lock.html#overview",
  "/dev-tools/mise-lock.html#shared-lockfiles":
    "/dev-tools/mise-lock-reference.html#shared-lockfiles",
  "/dev-tools/mise-lock.html#sidecar-locations":
    "/dev-tools/mise-lock-reference.html#sidecar-locations",
  "/dev-tools/mise-lock.html#team-workflow":
    "/dev-tools/mise-lock.html#overview",
  "/dev-tools/mise-lock.html#tool-entry-fields":
    "/dev-tools/mise-lock-reference.html#tool-entry-fields",
  "/dev-tools/mise-lock.html#updating-versions":
    "/dev-tools/mise-lock.html#workflow",
  "/dev-tools/mise-lock.html#version-control":
    "/dev-tools/mise-lock.html#overview",
  "/dev-tools/mise-oci.html#commands-at-a-glance":
    "/dev-tools/mise-oci.html#quick-start",
  "/dev-tools/mise-oci.html#cross-platform-builds":
    "/dev-tools/mise-oci.html#requirements",
  "/dev-tools/mise-oci.html#known-limitations-v1":
    "/dev-tools/mise-oci.html#requirements",
  "/dev-tools/mise-oci.html#mise-oci-build":
    "/dev-tools/mise-oci.html#what-goes-into-the-image",
  "/dev-tools/mise-oci.html#see-also": "/cli/oci/build.html",
  "/dev-tools/mise-oci.html#settings": "/dev-tools/mise-oci.html#precedence",
  "/dev-tools/packslip-resources.html#follow-the-active-version":
    "/dev-tools/packslip-resources.html#manual-setup-without-shell-activation",
  "/dev-tools/packslip-resources.html#generation-and-caching":
    "/dev-tools/packslip-resources.html#resource-selection-and-command-execution",
  "/dev-tools/packslip-resources.html#source-selection":
    "/dev-tools/packslip-resources.html#resource-selection-and-command-execution",
  "/dev-tools/packslip-verification.html#inspect-the-tool-and-accepted-signers":
    "/dev-tools/backends/packslip.html#troubleshooting",
  "/dev-tools/packslip-verification.html#interpreting-policy-failures":
    "/dev-tools/backends/packslip.html#troubleshooting",
  "/dev-tools/tool-stubs.html#basic-generation":
    "/dev-tools/tool-stubs.html#generating-tool-stubs-http",
  "/dev-tools/tool-stubs.html#basic-node-js-stub":
    "/dev-tools/tool-stubs.html#tool-non-http-stubs",
  "/dev-tools/tool-stubs.html#caching": "/dev-tools/tool-stubs.html#usage",
  "/dev-tools/tool-stubs.html#direct-execution":
    "/dev-tools/tool-stubs.html#usage",
  "/dev-tools/tool-stubs.html#examples":
    "/dev-tools/tool-stubs.html#tool-non-http-stubs",
  "/dev-tools/tool-stubs.html#generated-stub-example":
    "/dev-tools/tool-stubs.html#generating-tool-stubs-http",
  "/dev-tools/tool-stubs.html#generation-options":
    "/dev-tools/tool-stubs.html#generating-tool-stubs-http",
  "/dev-tools/tool-stubs.html#github-release-backend":
    "/dev-tools/tool-stubs.html#configuration-fields",
  "/dev-tools/tool-stubs.html#http-backend-with-platform-support":
    "/dev-tools/tool-stubs.html#http-stubs",
  "/dev-tools/tool-stubs.html#optional-fields":
    "/dev-tools/tool-stubs.html#configuration-fields",
  "/dev-tools/tool-stubs.html#overview": "/dev-tools/tool-stubs.html",
  "/dev-tools/tool-stubs.html#platform-specific-generation":
    "/dev-tools/tool-stubs.html#generating-tool-stubs-http",
  "/dev-tools/tool-stubs.html#python-with-custom-binary-name":
    "/dev-tools/tool-stubs.html#tool-non-http-stubs",
  "/dev-tools/tool-stubs.html#supported-archive-formats":
    "/dev-tools/tool-stubs.html#generating-tool-stubs-http",
  "/dev-tools/tool-stubs.html#via-mise-command":
    "/dev-tools/tool-stubs.html#usage",
  "/dotfiles.html#absent": "/dotfiles/managed.html#absent",
  "/dotfiles.html#adding-files-to-a-group":
    "/dotfiles/groups.html#adding-files-to-a-group",
  "/dotfiles.html#capturing-changes":
    "/dotfiles/managed.html#capturing-changes",
  "/dotfiles.html#command-names": "/dotfiles.html",
  "/dotfiles.html#conflicts": "/dotfiles/managed.html#conflicts",
  "/dotfiles.html#deselecting-and-removing-groups":
    "/dotfiles/groups.html#deselecting-and-removing-groups",
  "/dotfiles.html#dot-prefix": "/dotfiles/managed.html#dot-prefix",
  "/dotfiles.html#edit-entries": "/dotfiles/edits.html",
  "/dotfiles.html#excluding-files": "/dotfiles/managed.html#excluding-files",
  "/dotfiles.html#files-directories-and-symlinks":
    "/dotfiles/history.html#explicit-tracking-and-exclusions",
  "/dotfiles.html#git-tracked-directories":
    "/dotfiles/managed.html#git-tracked-directories",
  "/dotfiles.html#group-entries": "/dotfiles/groups.html#group-entries",
  "/dotfiles.html#groups": "/dotfiles/groups.html",
  "/dotfiles.html#inline-content": "/dotfiles/managed.html#inline-content",
  "/dotfiles.html#json-output": "/dotfiles/reference.html#json-output",
  "/dotfiles.html#machine-variants": "/dotfiles/history.html#machine-variants",
  "/dotfiles.html#matching-multiple-source-files":
    "/dotfiles/managed.html#matching-multiple-source-files",
  "/dotfiles.html#merge": "/dotfiles/edits.html#merge",
  "/dotfiles.html#modes": "/dotfiles/managed.html#modes",
  "/dotfiles.html#ownership": "/dotfiles/history.html#ownership",
  "/dotfiles.html#permissions": "/dotfiles/managed.html#permissions",
  "/dotfiles.html#platform-specific-destinations":
    "/dotfiles/managed.html#platform-specific-destinations",
  "/dotfiles.html#policies": "/dotfiles/history.html#save-on-request",
  "/dotfiles.html#preview-before-tracking":
    "/dotfiles/history.html#preview-before-tracking",
  "/dotfiles.html#relative": "/dotfiles/managed.html#relative",
  "/dotfiles.html#remove-empty": "/dotfiles/managed.html#remove-empty",
  "/dotfiles.html#root-owned-files": "/dotfiles/managed.html#root-owned-files",
  "/dotfiles.html#save-edits-automatically":
    "/dotfiles/history.html#automatic-saves",
  "/dotfiles.html#select-files-within-a-tracked-directory":
    "/dotfiles/history.html#choose-which-files-a-directory-saves",
  "/dotfiles.html#selecting-groups": "/dotfiles/groups.html#selecting-groups",
  "/dotfiles.html#self-managing-mise-config":
    "/dotfiles/managed.html#self-managing-mise-config",
  "/dotfiles.html#semantics": "/dotfiles/reference.html#how-entries-combine",
  "/dotfiles.html#share-with-another-machine": "/dotfiles/sync.html",
  "/dotfiles.html#start-with-one-managed-file":
    "/dotfiles.html#deploy-from-a-repository",
  "/dotfiles.html#stop-tracking-a-file":
    "/dotfiles/history.html#stop-tracking-a-file",
  "/dotfiles.html#templates": "/dotfiles/managed.html#templates",
  "/dotfiles.html#tracking-options":
    "/dotfiles/history.html#explicit-tracking-and-exclusions",
  "/dotfiles.html#try-restoring-a-change":
    "/dotfiles/history.html#rolling-back",
  "/dotfiles.html#unapplying": "/dotfiles/managed.html#unapplying",
  "/dotfiles.html#variants": "/dotfiles/history.html#variants",
  "/dotfiles.html#whole-file-entries":
    "/dotfiles/managed.html#whole-file-entries",
  "/dotfiles.html#windows": "/dotfiles/managed.html#windows",
  "/env-plugin-development.html#_4-use-built-in-caching-for-expensive-operations":
    "/env-plugin-development.html#caching",
  "/env-plugin-development.html#available-lua-modules":
    "/env-plugin-development.html#hooks",
  "/env-plugin-development.html#best-practices":
    "/env-plugin-development.html#common-mistakes",
  "/env-plugin-development.html#common-issues":
    "/env-plugin-development.html#common-mistakes",
  "/env-plugin-development.html#complete-example-secret-manager-plugin":
    "/env-plugin-development.html#complete-example",
  "/env-plugin-development.html#configuration-in-mise-toml":
    "/env-plugin-development.html#options",
  "/env-plugin-development.html#examples":
    "/env-plugin-development.html#complete-example",
  "/env-plugin-development.html#hooks-mise-env-lua":
    "/env-plugin-development.html#miseenv-hook",
  "/env-plugin-development.html#hooks-mise-path-lua":
    "/env-plugin-development.html#misepath-hook",
  "/env-plugin-development.html#local-testing":
    "/env-plugin-development.html#testing-your-plugin",
  "/env-plugin-development.html#metadata-lua":
    "/env-plugin-development.html#quick-start",
  "/env-plugin-development.html#plugin-structure":
    "/env-plugin-development.html#quick-start",
  "/env-plugin-development.html#publishing-your-plugin":
    "/plugin-publishing.html",
  "/env-plugin-development.html#related-documentation": "/plugins.html",
  "/environments/#basic-usage": "/environments/#plugin-directives",
  "/environments/#ci-masking": "/environments/secrets/#ci-masking",
  "/environments/#config-root": "/configuration.html#config-root",
  "/environments/#creating-environment-plugins": "/env-plugin-development.html",
  "/environments/#example-dynamic-environment-plugin":
    "/environments/#plugin-directives",
  "/environments/#example-secret-management-plugin":
    "/environments/#plugin-directives",
  "/environments/#how-it-works": "/environments/#plugin-directives",
  "/environments/#multiple-env-directives": "/environments/#env-directives",
  "/environments/#plugin-provided-env-directives":
    "/environments/#plugin-directives",
  "/environments/#redactions": "/environments/secrets/#redaction",
  "/environments/#required-variable-behavior":
    "/environments/#required-variables",
  "/environments/#use-cases": "/environments/#required-variables",
  "/environments/#using-env-vars-in-other-env-vars":
    "/environments/#reference-other-values",
  "/environments/#validation-behavior": "/environments/#required-variables",
  "/environments/#viewing-redacted-environment-variables":
    "/environments/secrets/#export-redacted-values",
  "/environments/secrets/age.html#cli-flags":
    "/environments/secrets/age.html#share-with-a-team",
  "/environments/secrets/age.html#notes":
    "/environments/secrets/age.html#quick-start",
  "/environments/secrets/sops.html#ci-masking-github-actions":
    "/environments/secrets/#ci-masking",
  "/environments/secrets/sops.html#redaction":
    "/environments/secrets/#redaction",
  "/errors.html#checksum-mismatch-for-file-file":
    "/errors.html#checksum-mismatch",
  "/errors.html#command-exited-with-non-zero-status-exit-code-n-command-failed-exit-code-n":
    "/errors.html#command-failed",
  "/errors.html#config-file-tool-version-error-failed-to-resolve-version":
    "/errors.html#failed-to-resolve-version",
  "/errors.html#config-files-in-dir-are-not-trusted-trust-them-with-mise-trust":
    "/errors.html#untrusted-config",
  "/errors.html#errors": "/errors.html#error-messages",
  "/errors.html#failed-to-install-tool-version-underlying-error":
    "/errors.html#failed-to-install",
  "/errors.html#http-status-client-error-401-unauthorized":
    "/errors.html#http-401",
  "/errors.html#http-status-client-error-403-forbidden-github-rate-limit-exceeded":
    "/errors.html#http-403",
  "/errors.html#mise-version-x-is-required-but-you-are-using-y":
    "/errors.html#min-version",
  "/errors.html#no-tasks-name-found": "/errors.html#task-not-found",
  "/errors.html#tool-not-found-in-mise-tool-registry":
    "/errors.html#not-in-registry",
  "/errors.html#tool-version-not-installed": "/errors.html#not-installed",
  "/faq.html#faqs": "/faq.html#faq",
  "/faq.html#vscode-for-windows-extension-with-error-spawn-einval":
    "/troubleshooting.html#vscode-for-windows-extension-with-error-spawn-einval",
  "/getting-started.html#autocompletion": "/shell-setup.html#autocompletion",
  "/getting-started.html#project-configuration-or-global-defaults":
    "/getting-started.html#confirm-what-is-active",
  "/getting-started.html#shell-feature-compatibility":
    "/shell-setup.html#shell-feature-compatibility",
  "/glossary.html#backends": "/glossary.html#backend",
  "/glossary.html#core-concepts": "/glossary.html#tools-and-versions",
  "/glossary.html#directories-environment": "/glossary.html#directories",
  "/glossary.html#environment-variables":
    "/glossary.html#shell-and-environment",
  "/glossary.html#other-terms": "/glossary.html",
  "/glossary.html#shell-integration": "/glossary.html#shell-and-environment",
  "/hooks.html#cd-hook": "/hooks.html#directory-hooks",
  "/hooks.html#enter-hook": "/hooks.html#directory-hooks",
  "/hooks.html#leave-hook": "/hooks.html#directory-hooks",
  "/hooks.html#multiple-hooks-syntax": "/hooks.html#define-a-hook",
  "/hooks.html#tool-level-postinstall": "/dev-tools/#tool-postinstall-commands",
  "/ide-integration.html#intellij-plugin": "/ide-integration.html#ide-plugins",
  "/ide-integration.html#shims": "/ide-integration.html#emacs",
  "/ide-integration.html#use-with-package-mise-el":
    "/ide-integration.html#emacs",
  "/ide-integration.html#vscode-plugin": "/ide-integration.html#ide-plugins",
  "/installing-mise.html#autocompletion": "/shell-setup.html#autocompletion",
  "/installing-mise.html#bash": "/shell-setup.html#bash",
  "/installing-mise.html#elvish": "/shell-setup.html#elvish",
  "/installing-mise.html#fedora-41-centos-stream-9-rhel-10":
    "/installing-mise.html#dnf-fedora",
  "/installing-mise.html#fish": "/shell-setup.html#fish",
  "/installing-mise.html#nushell": "/shell-setup.html#nushell",
  "/installing-mise.html#pin-the-bootstrapper-and-let-mise-float":
    "/mise-cookbook/docker.html#bootstrap-with-packslip",
  "/installing-mise.html#powershell": "/shell-setup.html#powershell",
  "/installing-mise.html#rhel-9-almalinux-9-rocky-9":
    "/installing-mise.html#dnf-el9",
  "/installing-mise.html#something-else": "/shell-setup.html#other-shells",
  "/installing-mise.html#troubleshooting": "/troubleshooting.html",
  "/installing-mise.html#xonsh": "/shell-setup.html#xonsh",
  "/installing-mise.html#yum-rhel-8-centos-stream-8":
    "/installing-mise.html#yum",
  "/installing-mise.html#zsh": "/shell-setup.html#zsh",
  "/lang/bun.html#install-env": "/lang/bun.html#tool-options",
  "/lang/bun.html#usage": "/lang/bun.html#quick-start",
  "/lang/deno.html#install-env": "/lang/deno.html#tool-options",
  "/lang/deno.html#usage": "/lang/deno.html#quick-start",
  "/lang/dotnet.html#example-mix-sdk-and-runtime":
    "/lang/dotnet.html#runtime-only-installs",
  "/lang/dotnet.html#global-json-support": "/lang/dotnet.html#version-files",
  "/lang/dotnet.html#usage": "/lang/dotnet.html#quick-start",
  "/lang/dotnet.html#valid-runtime-values":
    "/lang/dotnet.html#runtime-only-installs",
  "/lang/elixir.html#install-env": "/lang/elixir.html#tool-options",
  "/lang/elixir.html#usage": "/lang/elixir.html#quick-start",
  "/lang/erlang.html#install-env": "/lang/erlang.html#source-builds-with-kerl",
  "/lang/erlang.html#usage": "/lang/erlang.html#quick-start",
  "/lang/go.html#install-env": "/lang/go.html#tool-options",
  "/lang/go.html#usage": "/lang/go.html#quick-start",
  "/lang/java.html#install-env": "/lang/java.html#tool-options",
  "/lang/java.html#java-version-and-sdkmanrc-files-support":
    "/lang/java.html#version-files",
  "/lang/java.html#usage": "/lang/java.html#quick-start",
  "/lang/node.html#install-env": "/lang/node.html#tool-options",
  "/lang/node.html#nodejs-node-alias":
    "/faq.html#what-is-the-difference-between-nodejs-and-node-or-golang-and-go",
  "/lang/node.html#run-projects-with-aube":
    "/mise-cookbook/nodejs.html#run-projects-with-aube",
  "/lang/node.html#usage": "/lang/node.html#quick-start",
  "/lang/python.html#install-env": "/lang/python.html#generic-options",
  "/lang/python.html#installing-free-threaded-python":
    "/lang/python.html#free-threaded-python",
  "/lang/python.html#mise-uv": "/lang/python.html#uv-projects",
  "/lang/python.html#python-build":
    "/lang/python.html#precompiled-python-binaries",
  "/lang/python.html#python-uv-venv-auto-setting":
    "/lang/python.html#uv-projects",
  "/lang/python.html#python-venv-configuration":
    "/lang/python.html#python-venv",
  "/lang/python.html#python-version-support": "/lang/python.html#version-files",
  "/lang/python.html#usage": "/lang/python.html#quick-start",
  "/lang/ruby.html#install-env": "/lang/ruby.html#tool-options",
  "/lang/ruby.html#ruby-version-and-gemfile-support":
    "/lang/ruby.html#version-files",
  "/lang/ruby.html#usage": "/lang/ruby.html#quick-start",
  "/lang/rust.html#existing-rustup-projects": "/lang/rust.html#version-files",
  "/lang/rust.html#usage": "/lang/rust.html#quick-start",
  "/lang/swift.html#usage": "/lang/swift.html#quick-start",
  "/lang/zig.html#install-env": "/lang/zig.html#tool-options",
  "/lang/zig.html#master-nightly-channel":
    "/lang/zig.html#nightly-builds-master",
  "/lang/zig.html#usage": "/lang/zig.html#quick-start",
  "/mcp.html#install-tool": "/mcp.html#available-tools",
  "/mcp.html#list-commands": "/mcp.html#available-tools",
  "/mcp.html#run-task": "/mcp.html#available-tools",
  "/mcp.html#technical-details": "/cli/mcp.html",
  "/mcp.html#usage": "/mcp.html#integration-with-ai-assistants",
  "/mise-cookbook/nodejs.html#getting-started-with-node-js":
    "/lang/node.html#quick-start",
  "/paranoid.html#see-also": "/paranoid.html",
  "/plugin-lua-modules.html#accessing-via-vfox-namespace":
    "/plugin-lua-modules.html#loading-your-own-code",
  "/plugin-lua-modules.html#available-modules":
    "/plugin-lua-modules.html#module-index",
  "/plugin-lua-modules.html#basic-command-execution":
    "/plugin-lua-modules.html#command-module",
  "/plugin-lua-modules.html#best-practices": "/plugin-lua-modules.html#errors",
  "/plugin-lua-modules.html#command-execution-with-options":
    "/plugin-lua-modules.html#command-module",
  "/plugin-lua-modules.html#core-modules":
    "/plugin-lua-modules.html#module-index",
  "/plugin-lua-modules.html#css-selectors":
    "/plugin-lua-modules.html#html-module",
  "/plugin-lua-modules.html#error-handling": "/plugin-lua-modules.html#errors",
  "/plugin-lua-modules.html#error-handling-lua":
    "/plugin-lua-modules.html#json-module",
  "/plugin-lua-modules.html#file-download-with-progress":
    "/plugin-lua-modules.html#http-module",
  "/plugin-lua-modules.html#file-downloads":
    "/plugin-lua-modules.html#http-module",
  "/plugin-lua-modules.html#interactive-children-with-cmd-stream":
    "/plugin-lua-modules.html#hooks-and-stdin",
  "/plugin-lua-modules.html#next-steps":
    "/plugin-lua-modules.html#related-pages",
  "/plugin-lua-modules.html#non-raising-variants-try":
    "/plugin-lua-modules.html#http-module",
  "/plugin-lua-modules.html#path-operations":
    "/plugin-lua-modules.html#environment-module",
  "/plugin-lua-modules.html#platform-detection":
    "/plugin-lua-modules.html#runtime",
  "/plugin-lua-modules.html#platform-specific-commands":
    "/plugin-lua-modules.html#runtime",
  "/plugin-lua-modules.html#plugin-name-prefix":
    "/plugin-lua-modules.html#log-module",
  "/plugin-lua-modules.html#practical-examples":
    "/plugin-lua-modules.html#module-index",
  "/plugin-lua-modules.html#real-world-example-available-hook":
    "/plugin-lua-modules.html#semver-module",
  "/plugin-lua-modules.html#real-world-example-plugin-installation":
    "/plugin-lua-modules.html#http-module",
  "/plugin-lua-modules.html#real-world-example-scraping-releases":
    "/plugin-lua-modules.html#collect-links-from-a-download-page",
  "/plugin-lua-modules.html#response-object":
    "/plugin-lua-modules.html#http-module",
  "/plugin-lua-modules.html#using-compare-in-custom-sort":
    "/plugin-lua-modules.html#semver-module",
  "/plugin-lua-modules.html#variadic-arguments":
    "/plugin-lua-modules.html#log-module",
  "/plugin-lua-modules.html#version-string-utilities":
    "/plugin-lua-modules.html#strings-module",
  "/plugin-publishing.html#best-practices":
    "/plugin-publishing.html#publishing-checklist",
  "/plugin-publishing.html#examples":
    "/plugin-publishing.html#testing-before-publication",
  "/plugin-publishing.html#publishing-process":
    "/plugin-publishing.html#tag-a-release",
  "/plugin-publishing.html#repository-setup":
    "/plugin-lua-modules.html#hook-files",
  "/plugin-publishing.html#versioning-strategy":
    "/plugin-publishing.html#tag-a-release",
  "/plugins.html#asdf-legacy-plugins": "/plugins.html#hook-migration",
  "/plugins.html#general-plugin-usage": "/plugins.html#installing-plugins",
  "/plugins.html#plugin-authors": "/plugin-publishing.html",
  "/plugins.html#templates": "/plugins.html#from-a-private-repository",
  "/registry.html#backends-priority":
    "/dev-tools/backends/#how-backend-selection-works",
  "/registry.html#environment-variable-overrides":
    "/dev-tools/backends/#environment-variable-overrides",
  "/registry.html#version-specific-backends":
    "/dev-tools/backends/#version-specific-backends",
  "/sandboxing.html#build-with-network-isolation":
    "/sandboxing.html#quick-start",
  "/sandboxing.html#examples": "/sandboxing.html#quick-start",
  "/sandboxing.html#implicit-rules": "/sandboxing.html#always-readable",
  "/sandboxing.html#restrict-env-vars-to-a-namespace":
    "/sandboxing.html#quick-start",
  "/sandboxing.html#run-tool-with-minimal-permissions":
    "/sandboxing.html#quick-start",
  "/sandboxing.html#sandboxed-task-definition":
    "/sandboxing.html#task-sandboxing",
  "/security.html#trust-and-backend-support": "/security.html#what-still-works",
  "/shell-aliases.html#comparison-to-tool-aliases": "/shell-aliases.html",
  "/shell-aliases.html#configuration": "/shell-aliases.html",
  "/shell-aliases.html#dynamic-behavior": "/shell-aliases.html",
  "/shell-aliases.html#project-specific-shortcuts":
    "/shell-aliases.html#use-cases",
  "/shell-aliases.html#quick-navigation": "/shell-aliases.html#templates",
  "/shell-aliases.html#supported-shells":
    "/shell-setup.html#shell-feature-compatibility",
  "/shell-aliases.html#tool-wrappers": "/shell-aliases.html#use-cases",
  "/tasks/#environment-variables-passed-to-tasks":
    "/tasks/running-tasks.html#task-environment",
  "/tasks/#file-tasks": "/tasks/#write-a-task-as-a-script",
  "/tasks/#tasks-in-mise-toml-files": "/tasks/#group-tasks-with-dependencies",
  "/tasks/architecture.html#advanced-dependency-features":
    "/tasks/architecture.html#run-a-task-from-a-script",
  "/tasks/architecture.html#common-issues":
    "/tasks/architecture.html#inspect-and-debug",
  "/tasks/architecture.html#conditional-dependencies":
    "/tasks/architecture.html#run-a-task-from-a-script",
  "/tasks/architecture.html#cross-project-dependencies": "/tasks/monorepo.html",
  "/tasks/architecture.html#debugging-task-dependencies":
    "/tasks/architecture.html#inspect-and-debug",
  "/tasks/architecture.html#dependency-graph-resolution":
    "/tasks/architecture.html#how-mise-schedules-a-run",
  "/tasks/architecture.html#dependency-types":
    "/tasks/architecture.html#choose-a-dependency-type",
  "/tasks/architecture.html#depends-post-cleanup-tasks":
    "/tasks/architecture.html#depends-post",
  "/tasks/architecture.html#depends-prerequisites":
    "/tasks/architecture.html#depends",
  "/tasks/architecture.html#dynamic-dependencies":
    "/tasks/architecture.html#run-a-task-from-a-script",
  "/tasks/architecture.html#example-execution-flow":
    "/tasks/architecture.html#how-mise-schedules-a-run",
  "/tasks/architecture.html#execution-tracing":
    "/tasks/architecture.html#see-the-graph",
  "/tasks/architecture.html#incremental-execution":
    "/tasks/running-tasks.html#skip-tasks-that-are-up-to-date",
  "/tasks/architecture.html#job-control":
    "/tasks/running-tasks.html#control-a-run",
  "/tasks/architecture.html#parallel-execution-engine":
    "/tasks/architecture.html#how-mise-schedules-a-run",
  "/tasks/architecture.html#parallel-file-watching":
    "/tasks/running-tasks.html#rerun-tasks-when-files-change",
  "/tasks/architecture.html#performance-optimizations":
    "/tasks/running-tasks.html#skip-tasks-that-are-up-to-date",
  "/tasks/architecture.html#source-and-output-tracking":
    "/tasks/running-tasks.html#skip-tasks-that-are-up-to-date",
  "/tasks/architecture.html#task-dependency-system":
    "/tasks/architecture.html#how-mise-schedules-a-run",
  "/tasks/architecture.html#task-discovery-and-resolution":
    "/tasks/task-discovery.html#task-sources",
  "/tasks/architecture.html#task-resolution-across-directories":
    "/tasks/task-discovery.html#task-sources",
  "/tasks/architecture.html#task-resolution-process":
    "/tasks/architecture.html#how-mise-schedules-a-run",
  "/tasks/architecture.html#task-sources":
    "/tasks/task-discovery.html#task-sources",
  "/tasks/architecture.html#visualize-dependencies":
    "/tasks/architecture.html#see-the-graph",
  "/tasks/architecture.html#wait-for-soft-dependencies":
    "/tasks/architecture.html#wait-for",
  "/tasks/caching.html#remote-cache-and-sensitive-data":
    "/tasks/remote-cache.html",
  "/tasks/file-tasks.html#cwd": "/tasks/file-tasks.html#working-directory",
  "/tasks/file-tasks.html#editing-tasks":
    "/tasks/file-tasks.html#create-or-edit-a-file-task",
  "/tasks/file-tasks.html#environment-variable-backing":
    "/tasks/task-arguments.html#environment-variable-backing",
  "/tasks/file-tasks.html#example-file-task-with-arguments":
    "/tasks/file-tasks.html#arguments",
  "/tasks/file-tasks.html#example-of-a-nodejs-file-task-with-arguments":
    "/tasks/file-tasks.html#a-node-js-task-with-arguments",
  "/tasks/file-tasks.html#extending-a-task-template":
    "/tasks/file-tasks.html#extend-a-task-template",
  "/tasks/file-tasks.html#powershell-tasks-with-no-ps1-extension":
    "/tasks/file-tasks.html#powershell-scripts-without-ps1",
  "/tasks/file-tasks.html#running-tasks-directly":
    "/tasks/file-tasks.html#run-a-script-by-path",
  "/tasks/file-tasks.html#task-configuration":
    "/tasks/file-tasks.html#configure-with-mise-comments",
  "/tasks/file-tasks.html#writing-one-task-for-both-platforms":
    "/tasks/file-tasks.html#one-task-two-scripts",
  "/tasks/monorepo.html#_1-define-shared-tools-and-environment-at-root":
    "/tasks/monorepo.html#tool-environment-and-vars-layering",
  "/tasks/monorepo.html#_2-override-only-when-necessary":
    "/tasks/monorepo.html#tool-environment-and-vars-layering",
  "/tasks/monorepo.html#_3-use-descriptive-task-names":
    "/tasks/monorepo.html#wildcard-patterns",
  "/tasks/monorepo.html#_4-group-related-projects":
    "/tasks/monorepo.html#wildcard-patterns",
  "/tasks/monorepo.html#affected-tasks":
    "/tasks/workspace-graph.html#affected-tasks",
  "/tasks/monorepo.html#benefits": "/tasks/monorepo.html#set-up-a-monorepo",
  "/tasks/monorepo.html#best-practices":
    "/tasks/monorepo.html#tool-environment-and-vars-layering",
  "/tasks/monorepo.html#cargo-dependency-inference":
    "/tasks/workspace-graph.html#cargo-workspaces",
  "/tasks/monorepo.html#cargo-workspace-discovery":
    "/tasks/workspace-graph.html#cargo-workspaces",
  "/tasks/monorepo.html#combining-wildcards":
    "/tasks/monorepo.html#wildcard-patterns",
  "/tasks/monorepo.html#comparison-to-other-tools":
    "/tasks/monorepo.html#depend-on-tasks-in-other-projects",
  "/tasks/monorepo.html#config-roots":
    "/tasks/monorepo.html#explicit-config-roots",
  "/tasks/monorepo.html#configuration":
    "/tasks/monorepo.html#set-up-a-monorepo",
  "/tasks/monorepo.html#enabling-monorepo-mode":
    "/tasks/monorepo.html#set-up-a-monorepo",
  "/tasks/monorepo.html#example-structure":
    "/tasks/monorepo.html#set-up-a-monorepo",
  "/tasks/monorepo.html#go-workspace-discovery":
    "/tasks/workspace-graph.html#go-workspaces",
  "/tasks/monorepo.html#layering-example":
    "/tasks/monorepo.html#tool-environment-and-vars-layering",
  "/tasks/monorepo.html#layering-rules":
    "/tasks/monorepo.html#tool-environment-and-vars-layering",
  "/tasks/monorepo.html#listing-example": "/tasks/monorepo.html#listing-tasks",
  "/tasks/monorepo.html#node-dependency-inference":
    "/tasks/workspace-graph.html#node-js-workspaces",
  "/tasks/monorepo.html#node-package-scripts":
    "/tasks/workspace-graph.html#node-package-scripts",
  "/tasks/monorepo.html#node-workspace-discovery":
    "/tasks/workspace-graph.html#node-js-workspaces",
  "/tasks/monorepo.html#overview": "/tasks/monorepo.html#set-up-a-monorepo",
  "/tasks/monorepo.html#path-wildcards":
    "/tasks/monorepo.html#wildcard-patterns",
  "/tasks/monorepo.html#project-overrides":
    "/tasks/workspace-graph.html#project-overrides",
  "/tasks/monorepo.html#provider-task-suggestions":
    "/tasks/workspace-graph.html#provider-task-suggestions",
  "/tasks/monorepo.html#related": "/tasks/monorepo.html#next-steps",
  "/tasks/monorepo.html#root-task-defaults":
    "/tasks/workspace-graph.html#root-task-defaults",
  "/tasks/monorepo.html#task-definition-precedence":
    "/tasks/workspace-graph.html#task-definition-precedence",
  "/tasks/monorepo.html#task-name-wildcards":
    "/tasks/monorepo.html#wildcard-patterns",
  "/tasks/monorepo.html#upstream-task-dependencies":
    "/tasks/workspace-graph.html#upstream-task-dependencies",
  "/tasks/monorepo.html#uv-dependency-inference":
    "/tasks/workspace-graph.html#uv-workspaces",
  "/tasks/monorepo.html#uv-workspace-discovery":
    "/tasks/workspace-graph.html#uv-workspaces",
  "/tasks/monorepo.html#view-specific-project-tasks":
    "/tasks/monorepo.html#listing-tasks",
  "/tasks/monorepo.html#workspace-project-graph-experimental":
    "/tasks/workspace-graph.html",
  "/tasks/opentelemetry.html#example-local-development-with-jaeger":
    "/tasks/opentelemetry.html#quick-start",
  "/tasks/opentelemetry.html#notes":
    "/tasks/opentelemetry.html#behavior-and-limits",
  "/tasks/opentelemetry.html#privacy-and-trust-boundary":
    "/tasks/opentelemetry.html#privacy",
  "/tasks/opentelemetry.html#standard-otel-environment-variables":
    "/tasks/opentelemetry.html#standard-environment-variables",
  "/tasks/opentelemetry.html#what-you-see":
    "/tasks/opentelemetry.html#trace-structure",
  "/tasks/running-tasks.html#control-execution":
    "/tasks/running-tasks.html#control-a-run",
  "/tasks/running-tasks.html#examples": "/tasks/running-tasks.html#wildcards",
  "/tasks/running-tasks.html#execution-order":
    "/tasks/architecture.html#run-steps-in-order",
  "/tasks/running-tasks.html#parallelism-and-output":
    "/tasks/running-tasks.html#output",
  "/tasks/running-tasks.html#pass-arguments-and-select-tasks":
    "/tasks/running-tasks.html#pass-arguments",
  "/tasks/running-tasks.html#running-on-file-changes":
    "/tasks/running-tasks.html#skip-tasks-that-are-up-to-date",
  "/tasks/running-tasks.html#shell-execution":
    "/tasks/toml-tasks.html#simple-commands-run-without-a-shell",
  "/tasks/running-tasks.html#task-grouping":
    "/tasks/running-tasks.html#select-tasks-by-name",
  "/tasks/running-tasks.html#watching-files":
    "/tasks/running-tasks.html#rerun-tasks-when-files-change",
  "/tasks/task-arguments.html#advanced-features":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#args-and-flags-with-defaults":
    "/tasks/task-arguments.html#bash-variable-expansion",
  "/tasks/task-arguments.html#basic-syntax":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#boolean-flags":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#boolean-flags-without-defaults":
    "/tasks/task-arguments.html#bash-variable-expansion",
  "/tasks/task-arguments.html#choices-enum-values":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#combining-features-example":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#common-patterns":
    "/tasks/task-arguments.html#bash-variable-expansion",
  "/tasks/task-arguments.html#complete-usage-specification-reference":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#completion-complete":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#conditional-flags":
    "/tasks/task-arguments.html#bash-variable-expansion",
  "/tasks/task-arguments.html#count-flags":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#deprecated-method":
    "/tasks/task-arguments.html#tera-templates",
  "/tasks/task-arguments.html#double-dash-behavior":
    "/tasks/task-arguments.html#double-dash",
  "/tasks/task-arguments.html#flag-advanced-features":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#flag-with-defaults":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#flag-with-values":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#flags-flag":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#global-flags":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#guidelines-for-usage-variables":
    "/tasks/task-arguments.html#bash-variable-expansion",
  "/tasks/task-arguments.html#hide-arguments":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#long-help-text":
    "/tasks/task-arguments.html#long-help",
  "/tasks/task-arguments.html#migration-guide":
    "/tasks/task-arguments.html#tera-templates",
  "/tasks/task-arguments.html#mounting-generated-specs":
    "/tasks/task-arguments.html#mount-a-spec-from-another-cli",
  "/tasks/task-arguments.html#negation":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#positional-arguments-arg":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#quick-example":
    "/tasks/task-arguments.html#usage-field",
  "/tasks/task-arguments.html#recommended-methods":
    "/tasks/task-arguments.html#usage-field",
  "/tasks/task-arguments.html#required-arguments":
    "/tasks/task-arguments.html#bash-variable-expansion",
  "/tasks/task-arguments.html#see-also":
    "/tasks/task-arguments.html#usage-field",
  "/tasks/task-arguments.html#short-only-or-long-only":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#variadic-arguments":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#with-defaults":
    "/tasks/task-arguments.html#spec-quick-reference",
  "/tasks/task-arguments.html#with-descriptions":
    "/tasks/task-arguments.html#completions-with-descriptions",
  "/tasks/task-configuration.html#add-metadata-and-dependencies":
    "/tasks/task-discovery.html#add-metadata-and-dependencies",
  "/tasks/task-configuration.html#allow-env":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#allow-net":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#allow-read":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#allow-write":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#cache-correctness-and-deterministic-tasks":
    "/tasks/caching.html#cache-correctness-and-deterministic-tasks",
  "/tasks/task-configuration.html#configuring-file-tasks-from-toml":
    "/tasks/task-discovery.html#configuring-file-tasks-from-toml",
  "/tasks/task-configuration.html#deny-all":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#deny-env":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#deny-net":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#deny-read":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#deny-write":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#environment-variable-support-for-args-and-flags":
    "/tasks/task-arguments.html#environment-variable-backing",
  "/tasks/task-configuration.html#external-dependencies-and-lockfiles":
    "/tasks/caching.html#external-dependencies-and-lockfiles",
  "/tasks/task-configuration.html#file-task-config-precedence":
    "/tasks/task-discovery.html#file-task-config-precedence",
  "/tasks/task-configuration.html#included-toml-files":
    "/tasks/task-discovery.html#included-toml-files",
  "/tasks/task-configuration.html#layered-task-definitions":
    "/tasks/task-discovery.html#layered-task-definitions",
  "/tasks/task-configuration.html#pass-through-env":
    "/tasks/task-configuration.html#sandbox",
  "/tasks/task-configuration.html#per-run-cache-access":
    "/tasks/caching.html#per-run-cache-access",
  "/tasks/task-configuration.html#redactions":
    "/environments/secrets/#redaction",
  "/tasks/task-configuration.html#remote-cache-and-sensitive-data":
    "/tasks/remote-cache.html",
  "/tasks/task-configuration.html#remote-git-includes":
    "/tasks/task-discovery.html#remote-git-includes",
  "/tasks/task-configuration.html#remote-oci-includes":
    "/tasks/task-discovery.html#remote-oci-includes",
  "/tasks/task-configuration.html#replace-a-script-s-command":
    "/tasks/task-discovery.html#replace-a-script-s-command",
  "/tasks/task-configuration.html#reusable-and-global-inputs":
    "/tasks/task-configuration.html#task_config.input_groups",
  "/tasks/task-configuration.html#task": "/configuration/settings.html#task",
  "/tasks/task-configuration.html#task-config-cache":
    "/tasks/task-configuration.html#task_config.cache",
  "/tasks/task-configuration.html#task-config-cascade":
    "/tasks/task-configuration.html#task_config.cascade",
  "/tasks/task-configuration.html#task-config-dir":
    "/tasks/task-configuration.html#task_config.dir",
  "/tasks/task-configuration.html#task-config-global-env":
    "/tasks/task-configuration.html#task_config.global_env",
  "/tasks/task-configuration.html#task-config-global-inputs":
    "/tasks/task-configuration.html#task_config.global_inputs",
  "/tasks/task-configuration.html#task-config-global-pass-through-env":
    "/tasks/task-configuration.html#task_config.global_pass_through_env",
  "/tasks/task-configuration.html#task-config-input-groups":
    "/tasks/task-configuration.html#task_config.input_groups",
  "/tasks/task-configuration.html#task-config-rust-cache":
    "/tasks/task-configuration.html#task_config.rust_cache",
  "/tasks/task-configuration.html#task-configuration-settings":
    "/tasks/task-configuration.html#settings",
  "/tasks/task-configuration.html#task.auto_infer":
    "/configuration/settings.html#task.auto_infer",
  "/tasks/task-configuration.html#task.cache_dir":
    "/configuration/settings.html#task.cache_dir",
  "/tasks/task-configuration.html#task.cache_max_age":
    "/configuration/settings.html#task.cache_max_age",
  "/tasks/task-configuration.html#task.cache_max_size":
    "/configuration/settings.html#task.cache_max_size",
  "/tasks/task-configuration.html#task.cache.audit_report":
    "/configuration/settings.html#task.cache.audit_report",
  "/tasks/task-configuration.html#task.cache.remote_mode":
    "/configuration/settings.html#task.cache.remote_mode",
  "/tasks/task-configuration.html#task.cache.remote_namespace":
    "/configuration/settings.html#task.cache.remote_namespace",
  "/tasks/task-configuration.html#task.cache.remote_oidc_audience":
    "/configuration/settings.html#task.cache.remote_oidc_audience",
  "/tasks/task-configuration.html#task.cache.remote_token":
    "/configuration/settings.html#task.cache.remote_token",
  "/tasks/task-configuration.html#task.cache.remote_token_file":
    "/configuration/settings.html#task.cache.remote_token_file",
  "/tasks/task-configuration.html#task.cache.remote_url":
    "/configuration/settings.html#task.cache.remote_url",
  "/tasks/task-configuration.html#task.disable_paths":
    "/configuration/settings.html#task.disable_paths",
  "/tasks/task-configuration.html#task.disable_spec_from_run_scripts":
    "/configuration/settings.html#task.disable_spec_from_run_scripts",
  "/tasks/task-configuration.html#task.monorepo_depth":
    "/configuration/settings.html#task.monorepo_depth",
  "/tasks/task-configuration.html#task.monorepo_exclude_dirs":
    "/configuration/settings.html#task.monorepo_exclude_dirs",
  "/tasks/task-configuration.html#task.monorepo_respect_gitignore":
    "/configuration/settings.html#task.monorepo_respect_gitignore",
  "/tasks/task-configuration.html#task.output":
    "/configuration/settings.html#task.output",
  "/tasks/task-configuration.html#task.quiet":
    "/configuration/settings.html#task.quiet",
  "/tasks/task-configuration.html#task.remote_no_cache":
    "/configuration/settings.html#task.remote_no_cache",
  "/tasks/task-configuration.html#task.run_auto_install":
    "/configuration/settings.html#task.run_auto_install",
  "/tasks/task-configuration.html#task.show_full_cmd":
    "/configuration/settings.html#task.show_full_cmd",
  "/tasks/task-configuration.html#task.skip":
    "/configuration/settings.html#task.skip",
  "/tasks/task-configuration.html#task.skip_depends":
    "/configuration/settings.html#task.skip_depends",
  "/tasks/task-configuration.html#task.source_freshness_equal_mtime_is_fresh":
    "/configuration/settings.html#task.source_freshness_equal_mtime_is_fresh",
  "/tasks/task-configuration.html#task.source_freshness_hash_contents":
    "/configuration/settings.html#task.source_freshness_hash_contents",
  "/tasks/task-configuration.html#task.timeout":
    "/configuration/settings.html#task.timeout",
  "/tasks/task-configuration.html#task.timings":
    "/configuration/settings.html#task.timings",
  "/tasks/task-configuration.html#vars":
    "/tasks/task-configuration.html#task-vars",
  "/tasks/task-configuration.html#vars-options": "/configuration/vars.html",
  "/tasks/task-configuration.html#windows-script-pairs":
    "/tasks/task-discovery.html#windows-script-pairs",
  "/tasks/templates.html#template-naming":
    "/tasks/templates.html#defining-templates",
  "/tasks/toml-tasks.html#adding-a-description-and-alias":
    "/tasks/toml-tasks.html#description-and-alias",
  "/tasks/toml-tasks.html#adding-tasks":
    "/tasks/toml-tasks.html#add-a-task-from-the-command-line",
  "/tasks/toml-tasks.html#common-options":
    "/tasks/toml-tasks.html#common-properties",
  "/tasks/toml-tasks.html#detailed-task-examples":
    "/tasks/toml-tasks.html#define-a-task",
  "/tasks/toml-tasks.html#flags": "/tasks/task-arguments.html#tera-templates",
  "/tasks/toml-tasks.html#options": "/tasks/task-arguments.html#tera-templates",
  "/tasks/toml-tasks.html#positional-arguments":
    "/tasks/task-arguments.html#tera-templates",
  "/tasks/toml-tasks.html#recommended-using-the-usage-field":
    "/tasks/toml-tasks.html#arguments",
  "/tasks/toml-tasks.html#run-command": "/tasks/toml-tasks.html#run-commands",
  "/tasks/toml-tasks.html#sources-outputs":
    "/tasks/toml-tasks.html#sources-and-outputs",
  "/tasks/toml-tasks.html#specifying-which-directory-to-use":
    "/tasks/toml-tasks.html#working-directory",
  "/tasks/toml-tasks.html#tera-template-functions":
    "/tasks/task-arguments.html#tera-templates",
  "/tasks/toml-tasks.html#trivial-task-examples":
    "/tasks/toml-tasks.html#define-a-task",
  "/templates.html#additional-mise-functions": "/templates.html#functions",
  "/templates.html#available-context":
    "/templates.html#miserc-template-support",
  "/templates.html#examples": "/templates.html#exec",
  "/templates.html#exec-options": "/templates.html#exec",
  "/templates.html#general-functions": "/templates.html#functions",
  "/templates.html#mise-template-features": "/templates.html#variables",
  "/templates.html#miserc-toml-examples":
    "/templates.html#miserc-template-support",
  "/templates.html#not-available": "/templates.html#miserc-template-support",
  "/templates.html#task-specific-functions":
    "/templates.html#task-source-files",
  "/templates.html#template-rendering": "/templates.html#syntax",
  "/templates.html#tera-built-in-functions": "/templates.html#functions",
  "/templates.html#tera-filters": "/templates.html#filters",
  "/templates.html#tera-functions": "/templates.html#functions",
  "/templates.html#tera-tests": "/templates.html#tests",
  "/tips-and-tricks.html#ci-cd": "/continuous-integration.html",
  "/tips-and-tricks.html#github-actions":
    "/continuous-integration.html#github-actions",
  "/tips-and-tricks.html#installation-via-zsh-zinit":
    "/troubleshooting.html#the-wrong-version-of-a-tool-is-being-used",
  "/tips-and-tricks.html#lockfile-url-tracking-avoiding-rate-limits":
    "/dev-tools/mise-lock.html",
  "/tips-and-tricks.html#machine-bootstrapping": "/bootstrap.html",
  "/tips-and-tricks.html#minimum-release-age":
    "/security.html#minimum-release-age",
  "/tips-and-tricks.html#mise-cache-clear": "/cache-behavior.html",
  "/tips-and-tricks.html#mise-lock": "/dev-tools/mise-lock.html",
  "/tips-and-tricks.html#mise-run-shorthand":
    "/tasks/running-tasks.html#mise-run-shorthand",
  "/tips-and-tricks.html#mise-set": "/environments/#set-and-unset",
  "/tips-and-tricks.html#mise-up-bump": "/dev-tools/#upgrade-tools",
  "/tips-and-tricks.html#software-verification":
    "/security.html#download-verification",
  "/tips-and-tricks.html#watch-tasks-while-editing":
    "/tasks/running-tasks.html#rerun-tasks-when-files-change",
  "/tool-plugin-development.html#_1-plugin-structure":
    "/tool-plugin-development.html#quick-start",
  "/tool-plugin-development.html#_2-metadata-lua":
    "/tool-plugin-development.html#metadata-lua",
  "/tool-plugin-development.html#_3-helper-libraries":
    "/tool-plugin-development.html#complete-example",
  "/tool-plugin-development.html#advanced-features":
    "/tool-plugin-development.html#preinstall-hook",
  "/tool-plugin-development.html#available-hook-example":
    "/tool-plugin-development.html#complete-example",
  "/tool-plugin-development.html#best-practices":
    "/tool-plugin-development.html#common-mistakes",
  "/tool-plugin-development.html#caching": "/plugin-lua-modules.html#caching",
  "/tool-plugin-development.html#conditional-installation":
    "/tool-plugin-development.html#preinstall-hook",
  "/tool-plugin-development.html#creating-a-tool-plugin":
    "/tool-plugin-development.html#quick-start",
  "/tool-plugin-development.html#debug-mode":
    "/tool-plugin-development.html#testing-your-plugin",
  "/tool-plugin-development.html#environment-configuration":
    "/tool-plugin-development.html#envkeys-hook",
  "/tool-plugin-development.html#envkeys-hook-example":
    "/tool-plugin-development.html#envkeys-hook",
  "/tool-plugin-development.html#error-handling":
    "/tool-plugin-development.html#common-mistakes",
  "/tool-plugin-development.html#legacy-file-support":
    "/tool-plugin-development.html#parselegacyfile-hook",
  "/tool-plugin-development.html#local-development":
    "/tool-plugin-development.html#testing-your-plugin",
  "/tool-plugin-development.html#next-steps": "/plugin-publishing.html",
  "/tool-plugin-development.html#optional-hooks":
    "/tool-plugin-development.html#hook-functions",
  "/tool-plugin-development.html#platform-detection":
    "/plugin-lua-modules.html#runtime",
  "/tool-plugin-development.html#plugin-test-script":
    "/tool-plugin-development.html#testing-your-plugin",
  "/tool-plugin-development.html#postinstall-hook-example":
    "/tool-plugin-development.html#common-mistakes",
  "/tool-plugin-development.html#preinstall-hook-example":
    "/tool-plugin-development.html#complete-example",
  "/tool-plugin-development.html#preuse-hook":
    "/tool-plugin-development.html#differences-from-vfox",
  "/tool-plugin-development.html#real-world-example-vfox-nodejs":
    "/tool-plugin-development.html#complete-example",
  "/tool-plugin-development.html#required-hooks":
    "/tool-plugin-development.html#hook-functions",
  "/tool-plugin-development.html#source-compilation":
    "/tool-plugin-development.html#postinstall-hook",
  "/tool-plugin-development.html#using-the-template-repository":
    "/tool-plugin-development.html#quick-start",
  "/tool-plugin-development.html#version-normalization":
    "/tool-plugin-development.html#common-mistakes",
  "/tool-plugin-development.html#what-are-tool-plugins":
    "/tool-plugin-development.html#quick-start",
  "/troubleshooting.html#is-mise-secure": "/security.html",
  "/url-replacements.html#_1-protocol-conversion-http-to-https":
    "/url-replacements.html#http-to-https",
  "/url-replacements.html#_2-github-release-mirroring-with-path-restructuring":
    "/url-replacements.html#restructure-github-release-paths",
  "/url-replacements.html#_3-subdomain-to-path-conversion":
    "/url-replacements.html#subdomain-to-path",
  "/url-replacements.html#_4-multiple-replacement-patterns-processed-in-order":
    "/url-replacements.html#specific-rule-before-a-general-one",
  "/url-replacements.html#advanced-regex-replacement":
    "/url-replacements.html#regex-rules",
  "/url-replacements.html#authentication": "/url-replacements.html#netrc",
  "/url-replacements.html#configuration-examples": "/url-replacements.html",
  "/url-replacements.html#precedence-and-matching":
    "/url-replacements.html#how-rules-match",
  "/url-replacements.html#regex-examples": "/url-replacements.html#regex-rules",
  "/url-replacements.html#regex-syntax": "/url-replacements.html#regex-rules",
  "/url-replacements.html#routing-github-through-a-package-proxy":
    "/url-replacements.html#route-github-through-a-proxy",
  "/url-replacements.html#security-considerations":
    "/url-replacements.html#credentials",
  "/url-replacements.html#simple-hostname-replacement":
    "/url-replacements.html#how-rules-match",
  "/walkthrough.html#dev-tool-backends":
    "/walkthrough.html#installing-dev-tools",
  "/walkthrough.html#setting-environment-variables": "/environments/",
};
