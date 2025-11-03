# frozen_string_literal: true

module ReshimInstaller
  class << self
    def debug?
      ENV["MISE_DEBUG"] == "true"
    end

    def log_debug(msg)
      warn "[DEBUG] mise #{msg}" if debug?
    end

    def reshim
      if defined?(RbConfig::CONFIG)
        log_debug "reshim"
        `mise reshim`
      else
        log_debug "reshim skipped: ruby not found"
      end
    end
  end

  def install(options)
    super
    # We don't know which gems were installed, so always reshim.
    ReshimInstaller.reshim
  end
end unless defined?(ReshimInstaller)

if defined?(Bundler::Installer)
  Bundler::Installer.prepend ReshimInstaller
else
  Gem.post_install do |installer|
    # Reshim any (potentially) new executables.
    ReshimInstaller.reshim if installer.spec.executables.any?
  end
  Gem.post_uninstall do |installer|
    # Unfortunately, reshimming just the removed executables or
    # ruby version doesn't work as of 2020/04/23.
    ReshimInstaller.reshim if installer.spec.executables.any?
  end
end
