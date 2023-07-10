module ReshimInstaller
  def install(options)
    super
    # We don't know which gems were installed, so always reshim.
    `rtx reshim`
  end
end

if defined?(Bundler::Installer)
  Bundler::Installer.prepend ReshimInstaller
else
  Gem.post_install do |installer|
    # Reshim any (potentially) new executables.
    `rtx reshim` if installer.spec.executables.any?
  end
  Gem.post_uninstall do |installer|
    # Unfortunately, reshimming just the removed executables or
    # ruby version doesn't work as of 2020/04/23.
    `rtx reshim` if installer.spec.executables.any?
  end
end
