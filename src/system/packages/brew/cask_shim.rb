# frozen_string_literal: true

# mise's Homebrew cask lifecycle shim.
#
# This evaluates a sha-verified cask .rb file without requiring Homebrew and
# executes the lifecycle hook requested by Rust. It intentionally implements a
# small Cask DSL surface first; unsupported lifecycle code fails loudly instead
# of falling back to `brew`.

require "etc"
require "fileutils"
require "open3"
require "pathname"
require "rbconfig"
require "shellwords"

class Array
  def second
    self[1]
  end

  def third
    self[2]
  end

  def fourth
    self[3]
  end

  def fifth
    self[4]
  end
end

MISE_BREW_CASK_FILE = Pathname.new(ENV.fetch("MISE_BREW_CASK_FILE"))
MISE_BREW_CASK_TOKEN = ENV.fetch("MISE_BREW_CASK_TOKEN")
MISE_BREW_CASK_VERSION = ENV.fetch("MISE_BREW_CASK_VERSION")
MISE_BREW_CASK_STAGED_PATH = Pathname.new(ENV.fetch("MISE_BREW_CASK_STAGED_PATH"))
MISE_BREW_CASK_APPDIR = Pathname.new(ENV.fetch("MISE_BREW_CASK_APPDIR"))
MISE_BREW_PREFIX = Pathname.new(ENV.fetch("MISE_BREW_PREFIX"))
MISE_BREW_CASK_HOOK = ENV.fetch("MISE_BREW_CASK_HOOK")

def odie(message)
  $stderr.puts "Error: #{message}"
  exit 1
end

class ShimUnsupportedError < StandardError; end

def shim_unsupported!(feature)
  raise ShimUnsupportedError,
        "cask uses `#{feature}`, which mise's cask shim does not support"
end

module OS
  def self.mac?
    RbConfig::CONFIG["host_os"].include?("darwin")
  end

  def self.linux?
    RbConfig::CONFIG["host_os"].include?("linux")
  end
end

class MacOSVersion
  include Comparable

  SYMBOLS = {
    tahoe: "26", sequoia: "15", sonoma: "14", ventura: "13",
    monterey: "12", big_sur: "11", catalina: "10.15", mojave: "10.14",
    high_sierra: "10.13", sierra: "10.12", el_capitan: "10.11",
  }.freeze

  def self.host
    @host ||= begin
      version = OS.mac? ? `sw_vers -productVersion`.strip : "0"
      new(version.empty? ? "0" : version)
    end
  end

  def self.from_symbol(sym)
    new(SYMBOLS.fetch(sym.to_sym, "0"))
  end

  def initialize(version)
    @version = version.to_s
  end

  def <=>(other)
    other = self.class.from_symbol(other) if other.is_a?(Symbol)
    other = self.class.new(other.to_s) unless other.is_a?(MacOSVersion)
    Gem::Version.new(@version) <=> Gem::Version.new(other.to_s)
  end

  def same_release?(other)
    other = self.class.from_symbol(other) if other.is_a?(Symbol)
    other = self.class.new(other.to_s) unless other.is_a?(MacOSVersion)

    if major >= 11 || other.major >= 11
      major == other.major
    else
      major == other.major && minor == other.minor
    end
  end

  def to_s
    @version
  end

  protected

  def major
    version_parts[0] || 0
  end

  def minor
    version_parts[1] || 0
  end

  private

  def version_parts
    @version.split(".").map(&:to_i)
  end
end

module Hardware
  module CPU
    def self.arch
      RbConfig::CONFIG["host_cpu"] =~ /arm|aarch64/ ? :arm64 : :x86_64
    end

    def self.arm?
      arch == :arm64
    end

    def self.intel?
      arch == :x86_64
    end

    def self.cores
      Etc.respond_to?(:nprocessors) ? Etc.nprocessors : 4
    end
  end
end

class Version
  include Comparable

  def initialize(version)
    @version = version.to_s
  end

  def <=>(other)
    Gem::Version.new(@version.gsub(/[^0-9.].*\z/, "")) <=> Gem::Version.new(other.to_s.gsub(/[^0-9.].*\z/, ""))
  end

  def to_s
    @version
  end

  def to_str
    @version
  end

  def inspect
    @version.inspect
  end

  def major
    token(0)
  end

  def minor
    token(1)
  end

  def patch
    token(2)
  end

  def major_minor
    Version.new(@version.split(".")[0, 2].to_a.join("."))
  end

  def major_minor_patch
    Version.new(@version.split(".")[0, 3].to_a.join("."))
  end

  def csv
    @version.split(",").map { |part| Version.new(part) }
  end

  private

  def token(idx)
    part = @version.split(".")[idx]
    part.nil? ? nil : Version.new(part)
  end
end

class SystemCommandResult
  attr_reader :stdout, :stderr, :exit_status

  def initialize(stdout, stderr, exit_status)
    @stdout = stdout
    @stderr = stderr
    @exit_status = exit_status
  end

  def success?
    @exit_status.zero?
  end

  def merged_output
    @stdout + @stderr
  end
end

class CaskContext
  attr_reader :token

  def initialize(token)
    @token = token
    @version = Version.new(MISE_BREW_CASK_VERSION)
    @hooks = {}
    @arch = Hardware::CPU.arch.to_s
    @languages = {}
    @default_language = nil
  end

  def run_hook(name)
    hook = @hooks[name.to_sym]
    return unless hook

    instance_eval(&hook)
  end

  def arch(mapping = nil)
    return @arch if mapping.nil?

    @arch = if Hardware::CPU.arm?
      mapping.fetch(:arm, mapping.fetch("arm", @arch))
    else
      mapping.fetch(:intel, mapping.fetch("intel", @arch))
    end
  end

  def version(value = nil)
    @version = Version.new(value) unless value.nil?
    @version
  end

  def language(code = nil, default: false, &block)
    if code
      @languages[code.to_s] = instance_eval(&block)
      @default_language = code.to_s if default
      return
    end

    # The API metadata and downloaded artifact use the cask's default language,
    # so lifecycle evaluation must select the same variant.
    @languages[@default_language] || @languages.values.first
  end

  def on_system_conditional(values)
    key = OS.mac? ? :macos : :linux
    return values[key] if values.key?(key)
    return values[key.to_s] if values.key?(key.to_s)

    shim_unsupported!("on_system_conditional without #{key}")
  end

  def sha256(*) nil end
  def url(*) nil end
  def name(*) nil end
  def desc(*) nil end
  def homepage(*) nil end
  def livecheck(*) nil end
  def auto_updates(*) nil end
  def conflicts_with(*) nil end
  def depends_on(*) nil end
  def caveats(*) nil end
  def app(*) nil end
  def binary(*) nil end
  def pkg(*) nil end
  def font(*) nil end
  def manpage(*) nil end
  # Completion artifacts are linked natively by Rust; the stanzas only need to
  # parse so lifecycle hooks in the same cask can run.
  def bash_completion(*) nil end
  def zsh_completion(*) nil end
  def fish_completion(*) nil end
  def uninstall(*) nil end
  def zap(*) nil end

  def preflight(&block) @hooks[:preflight] = block end
  def postflight(&block) @hooks[:postflight] = block end
  def uninstall_preflight(&block) @hooks[:uninstall_preflight] = block end
  def uninstall_postflight(&block) @hooks[:uninstall_postflight] = block end

  def on_catalina(method = nil, &block) run_macos_condition(:catalina, method, &block) end
  def on_big_sur(method = nil, &block) run_macos_condition(:big_sur, method, &block) end
  def on_monterey(method = nil, &block) run_macos_condition(:monterey, method, &block) end
  def on_ventura(method = nil, &block) run_macos_condition(:ventura, method, &block) end
  def on_sonoma(method = nil, &block) run_macos_condition(:sonoma, method, &block) end
  def on_sequoia(method = nil, &block) run_macos_condition(:sequoia, method, &block) end
  def on_tahoe(method = nil, &block) run_macos_condition(:tahoe, method, &block) end
  def on_intel(&block) instance_eval(&block) if Hardware::CPU.intel? end
  def on_arm(&block) instance_eval(&block) if Hardware::CPU.arm? end
  def on_macos(&block) instance_eval(&block) if OS.mac? end
  def on_linux(&block) instance_eval(&block) if OS.linux? end

  # Mirrors Homebrew's Cask DSL `system_command` (SystemCommand.run!): raises
  # on failure unless must_succeed: false. Elevation follows mise's sudo
  # policy, passed down from Rust via MISE_BREW_CASK_SUDO:
  # - "interactive": run `sudo`, which may prompt on the controlling TTY
  # - "noninteractive": run `sudo -n`, so it fails instead of hanging on a
  #   password prompt that nothing can answer
  # - "deny": error with the exact command to run manually
  def system_command(executable, args: [], sudo: false, env: {}, must_succeed: true,
                     print_stdout: false, print_stderr: true, verbose: false,
                     input: nil, chdir: nil, **rest)
    shim_unsupported!("system_command #{rest.keys.join(", ")}") if rest.any?

    env = env.map { |k, v| [k.to_s, v.to_s] }.to_h
    argv = [executable.to_s, *args.map(&:to_s)]
    argv = sudo_argv(argv, env) if sudo && Process.euid != 0

    opts = {}
    opts[:chdir] = chdir.to_s if chdir
    opts[:stdin_data] = input.to_s if input
    stdout, stderr, status = Open3.capture3(env, *argv, **opts)
    $stdout.print(stdout) if print_stdout || verbose
    $stderr.print(stderr) if print_stderr || verbose

    result = SystemCommandResult.new(stdout, stderr, status.exitstatus || 1)
    if must_succeed && !result.success?
      odie "command failed (exit #{result.exit_status}): #{argv.shelljoin}\n#{stderr}"
    end
    result
  end

  def staged_path
    MISE_BREW_CASK_STAGED_PATH
  end

  def appdir
    MISE_BREW_CASK_APPDIR
  end

  def caskroom_path
    MISE_BREW_PREFIX + "Caskroom"
  end

  def method_missing(name, *args, &block)
    shim_unsupported!(name)
  end

  def respond_to_missing?(*args)
    false
  end

  private

  def sudo_argv(argv, env)
    # sudo resets the environment; pass env vars through `env` so they reach
    # the elevated command (same as mise's sudo::run on the Rust side)
    argv = ["env", *env.map { |k, v| "#{k}=#{v}" }, *argv] unless env.empty?
    case ENV.fetch("MISE_BREW_CASK_SUDO", "deny")
    when "interactive"
      ["sudo", *argv]
    when "noninteractive"
      ["sudo", "-n", *argv]
    else
      odie "cask lifecycle hook needs sudo, but mise is not allowed to elevate " \
           "(system_packages.sudo is disabled or sudo is unavailable). Run manually:\n" \
           "  #{(["sudo"] + argv).shelljoin}"
    end
  end

  def run_macos_condition(version, method, &block)
    target = MacOSVersion.from_symbol(version)
    host = MacOSVersion.host
    matched = case method
    when :or_older then host <= target
    when :or_newer then host >= target
    when nil then host.same_release?(target)
    else shim_unsupported!("on_#{version} #{method}")
    end
    instance_eval(&block) if matched
  end
end

def cask(token, &block)
  $mise_cask_context = CaskContext.new(token)
  $mise_cask_context.instance_eval(&block)
end

begin
  load MISE_BREW_CASK_FILE.to_s
  ctx = $mise_cask_context
  odie "no cask block found in #{MISE_BREW_CASK_FILE}" if ctx.nil?
  odie "expected cask #{MISE_BREW_CASK_TOKEN}, got #{ctx.token}" if ctx.token != MISE_BREW_CASK_TOKEN
  ctx.run_hook(MISE_BREW_CASK_HOOK)
rescue ShimUnsupportedError => e
  odie e.message
end
