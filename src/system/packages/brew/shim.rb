# frozen_string_literal: true

# mise's Homebrew formula build shim.
#
# Evaluates a homebrew/core formula .rb file and runs its `install` method
# against mise's prefix layout, without Homebrew installed. This implements
# the commonly-used subset of the Formula DSL; formulae that reach outside it
# fail loudly with a clear message rather than miscompiling silently.
#
# Contract with the Rust side (src/system/packages/brew/source.rs), all via
# environment variables:
#   MISE_BREW_PREFIX        Homebrew prefix (/opt/homebrew, ...)
#   MISE_BREW_CELLAR        <prefix>/Cellar
#   MISE_BREW_FORMULA_FILE  path to the formula .rb (sha-verified by Rust)
#   MISE_BREW_NAME          canonical formula name
#   MISE_BREW_VERSION       upstream version ("2.12.3")
#   MISE_BREW_PKG_VERSION   keg directory name ("2.12.3_1")
#   MISE_BREW_BUILDPATH     staged source directory (also the cwd)
#   MISE_BREW_CACHE         download cache for resources/patches
#   MISE_BREW_MAKE_JOBS     build parallelism
#
# The main source archive is downloaded, verified, and staged by Rust before
# this script runs; the shim only downloads resources and external patches
# (each verified against the sha256 declared in the formula).

require "digest/sha2"
require "date"
require "etc"
require "fileutils"
require "open-uri"
require "open3"
require "pathname"
require "rbconfig"
require "shellwords"
require "tmpdir"

MISE_BREW_PREFIX = Pathname.new(ENV.fetch("MISE_BREW_PREFIX"))
MISE_BREW_CELLAR = Pathname.new(ENV.fetch("MISE_BREW_CELLAR"))
MISE_BREW_FORMULA_FILE = Pathname.new(ENV.fetch("MISE_BREW_FORMULA_FILE"))
MISE_BREW_NAME = ENV.fetch("MISE_BREW_NAME")
MISE_BREW_VERSION = ENV.fetch("MISE_BREW_VERSION")
MISE_BREW_PKG_VERSION = ENV.fetch("MISE_BREW_PKG_VERSION")
MISE_BREW_BUILDPATH = Pathname.new(ENV.fetch("MISE_BREW_BUILDPATH"))
MISE_BREW_CACHE = Pathname.new(ENV.fetch("MISE_BREW_CACHE"))
MISE_BREW_MAKE_JOBS = ENV.fetch("MISE_BREW_MAKE_JOBS", "4")

def ohai(*args)
  $stdout.puts "==> #{args.join(" ")}"
  $stdout.flush
end

def opoo(message)
  $stderr.puts "Warning: #{message}"
end

def odie(message)
  $stderr.puts "Error: #{message}"
  exit 1
end

class ShimUnsupportedError < StandardError; end

def shim_unsupported!(feature)
  raise ShimUnsupportedError,
        "formula uses `#{feature}`, which mise's source-build shim does not support"
end

def checked_command_output!(*command)
  output, status = Open3.capture2e(*command)
  value = output.strip
  return value if status.success? && !value.empty? && !value.include?("\0")

  raise "command produced no valid output: #{command.join(" ")}"
end

module MiseDownload
  module_function

  # download with redirects into the cache, verify, return the path
  def fetch(url, sha256, context)
    raise "#{context}: missing sha256" if sha256.to_s.strip.empty?
    sha256 = sha256.to_s.strip.downcase
    raise "#{context}: malformed sha256" unless sha256.match?(/\A[0-9a-f]{64}\z/)

    MISE_BREW_CACHE.mkpath
    dest = MISE_BREW_CACHE + "#{sha256}--#{File.basename(URI(url).path)}"
    unless dest.file? && Digest::SHA256.file(dest).hexdigest == sha256
      ohai "Downloading #{url}"
      tmp = Pathname.new("#{dest}.incomplete")
      URI.open(url, "rb", redirect: true) do |remote|
        tmp.open("wb") { |f| IO.copy_stream(remote, f) }
      end
      actual = Digest::SHA256.file(tmp).hexdigest
      if actual != sha256
        tmp.unlink
        raise "#{context}: sha256 mismatch (expected #{sha256}, got #{actual})"
      end
      tmp.rename(dest)
    end
    dest
  end

  # unpack an archive the way brew stages sources: if the archive contains a
  # single top-level directory, its contents become the stage root
  def unpack(archive, dest)
    dest.mkpath
    case archive.basename.to_s
    when /\.(tar\.(gz|xz|bz2|zst)|tgz|txz|tbz2?|tar|crate)\z/i
      system_or_die "tar", "xf", archive.to_s, "-C", dest.to_s
    when /\.zip\z/i
      system_or_die "unzip", "-qo", archive.to_s, "-d", dest.to_s
    when /\.(gz|xz|bz2)\z/i
      data = `#{archive.to_s =~ /xz\z/ ? "xz -dc" : archive.to_s =~ /bz2\z/ ? "bzip2 -dc" : "gzip -dc"} #{Shellwords.escape(archive.to_s)}`
      raise "failed to decompress #{archive}" unless $?.success?
      (dest + archive.basename.to_s.sub(/\.(gz|xz|bz2)\z/i, "")).binwrite(data)
    else
      FileUtils.cp archive, dest
    end
    entries = dest.children
    return entries.first if entries.size == 1 && entries.first.directory?
    dest
  end

  def system_or_die(*args)
    raise "command failed: #{args.join(" ")}" unless system(*args)
  end
end

module OS
  def self.mac? = RbConfig::CONFIG["host_os"].include?("darwin")
  def self.linux? = RbConfig::CONFIG["host_os"].include?("linux")

  module Mac
    def self.version = MacOSVersion.host
  end

  module Linux
    def self.languages = ["en"]
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
      version = OS.mac? ? checked_command_output!("/usr/bin/sw_vers", "-productVersion") : "0"
      raise "invalid macOS version: #{version.inspect}" unless version.match?(/\A\d+(?:\.\d+)*\z/)

      new(version)
    end
  end

  def self.from_symbol(sym) = new(SYMBOLS.fetch(sym.to_sym))

  def initialize(version) = @version = version.to_s

  def <=>(other)
    other = self.class.from_symbol(other) if other.is_a?(Symbol)
    other = self.class.new(other.to_s) unless other.is_a?(MacOSVersion)
    Gem::Version.new(@version) <=> Gem::Version.new(other.to_s)
  end

  def to_s = @version
  def major = @version.split(".").first.to_i
  def requires_nehalem_cpu?
    shim_unsupported!("MacOSVersion#requires_nehalem_cpu?; exact Intel CPU feature detection is not implemented")
  end
  alias requires_sse4? requires_nehalem_cpu?
  alias requires_sse41? requires_nehalem_cpu?
  alias requires_sse42? requires_nehalem_cpu?
  alias requires_popcnt? requires_nehalem_cpu?
end

module Hardware
  module CPU
    def self.arch = RbConfig::CONFIG["host_cpu"] =~ /arm|aarch64/ ? :arm64 : :x86_64
    def self.arm? = arch == :arm64
    def self.intel? = arch == :x86_64
    def self.is_64_bit? = true
    def self.cores = Etc.respond_to?(:nprocessors) ? Etc.nprocessors : 4
  end
end

module MacOS
  def self.version = MacOSVersion.host

  def self.sdk_path
    @sdk_path ||= begin
      path = Pathname.new(checked_command_output!("/usr/bin/xcrun", "--show-sdk-path"))
      raise "xcrun returned an invalid SDK path: #{path}" unless path.absolute? && path.directory?

      path
    end
  end

  def self.sdk_path_if_needed = OS.mac? ? sdk_path : nil

  module CLT
    def self.installed? = OS.mac? && File.directory?("/Library/Developer/CommandLineTools")
  end

  module Xcode
    def self.installed? = shim_unsupported!("MacOS::Xcode.installed?; exact Xcode detection is not implemented")
  end
end

class Version
  include Comparable

  NUMERIC = /\A\d+(?:\.\d+)*\z/

  def initialize(version) = @version = version.to_s

  def <=>(other)
    other = other.to_s
    unless @version.match?(NUMERIC) && other.match?(NUMERIC)
      shim_unsupported!("opaque version comparison #{@version.inspect} <=> #{other.inspect}")
    end

    Gem::Version.new(@version) <=> Gem::Version.new(other)
  end
  def to_s = @version
  def to_str = @version
  def inspect = @version.inspect

  def major = token(0)
  def minor = token(1)
  def patch = token(2)
  def major_minor = Version.new(@version.split(".")[0, 2].to_a.join("."))
  def major_minor_patch = Version.new(@version.split(".")[0, 3].to_a.join("."))
  def csv = @version.split(",").map { |part| Version.new(part) }

  private

  def token(idx)
    part = @version.split(".")[idx]
    part.nil? ? nil : Version.new(part)
  end
end

# Pathname extensions mirroring brew's
class Pathname
  def install(*sources)
    mkpath
    sources.flatten.each do |src|
      case src
      when Hash
        src.each { |from, to| install_one(from, self + to) }
      else
        install_one(src, self + Pathname.new(src.to_s).basename)
      end
    end
  end

  def install_one(src, dest)
    src = Pathname.new(src.to_s)
    raise "cannot install #{src}: does not exist" unless src.exist? || src.symlink?
    dest.dirname.mkpath
    FileUtils.mv src.to_s, dest.to_s
  end
  private :install_one

  def install_symlink(*sources)
    mkpath
    sources.flatten.each do |src|
      case src
      when Hash
        src.each { |from, to| make_relative_symlink(Pathname.new(from.to_s), self + to) }
      else
        src = Pathname.new(src.to_s)
        make_relative_symlink(src, self + src.basename)
      end
    end
  end

  def make_relative_symlink(target, link)
    link.dirname.mkpath
    link.unlink if link.symlink? || link.file?
    link.make_symlink(target.relative_path_from(link.dirname))
  end
  private :make_relative_symlink

  def install_metafiles(from = Pathname.pwd)
    mkpath
    Pathname.new(from).children.each do |child|
      next unless child.file?
      next unless child.basename.to_s =~ /\A(readme|license|licence|copying|copyright|news|changelog|changes|authors)(\.|\z)/i
      FileUtils.cp child, self
    end
  end

  def write(content, *args)
    dirname.mkpath
    File.write(to_s, content, *args)
  end

  def atomic_write(content)
    dirname.mkpath
    File.write(to_s, content)
  end

  def append_lines(content)
    raise "Cannot append file that doesn't exist: #{self}" unless exist?

    open("a") { |f| f.puts(content) }
  end

  def ensure_executable!
    chmod(0o755) if file?
  end
end

class BuildOptions
  def with?(_name) = false
  def without?(name) = !with?(name)
  def head? = false
  def stable? = true
  def bottle? = false
  def include?(_name) = false
  def used_options = []
  def unused_options = []
end

# brew's build-environment helpers, grafted onto the real ENV object the way
# brew's EnvActivation does — formulae call `ENV.append`, `ENV.cc`, etc. and
# constant lookup always resolves `ENV` to Object::ENV, so the global must
# carry the methods itself
module BrewEnvExtension
  def append(keys, value, separator = " ")
    Array(keys).each do |key|
      old = self[key.to_s]
      self[key.to_s] = old.nil? || old.empty? ? value.to_s : "#{old}#{separator}#{value}"
    end
  end

  def prepend(keys, value, separator = " ")
    Array(keys).each do |key|
      old = self[key.to_s]
      self[key.to_s] = old.nil? || old.empty? ? value.to_s : "#{value}#{separator}#{old}"
    end
  end

  def append_path(key, path) = append(key, path, File::PATH_SEPARATOR)
  def prepend_path(key, path) = prepend(key, path, File::PATH_SEPARATOR)

  def prepend_create_path(key, path)
    Pathname.new(path.to_s).mkpath
    prepend_path(key, path)
  end

  def remove(keys, value)
    Array(keys).each do |key|
      next if self[key.to_s].nil?
      self[key.to_s] = self[key.to_s].sub(value, "").strip
    end
  end

  def append_to_cflags(flag) = append(%w[CFLAGS CXXFLAGS], flag)
  def remove_from_cflags(flag) = remove(%w[CFLAGS CXXFLAGS], flag)

  def cc = fetch("CC", "cc")
  def cxx = fetch("CXX", "c++")
  def cflags = self["CFLAGS"]
  def cxxflags = self["CXXFLAGS"]
  def cppflags = self["CPPFLAGS"]
  def ldflags = self["LDFLAGS"]

  def make_jobs = MISE_BREW_MAKE_JOBS.to_i

  def deparallelize
    old = delete("MAKEFLAGS")
    if block_given?
      begin
        yield
      ensure
        self["MAKEFLAGS"] = old unless old.nil?
      end
    end
    old
  end

  def method_missing(name, *_args)
    shim_unsupported!("ENV.#{name}; exact Homebrew build-environment semantics are not implemented")
  end

  def respond_to_missing?(_name, _include_private = false) = true
end

ENV.extend(BrewEnvExtension)

class Resource
  attr_reader :name
  attr_accessor :owner

  def initialize(name)
    @name = name
    @specs = {}
  end

  def url(url = nil, **specs)
    @url = url unless url.nil?
    @specs.merge!(specs)
    @url
  end

  def sha256(sha = nil)
    @sha256 = sha unless sha.nil?
    @sha256
  end

  def version(version = nil)
    @version = version unless version.nil?
    @version || (@url && Version.new(@url[/[0-9]+(?:\.[0-9]+)+/].to_s))
  end

  def mirror(url) = (@mirrors ||= []) << url
  def using = @specs[:using]
  def livecheck(&) = nil

  def patch(*, &)
    # building against an unpatched resource silently produces a wrong
    # artifact — refuse instead
    shim_unsupported!("resource patches")
  end

  # unpack into `target` (or a tmpdir) and run the optional block there
  def stage(target = nil, &block)
    shim_unsupported!("resource with download strategy #{using.inspect}") unless using.nil?
    raise "resource #{name}: missing url" if @url.nil?
    archive = MiseDownload.fetch(@url, @sha256, "resource #{name}")
    if target
      target = Pathname.new(target.to_s)
      stage_dir = Pathname.new(Dir.mktmpdir("mise-resource-"))
      root = MiseDownload.unpack(archive, stage_dir)
      target.mkpath
      FileUtils.cp_r("#{root}/.", target.to_s)
      FileUtils.remove_entry(stage_dir)
      block&.call(self)
    else
      raise "resource #{name}: stage requires a target or a block" unless block
      Dir.mktmpdir("mise-resource-") do |dir|
        root = MiseDownload.unpack(archive, Pathname.new(dir))
        Dir.chdir(root) { block.call(self) }
      end
    end
  end
end

class PatchSpec
  def initialize(strip, formula_file)
    @strip = strip
    @formula_file = formula_file
  end

  def url(url = nil, **)
    @url = url unless url.nil?
    @url
  end

  def sha256(sha = nil)
    @sha256 = sha unless sha.nil?
    @sha256
  end

  def data!
    @data = true
  end

  def apply!
    if @data
      content = @formula_file.read.split(/^__END__$/, 2)[1]
      raise "formula declares a DATA patch but has no __END__ section" if content.nil?
      apply_content(content.sub(/\A\n/, ""))
    elsif @url
      file = MiseDownload.fetch(@url, @sha256, "patch")
      apply_content(file.read)
    end
  end

  private

  def apply_content(content)
    ohai "Applying patch (-p#{@strip})"
    Open3.popen2e("patch", "-g", "0", "-f", "-p#{@strip}") do |stdin, out, thread|
      stdin.write(content)
      stdin.close
      output = out.read
      raise "patch failed:\n#{output}" unless thread.value.success?
      $stdout.puts output
    end
  end
end

# reference to an installed dependency, as returned by Formula["name"]
class DependencyFormula
  def initialize(name) = @name = name

  def opt_prefix = MISE_BREW_PREFIX + "opt" + @name
  def opt_bin = opt_prefix + "bin"
  def opt_lib = opt_prefix + "lib"
  def opt_include = opt_prefix + "include"
  def opt_libexec = opt_prefix + "libexec"
  def opt_share = opt_prefix + "share"
  def opt_frameworks = opt_prefix + "Frameworks"

  # resolved keg path (through the opt symlink)
  def prefix = opt_prefix.exist? ? opt_prefix.realpath : opt_prefix
  def bin = prefix + "bin"
  def lib = prefix + "lib"
  def include = prefix + "include"
  def libexec = prefix + "libexec"
  def share = prefix + "share"

  def installed? = opt_prefix.exist?
  def any_installed? = installed?

  def version
    Version.new(prefix.basename.to_s.sub(/_\d+\z/, ""))
  rescue StandardError
    Version.new("0")
  end

  def name = @name
  def to_s = @name
end

class Formula
  include FileUtils

  class << self
    def inherited(subclass)
      super
      Formula.instance_variable_set(:@formula_subclass, subclass)
    end

    def [](name) = DependencyFormula.new(name.to_s)

    # ---- recorded-but-inert metadata DSL ----
    def desc(*); end
    def homepage(*); end
    def license(*); end
    def revision(*); end
    def version_scheme(*); end
    def compatibility_version(*); end
    def no_autobump!(*, **); end
    def mirror(*); end

    def url(url = nil, **)
      @url = url
    end

    def sha256(*args)
      # top-level stable sha; bottle-block shas never reach here because the
      # bottle block is not evaluated
    end

    def version(v = nil)
      @explicit_version = v unless v.nil?
    end

    # blocks that must not be evaluated
    def bottle(&) = nil
    def head(*, &) = nil
    def livecheck(&) = nil
    def service(&) = nil
    def test(&) = nil
    def plist_options(*); end

    def stable(&block)
      class_exec(&block) if block
    end

    def depends_on(*); end
    def uses_from_macos(*, **); end
    def keg_only(*); end
    def skip_clean(*); end
    def link_overwrite(*); end
    def conflicts_with(*, **); end
    def fails_with(*, &) = shim_unsupported!("fails_with compiler selection")
    def needs(*) = shim_unsupported!("needs build requirement")
    def env(*) = shim_unsupported!("env build-environment selection")
    def option(*, **); end
    def deprecated_option(*); end
    def pour_bottle?(*, &) = shim_unsupported!("pour_bottle? predicate")
    def allow_network_access!(*); end
    def deny_network_access!(*); end

    def deprecate!(**kwargs)
      opoo "formula is deprecated upstream#{kwargs[:because] ? " (#{kwargs[:because]})" : ""}"
    end

    def disable!(date:, because:, **)
      return if Date.parse(date) > Date.today

      shim_unsupported!("disabled formula (#{because})")
    end

    # ---- platform-conditional blocks ----
    def on_macos(&block)
      class_exec(&block) if OS.mac? && block
    end

    def on_linux(&block)
      class_exec(&block) if OS.linux? && block
    end

    def on_arm(&block)
      class_exec(&block) if Hardware::CPU.arm? && block
    end

    def on_intel(&block)
      class_exec(&block) if Hardware::CPU.intel? && block
    end

    # `on_system :linux, macos: :ventura_or_older` — run on linux, or on
    # macOS when the host version matches the comparator
    def on_system(linux, macos:, &block)
      raise ArgumentError, "The first argument to `on_system` must be `:linux`" unless linux == :linux

      parsed = parse_macos_condition(macos)
      run = OS.linux?
      run ||= OS.mac? && macos_condition_matches?(parsed)
      class_exec(&block) if run && block
    end

    def parse_macos_condition(condition)
      sym = condition.to_s
      base, comparator = if sym.end_with?("_or_older")
        [sym.delete_suffix("_or_older"), :or_older]
      elsif sym.end_with?("_or_newer")
        [sym.delete_suffix("_or_newer"), :or_newer]
      else
        [sym, :==]
      end
      unless MacOSVersion::SYMBOLS.key?(base.to_sym)
        # an unknown version symbol must not silently skip install logic
        shim_unsupported!("on_system macos condition #{condition.inspect}")
      end
      [base.to_sym, comparator]
    end

    def macos_condition_matches?(parsed)
      base, comparator = parsed
      host = MacOSVersion.host
      target = MacOSVersion.from_symbol(base)
      case comparator
      when :or_older then host <= target
      when :or_newer then host >= target
      else host.major == target.major
      end
    end
    private :parse_macos_condition, :macos_condition_matches?

    # macOS-version blocks (on_sonoma, on_ventura :or_older, ...)
    MacOSVersion::SYMBOLS.each_key do |sym|
      define_method(:"on_#{sym}") do |comparator = nil, &block|
        unless [nil, :or_older, :or_newer].include?(comparator)
          raise ArgumentError, "Invalid OS `or_*` condition: #{comparator.inspect}"
        end
        next unless OS.mac? && block
        host = MacOSVersion.host
        target = MacOSVersion.from_symbol(sym)
        run = case comparator
              when :or_older then host <= target
              when :or_newer then host >= target
              else host.major == target.major
              end
        class_exec(&block) if run
      end
    end

    # ---- resources & patches ----
    def resource(name, &)
      shim_unsupported!("resource #{name.inspect}; network-fetched resources are not supported")
    end

    def resources = (@resources ||= {})

    def patch(strip = :p1, src = nil, &block)
      shim_unsupported!("formula patches; source builds require pre-fetched typed patches")
    end

    def patches = (@patches ||= [])

    def method_missing(name, *_args, &_block)
      shim_unsupported!("unknown formula DSL `#{name}`")
    end

    def respond_to_missing?(_name, _include_private = false) = true
  end

  # ---- instance API used inside def install ----
  def name = MISE_BREW_NAME
  def version = Version.new(MISE_BREW_VERSION)
  def pkg_version = MISE_BREW_PKG_VERSION
  def build = BuildOptions.new
  def head? = false
  def stable? = true

  def prefix = MISE_BREW_CELLAR + name + MISE_BREW_PKG_VERSION
  def opt_prefix = MISE_BREW_PREFIX + "opt" + name
  def opt_bin = opt_prefix + "bin"
  def opt_lib = opt_prefix + "lib"
  def opt_libexec = opt_prefix + "libexec"
  def opt_share = opt_prefix + "share"

  def bin = prefix + "bin"
  def sbin = prefix + "sbin"
  def lib = prefix + "lib"
  def libexec = prefix + "libexec"
  def include = prefix + "include"
  def frameworks = prefix + "Frameworks"
  def share = prefix + "share"
  def pkgshare = share + name
  def elisp = share + "emacs/site-lisp" + name
  def man = share + "man"
  (1..8).each { |n| define_method(:"man#{n}") { man + "man#{n}" } }
  def doc = share + "doc" + name
  def info = share + "info"
  def bash_completion = prefix + "etc/bash_completion.d"
  def zsh_completion = share + "zsh/site-functions"
  def fish_completion = share + "fish/vendor_completions.d"
  def pkgetc = etc + name

  # Formula install must stage persistent defaults privately. The common Rust
  # finalizer performs Homebrew's install_etc_var merge after the receipt and
  # public link are ready, preserving existing user configuration.
  def etc = prefix + ".bottle/etc"
  def var = prefix + ".bottle/var"

  def buildpath = MISE_BREW_BUILDPATH
  def testpath = shim_unsupported!("testpath")

  def deps = shim_unsupported!("deps; typed Dependency objects are not implemented")
  def declared_deps = shim_unsupported!("declared_deps; typed Dependency objects are not implemented")

  def resource(name)
    res = self.class.resources.fetch(name) { raise "undefined resource #{name.inspect}" }
    res.owner = self
    res
  end

  def resources = self.class.resources.values.each { |r| r.owner = self }

  # ---- build helpers ----
  def system(cmd, *args)
    args = args.map(&:to_s)
    pretty = ([cmd] + args).join(" ")
    ohai pretty
    # single-string invocations go through the shell, like Kernel#system
    ok = Kernel.system(cmd.to_s, *args)
    raise "command failed: #{pretty}" unless ok
  end

  def quiet_system(cmd, *args)
    Kernel.system(cmd.to_s, *args.map(&:to_s), out: File::NULL, err: File::NULL)
  end

  def which(cmd)
    ENV.fetch("PATH", "").split(File::PATH_SEPARATOR).each do |dir|
      candidate = Pathname.new(dir) + cmd.to_s
      return candidate if candidate.executable? && candidate.file?
    end
    nil
  end

  def inreplace(paths, before = nil, after = nil, audit_result: true, global: true, &block)
    paths = Array(paths)
    if paths.empty? || paths.all? { |path| path.nil? || path.to_s.empty? }
      raise "inreplace: `paths` was empty"
    end
    paths.each do |path|
      path = Pathname.new(path.to_s)
      content = path.read
      replaced = content.dup
      if block
        ext = InreplaceText.new(replaced)
        block.call(ext)
        raise "inreplace in #{path} failed: #{ext.errors.join("; ")}" unless ext.errors.empty?

        replaced = ext.text
      else
        raise "inreplace: missing before/after" if before.nil? || after.nil?

        before = before.to_s if before.is_a?(Pathname)
        result = global ? replaced.gsub!(before, after.to_s) : replaced.sub!(before, after.to_s)
        if result.nil? && audit_result
          raise "inreplace in #{path}: expected replacement of #{before.inspect} with #{after.inspect}"
        end
      end
      path.write(replaced)
    end
  end

  def cd(path, &block) = Dir.chdir(path.to_s, &block)

  def mkdir(name, &block)
    path = Pathname.new(name.to_s)
    path.mkpath
    block ? Dir.chdir(path.to_s, &block) : path
  end

  def loader_path = OS.mac? ? "@loader_path" : "$ORIGIN"

  def rpath(source: bin, target: lib)
    "#{loader_path}/#{Pathname.new(target.to_s).relative_path_from(Pathname.new(source.to_s))}"
  end

  def std_configure_args
    [
      "--disable-debug",
      "--disable-dependency-tracking",
      "--disable-silent-rules",
      "--prefix=#{prefix}",
      "--libdir=#{lib}",
    ]
  end

  def std_cmake_args(install_prefix: prefix, install_libdir: "lib", find_framework: "LAST")
    [
      "-DCMAKE_INSTALL_PREFIX=#{install_prefix}",
      "-DCMAKE_INSTALL_LIBDIR=#{install_libdir}",
      "-DCMAKE_BUILD_TYPE=Release",
      "-DCMAKE_FIND_FRAMEWORK=#{find_framework}",
      "-DCMAKE_VERBOSE_MAKEFILE=ON",
      "-DBUILD_TESTING=OFF",
      "-Wno-dev",
    ]
  end

  def std_meson_args
    ["--prefix=#{prefix}", "--libdir=#{lib}", "--buildtype=release", "--wrap-mode=nofallback"]
  end

  def std_cargo_args(root: prefix, path: ".")
    ["--jobs", ENV["MAKEFLAGS"].to_s[/-j(\d+)/, 1] || MISE_BREW_MAKE_JOBS, "--locked", "--root", root.to_s, "--path", path.to_s].tap(&:compact!)
  end

  def std_go_args(ldflags: nil, output: bin/name, tags: nil)
    args = ["-trimpath", "-o=#{output}"]
    args += ["-tags=#{Array(tags).join(",")}"] if tags
    args += ["-ldflags=#{Array(ldflags).join(" ")}"] if ldflags
    args
  end

  def generate_completions_from_executable(*commands, base_name: name, shells: [:bash, :zsh, :fish], shell_parameter_format: nil)
    shells.each do |shell|
      completion_dir = { bash: bash_completion, zsh: zsh_completion, fish: fish_completion }.fetch(shell)
      file_name = { bash: base_name, zsh: "_#{base_name}", fish: "#{base_name}.fish" }.fetch(shell)
      shell_arg = case shell_parameter_format
                  when nil then shell.to_s
                  when :flag then "--#{shell}"
                  when :arg then "--shell=#{shell}"
                  when :none then nil
                  when :click then nil # click uses env vars; handled below
                  when String then "#{shell_parameter_format}#{shell}"
                  end
      cmd = commands.map(&:to_s)
      cmd << shell_arg unless shell_arg.nil?
      env = {}
      if shell_parameter_format == :click
        env["_#{base_name.upcase.tr("-", "_")}_COMPLETE"] = "#{shell}_source"
      end
      output, status = Open3.capture2(env, *cmd)
      raise "completion generation failed: #{cmd.join(" ")}" unless status.success?
      completion_dir.mkpath
      (completion_dir + file_name).write(output)
    end
  end

  def time = Time.now

  def post_install; end

  # unknown instance helpers: fail loudly
  def method_missing(name, *args, &block)
    shim_unsupported!("#{name} (install-time helper)")
  end

  def respond_to_missing?(_name, _include_private = false) = true

  class InreplaceText
    attr_reader :errors, :text

    def initialize(text)
      @text = text
      @errors = []
    end

    def gsub!(before, after, audit_result: true)
      before = before.to_s if before.is_a?(Pathname)
      result = @text.gsub!(before, after.to_s)
      @errors << "expected replacement of #{before.inspect} with #{after.inspect}" if result.nil? && audit_result
      result
    end

    def sub!(before, after, audit_result: true)
      result = @text.sub!(before, after.to_s)
      @errors << "expected replacement of #{before.inspect} with #{after.inspect}" if result.nil? && audit_result
      result
    end

    def change_make_var!(flag, new_value)
      replaced = @text.gsub!(/^#{Regexp.escape(flag)}[ \t]*[\\?\\+\\:]?=[ \t]*((?:.*\\\n)*.*)$/, "#{flag}=#{new_value}")
      @errors << "expected to change #{flag.inspect} to #{new_value.inspect}" if replaced.nil?
      replaced
    end
  end
end

def main
  load MISE_BREW_FORMULA_FILE.to_s
  klass = Formula.instance_variable_get(:@formula_subclass)
  odie "no Formula subclass found in #{MISE_BREW_FORMULA_FILE}" if klass.nil?
  formula = klass.new

  exit 0 if ENV["MISE_BREW_INSPECT_ONLY"] == "1"

  Dir.chdir(MISE_BREW_BUILDPATH.to_s)
  klass.patches.each(&:apply!)

  ohai "#{MISE_BREW_NAME}: running install"
  formula.prefix.mkpath
  formula.install

  # Post-install is a separate Homebrew lifecycle phase. Rust executes the
  # preflighted typed representation only after receipt, linking, and
  # install_etc_var; invoking the Ruby hook here would be unsandboxed, out of
  # order, and duplicate typed steps.

  if formula.prefix.children.empty?
    odie "install completed but the keg at #{formula.prefix} is empty"
  end
rescue ShimUnsupportedError => e
  odie e.message
rescue StandardError => e
  $stderr.puts e.backtrace.first(10).map { |l| "  #{l}" }.join("\n") if ENV["MISE_DEBUG"]
  odie "build failed: #{e.message}"
end

main
