# frozen_string_literal: true

# Extract the subset of Homebrew Formula metadata mise needs to build a
# third-party formula from source. This does not load or invoke Homebrew.

require "json"

FORMULA_FILE = ENV.fetch("MISE_BREW_FORMULA_FILE")
OUTPUT_FILE = ENV.fetch("MISE_BREW_METADATA_OUTPUT")
FORMULA_NAME = ENV.fetch("MISE_BREW_NAME")

module OS
  def self.mac? = RUBY_PLATFORM.include?("darwin")
  def self.linux? = RUBY_PLATFORM.include?("linux")
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
      return if kind == :test || Array(kind).all? { |value| value == :test }
      target = kind == :build || Array(kind).include?(:build) ? @build_dependencies : @runtime_dependencies
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
      class_exec(&block) if block && (systems.include?(:linux) && OS.linux? || macos && OS.mac?)
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
