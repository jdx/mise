class Rtx < Formula
  desc "Multi-language runtime manager"
  homepage "https://github.com/jdxcode/rtx"
  license "MIT"
  version "$RTX_VERSION"

  on_macos do
    if Hardware::CPU.intel?
      url "https://rtx.pub/v$RTX_VERSION/rtx-brew-v$RTX_VERSION-macos-x64.tar.xz"
      sha256 "$RTX_CHECKSUM_MACOS_X86_64"
    end
    if Hardware::CPU.arm?
      url "https://rtx.pub/v$RTX_VERSION/rtx-brew-v$RTX_VERSION-macos-arm64.tar.xz"
      sha256 "$RTX_CHECKSUM_MACOS_ARM64"
    end
  end

  on_linux do
    if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?
      url "https://rtx.pub/v$RTX_VERSION/rtx-brew-v$RTX_VERSION-linux-arm64.tar.xz"
      sha256 "$RTX_CHECKSUM_LINUX_ARM64"
    end
    if Hardware::CPU.intel?
      url "https://rtx.pub/v$RTX_VERSION/rtx-brew-v$RTX_VERSION-linux-x64.tar.xz"
      sha256 "$RTX_CHECKSUM_LINUX_X86_64"
    end
  end

  def install
    bin.install "bin/rtx"
    man1.install "man/man1/rtx.1"
    generate_completions_from_executable(bin/"rtx", "complete", "--shell")
  end

  test do
    system "#{bin}/rtx --version"
    system "#{bin}/rtx", "install", "nodejs@18.13.0"
    assert_match "v18.13.0", shell_output("#{bin}/rtx exec nodejs@18.13.0 -- node -v")
  end
end
