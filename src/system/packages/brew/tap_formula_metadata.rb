# frozen_string_literal: true

# Extract the subset of Homebrew Formula metadata mise needs to build a
# third-party formula from source. This does not load or invoke Homebrew.

require "json"
require "rbconfig"
require "rubygems/version"

FORMULA_FILE = ENV.fetch("MISE_BREW_FORMULA_FILE")
OUTPUT_FILE = ENV.fetch("MISE_BREW_METADATA_OUTPUT")
FORMULA_NAME = ENV.fetch("MISE_BREW_NAME")

module OS
  def self.mac? = RbConfig::CONFIG["host_os"].include?("darwin")
  def self.linux? = RbConfig::CONFIG["host_os"].include?("linux")
end

class MacOSVersion
  include Comparable

  SYMBOLS = {
    tahoe: "26", sequoia: "15", sonoma: "14", ventura: "13",
    monterey: "12", big_sur: "11", catalina: "10.15", mojave: "10.14",
    high_sierra: "10.13", sierra: "10.12", el_capitan: "10.11"
  }.freeze

  def self.host
    @host ||= new(OS.mac? ? `sw_vers -productVersion`.strip : "0")
  end

  def self.from_symbol(symbol) = new(SYMBOLS.fetch(symbol.to_sym))
  def initialize(version) = (@version = version.to_s)

  def <=>(other)
    other = self.class.from_symbol(other) if other.is_a?(Symbol)
    other = self.class.new(other.to_s) unless other.is_a?(MacOSVersion)
    Gem::Version.new(@version) <=> Gem::Version.new(other.to_s)
  end

  def same_release?(other)
    other = self.class.from_symbol(other) if other.is_a?(Symbol)
    other = self.class.new(other.to_s) unless other.is_a?(MacOSVersion)
    mine = @version.split(".").map(&:to_i)
    theirs = other.to_s.split(".").map(&:to_i)
    mine[0] == theirs[0] && (mine[0] >= 11 || mine[1] == theirs[1])
  end

  def to_s = @version
end

module Hardware
  module CPU
    def self.arm? = RUBY_PLATFORM.match?(/arm|aarch64/)
    def self.intel? = !arm?
    def self.arch = arm? ? :arm64 : :x86_64
  end
end

class MetadataVersion
  def initialize(value) = @value = value.to_s
  def to_s = @value
  def to_str = @value
  def csv = @value.split(",").map { |part| self.class.new(part) }
  def before_comma = self.class.new(@value.split(",", 2).first)
  def dots_to_underscores = @value.tr(".", "_")
  def major = self.class.new(@value.split(".")[0])
  def minor = self.class.new(@value.split(".")[1])
  def patch = self.class.new(@value.split(".")[2])
  def major_minor = self.class.new(@value.split(".")[0, 2].join("."))
  def major_minor_patch = self.class.new(@value.split(".")[0, 3].join("."))
end

class Formula
  class << self
    attr_reader :source_url, :source_sha256, :explicit_version, :revision_value,
                :runtime_dependencies, :build_dependencies, :keg_only_value

    def inherited(subclass)
      Formula.instance_variable_set(:@subclass, subclass)
      subclass.instance_variable_set(:@runtime_dependencies, [])
      subclass.instance_variable_set(:@build_dependencies, [])
      subclass.instance_variable_set(:@revision_value, 0)
      subclass.instance_variable_set(:@keg_only_value, false)
    end

    def url(value = nil, **) = (@source_url = value unless value.nil?)
    def mirror(*) = nil
    def sha256(value = nil, **) = (@source_sha256 = value if value.is_a?(String))
    def version(value = nil) = (@explicit_version = value.to_s unless value.nil?)
    def revision(value = nil) = (@revision_value = value.to_i unless value.nil?)
    def keg_only(*) = (@keg_only_value = true)

    def depends_on(spec = nil, **)
      name, kind = spec.is_a?(Hash) ? spec.first : [spec, nil]
      return if name.nil?
      kinds = Array(kind)
      return if kind == :test || (!kinds.empty? && kinds.all? { |value| value == :test })
      target = kind == :build || kinds.include?(:build) ? @build_dependencies : @runtime_dependencies
      target << name.to_s
    end

    def uses_from_macos(spec = nil, **)
      depends_on(spec) if OS.linux?
    end

    def stable(&block)
      class_exec(&block) if block
    end

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
    def on_system(*systems, macos: nil, &block)
      run = systems.include?(:linux) && OS.linux?
      run ||= OS.mac? && macos && macos_condition_matches?(macos)
      class_exec(&block) if run && block
    end

    def macos_condition_matches?(condition)
      value = condition.to_s
      base, comparison = if value.end_with?("_or_older")
        [value.delete_suffix("_or_older"), :or_older]
      elsif value.end_with?("_or_newer")
        [value.delete_suffix("_or_newer"), :or_newer]
      else
        [value, :==]
      end
      target = MacOSVersion.from_symbol(base)
      host = MacOSVersion.host
      case comparison
      when :or_older then host <= target
      when :or_newer then host >= target
      else host.same_release?(target)
      end
    end
    private :macos_condition_matches?

    MacOSVersion::SYMBOLS.each_key do |release|
      define_method(:"on_#{release}") do |comparison = nil, &block|
        next unless OS.mac? && block
        host = MacOSVersion.host
        target = MacOSVersion.from_symbol(release)
        matches = case comparison
                  when :or_older then host <= target
                  when :or_newer then host >= target
                  else host.same_release?(target)
                  end
        class_exec(&block) if matches
      end
    end

    def resource(*) = nil
    def patch(*) = nil
    def bottle(*) = nil
    def head(*) = nil
    def livecheck(*) = nil
    def service(*) = nil
    def test(*) = nil
    def method_missing(*) = nil
    def respond_to_missing?(*) = true
  end
end

def inferred_version(url)
  basename = File.basename(url.to_s).sub(/\.(tar\.(gz|xz|bz2|zst)|tgz|txz|zip|gz)\z/i, "")
  match = basename.match(/(?:^|[-_v])([0-9]+(?:\.[0-9A-Za-z]+)+(?:[-_.][0-9A-Za-z]+)*)/)
  match && match[1]
end

load FORMULA_FILE
klass = Formula.instance_variable_get(:@subclass)
raise "no Formula subclass found" unless klass
raise "formula has no stable URL" if klass.source_url.to_s.empty?
raise "formula has no stable sha256" if klass.source_sha256.to_s.empty?
version = klass.explicit_version || inferred_version(klass.source_url)
raise "could not infer formula version; add an explicit version declaration" if version.to_s.empty?

metadata = {
  "name" => FORMULA_NAME,
  "tap" => ENV.fetch("MISE_BREW_TAP"),
  "versions" => { "stable" => version },
  "revision" => klass.revision_value || 0,
  "keg_only" => klass.keg_only_value || false,
  "dependencies" => klass.runtime_dependencies || [],
  "build_dependencies" => klass.build_dependencies || [],
  "bottle" => {},
  "urls" => { "stable" => { "url" => klass.source_url, "checksum" => klass.source_sha256 } },
  "ruby_source_path" => ENV.fetch("MISE_BREW_SOURCE_PATH"),
  "ruby_source_checksum" => { "sha256" => ENV.fetch("MISE_BREW_SOURCE_CHECKSUM") },
  "tap_git_head" => ENV.fetch("MISE_BREW_TAP_COMMIT")
}
File.write(OUTPUT_FILE, JSON.generate(metadata))
