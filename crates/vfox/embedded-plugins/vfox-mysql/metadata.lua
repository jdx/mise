PLUGIN = {}

PLUGIN.name = "mysql"
PLUGIN.version = "0.1.0"
PLUGIN.homepage = "https://github.com/jdx/vfox-mysql"
PLUGIN.license = "MIT"
PLUGIN.description = "MySQL Database"
PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
  "Uses MySQL's official download page for current releases and dbdeployer's metadata for archives.",
}

-- This file's top level runs every time the plugin's metadata is loaded, so it
-- must stay pure data: no subprocesses, no reads of host state. Where a package
-- is named differently across distro releases, list the candidates and let the
-- package manager be asked which one it has. Detection (the checks below) is
-- the source of truth regardless; `packages` only names what to install.
PLUGIN.systemDependencies = {
  {
    sharedlib = "libncurses.so.6",
    packages = {
      apt = "libncurses6",
      dnf = "ncurses-libs",
      pacman = "ncurses",
    },
  },
  {
    command = "test \"$(uname -s)\" != Linux || ldconfig -p 2>/dev/null | grep -Eq 'libaio\\.so\\.1(t64)? '",
    packages = {
      -- the 64-bit time_t transition renamed this on Ubuntu >= 24.04 and
      -- Debian >= 13; older releases still carry libaio1
      apt = { "libaio1t64", "libaio1" },
      dnf = "libaio",
      pacman = "libaio",
    },
  },
  {
    sharedlib = "libnuma.so.1",
    packages = {
      apt = "libnuma1",
      dnf = "numactl-libs",
      pacman = "numactl",
    },
  },
}
