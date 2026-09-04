# frozen_string_literal: true

# Extract Homebrew Cask metadata used by mise without loading Homebrew.

module JSON
  def self.generate(value)
    case value
    when Hash
      "{#{value.map { |key, item| "#{generate(key.to_s)}:#{generate(item)}" }.join(",")}}"
    when Array
      "[#{value.map { |item| generate(item) }.join(",")}]"
    when String
      '"' + value.each_codepoint.map { |codepoint|
        case codepoint
        when 0x08 then "\\b"
        when 0x09 then "\\t"
        when 0x0a then "\\n"
        when 0x0c then "\\f"
        when 0x0d then "\\r"
        when 0x22 then '\\"'
        when 0x5c then "\\\\"
        when 0...0x20 then format("\\u%04x", codepoint)
        else codepoint.chr(Encoding::UTF_8)
        end
      }.join + '"'
    when Numeric then value.to_s
    when true then "true"
    when false then "false"
    when nil then "null"
    else raise TypeError, "cannot encode #{value.class} as JSON"
    end
  end
end

CASK_FILE = ENV.fetch("MISE_BREW_CASK_FILE")
OUTPUT_FILE = ENV.fetch("MISE_BREW_METADATA_OUTPUT")

module OS
  def self.mac? = ENV.fetch("MISE_BREW_OS") == "macos"
  def self.linux? = ENV.fetch("MISE_BREW_OS") == "linux"
end

class MacOSVersion
  include Comparable

  SYMBOLS = {
    tahoe: "26", sequoia: "15", sonoma: "14", ventura: "13",
    monterey: "12", big_sur: "11", catalina: "10.15", mojave: "10.14",
    high_sierra: "10.13", sierra: "10.12", el_capitan: "10.11"
  }.freeze

  def self.host
    @host ||= new(ENV.fetch("MISE_BREW_MACOS_VERSION"))
  end

  def self.from_symbol(symbol) = new(SYMBOLS.fetch(symbol.to_sym))
  def initialize(version) = (@version = version.to_s)

  def <=>(other)
    other = self.class.from_symbol(other) if other.is_a?(Symbol)
    other = self.class.new(other.to_s) unless other.is_a?(MacOSVersion)
    version_parts <=> other.version_parts
  end

  protected

  def version_parts = @version.split(".").map(&:to_i)

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
    def self.arm? = ENV.fetch("MISE_BREW_ARCH").match?(/arm|aarch64/)
    def self.intel? = !arm?
    def self.arch = arm? ? :arm64 : :x86_64
  end
end

class Version
  def initialize(value) = @value = value.to_s
  def to_s = @value
  def to_str = @value
  def csv = @value.split(",").map { |part| self.class.new(part) }
  def before_comma = self.class.new(@value.split(",", 2).first)
  def dots_to_underscores = @value.tr(".", "_")
  def major = token(0)
  def minor = token(1)
  def patch = token(2)
  def major_minor = self.class.new(@value.split(".")[0, 2].join("."))
  def major_minor_patch = self.class.new(@value.split(".")[0, 3].join("."))

  private

  def token(index) = self.class.new(@value.split(".")[index].to_s)
end

class CaskMetadata
  attr_reader :token, :artifacts, :formula_dependencies, :cask_dependencies,
              :conflicting_casks

  def initialize(token)
    @token = token
    @artifacts = []
    @formula_dependencies = []
    @cask_dependencies = []
    @conflicting_casks = []
    @auto_updates = false
    @languages = {}
  end

  def version(value = nil)
    @version = Version.new(value) unless value.nil?
    @version
  end

  def arch(mapping = nil, **values)
    mapping = values if mapping.nil? && values.any?
    return @arch || Hardware::CPU.arch.to_s if mapping.nil?
    @arch = Hardware::CPU.arm? ? mapping[:arm] || mapping["arm"] : mapping[:intel] || mapping["intel"]
  end

  def on_arch_conditional(values)
    Hardware::CPU.arm? ? values[:arm] || values["arm"] : values[:intel] || values["intel"]
  end

  def sha256(value = nil, **values)
    value = Hardware::CPU.arm? ? values[:arm] : values[:intel] if value.nil? && values.any?
    @sha256 = value.to_s unless value.nil?
  end

  def url(value = nil, **) = (@url = value.to_s unless value.nil?)
  def auto_updates(value = nil) = (@auto_updates = value unless value.nil?)

  def depends_on(values = nil, **kwargs)
    values = kwargs if values.nil?
    return unless values.is_a?(Hash)
    @formula_dependencies.concat(Array(values[:formula] || values["formula"]).map(&:to_s))
    @cask_dependencies.concat(Array(values[:cask] || values["cask"]).map(&:to_s))
  end

  def conflicts_with(values = nil, **kwargs)
    values = kwargs if values.nil?
    return unless values.is_a?(Hash)
    @conflicting_casks.concat(Array(values[:cask] || values["cask"]).map(&:to_s))
  end

  def app(source, target: nil) = add_artifact("app", source, target)
  def binary(source, target: nil) = add_artifact("binary", source, target)
  def pkg(source, **) = add_artifact("pkg", source, nil)
  def font(source, target: nil) = add_artifact("font", source, target)
  def manpage(source, target: nil) = add_artifact("manpage", source, target)
  def bash_completion(source, target: nil) = add_artifact("bash_completion", source, target)
  def zsh_completion(source, target: nil) = add_artifact("zsh_completion", source, target)
  def fish_completion(source, target: nil) = add_artifact("fish_completion", source, target)

  def installer(**values) = @artifacts << { "installer" => values }
  def artifact(source, target: nil) = add_artifact("artifact", source, target)
  def uninstall(**values) = @artifacts << { "uninstall" => values }
  def zap(**values) = @artifacts << { "zap" => values }
  def preflight(*) = @artifacts << { "preflight" => nil }
  def postflight(*) = @artifacts << { "postflight" => nil }
  def uninstall_preflight(*) = @artifacts << { "uninstall_preflight" => nil }
  def uninstall_postflight(*) = @artifacts << { "uninstall_postflight" => nil }

  def language(code = nil, default: false, &block)
    if code
      @languages[code.to_s] = instance_eval(&block)
      @default_language = code.to_s if default
      return
    end
    @languages[@default_language] || @languages.values.first
  end

  def on_system_conditional(values)
    values[OS.mac? ? :macos : :linux] || values[OS.mac? ? "macos" : "linux"]
  end

  def on_arm(&block)
    instance_eval(&block) if Hardware::CPU.arm?
  end

  def on_intel(&block)
    instance_eval(&block) if Hardware::CPU.intel?
  end

  def on_macos(&block)
    instance_eval(&block) if OS.mac?
  end

  def on_linux(&block)
    instance_eval(&block) if OS.linux?
  end

  def on_system(*systems, macos: nil, &block)
    run = systems.include?(:linux) && OS.linux?
    run ||= OS.mac? && macos && macos_condition_matches?(macos)
    instance_eval(&block) if run && block
  end

  MacOSVersion::SYMBOLS.each_key do |release|
    define_method(:"on_#{release}") do |comparison = nil, &block|
      next unless OS.mac? && block
      host = MacOSVersion.host
      target = MacOSVersion.from_symbol(release)
      matches = case comparison
                when :or_older then host <= target || host.same_release?(target)
                when :or_newer then host >= target
                else host.same_release?(target)
                end
      instance_eval(&block) if matches
    end
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
    host = MacOSVersion.host
    target = MacOSVersion.from_symbol(base)
    case comparison
    when :or_older then host <= target || host.same_release?(target)
    when :or_newer then host >= target
    else host.same_release?(target)
    end
  end
  private :macos_condition_matches?

  def name(*) = nil
  def desc(*) = nil
  def homepage(*) = nil
  def livecheck(*) = nil
  def caveats(*) = nil
  def container(*) = nil
  def deprecate!(**) = nil
  def disable!(**) = nil
  def no_autobump!(*) = nil

  def to_h
    raise "cask has no version" if @version.nil?
    raise "cask has no URL" if @url.to_s.empty?
    {
      "token" => @token,
      "version" => @version.to_s,
      "auto_updates" => @auto_updates,
      "url" => @url,
      "sha256" => @sha256,
      "artifacts" => @artifacts,
      "depends_on" => {
        "formula" => @formula_dependencies,
        "cask" => @cask_dependencies
      },
      "conflicts_with" => { "cask" => @conflicting_casks },
      "ruby_source_path" => ENV.fetch("MISE_BREW_SOURCE_PATH"),
      "ruby_source_checksum" => { "sha256" => ENV.fetch("MISE_BREW_SOURCE_CHECKSUM") },
      "tap_git_head" => ENV.fetch("MISE_BREW_TAP_COMMIT")
    }
  end

  def method_missing(name, *, &block)
    raise "unsupported cask metadata DSL `#{name}`"
  end

  def respond_to_missing?(*) = true

  private

  def add_artifact(kind, source, target)
    value = [source.to_s]
    value << { "target" => target.to_s } unless target.nil?
    @artifacts << { kind => value }
  end
end

def cask(token, &block)
  $mise_cask_metadata = CaskMetadata.new(token)
  $mise_cask_metadata.instance_eval(&block)
end

eval(File.read(CASK_FILE), TOPLEVEL_BINDING, CASK_FILE, 1)
metadata = $mise_cask_metadata
raise "no cask block found" if metadata.nil?
expected = ENV.fetch("MISE_BREW_TOKEN")
raise "expected cask #{expected}, got #{metadata.token}" if metadata.token != expected
File.write(OUTPUT_FILE, JSON.generate(metadata.to_h))
