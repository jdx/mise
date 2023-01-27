class Rtx < Formula
  desc "Multi-language runtime manager"
  homepage "https://github.com/jdxcode/rtx"
  license "MIT"
  version "$RTX_VERSION"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/jdxcode/rtx/releases/download/v$RTX_VERSION/rtx-v$RTX_VERSION-macos-x64.tar.xz"
      sha256 "$RTX_CHECKSUM_MACOS_X86_64"

      def install
        bin.install "bin/rtx"
      end
    end
    if Hardware::CPU.arm?
      url "https://github.com/jdxcode/rtx/releases/download/v$RTX_VERSION/rtx-v$RTX_VERSION-macos-arm64.tar.xz"
      sha256 "$RTX_CHECKSUM_MACOS_ARM64"

      def install
        bin.install "bin/rtx"
      end
    end
  end

  on_linux do
    if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?
      url "https://github.com/jdxcode/rtx/releases/download/v$RTX_VERSION/rtx-v$RTX_VERSION-linux-arm64.tar.xz"
      sha256 "$RTX_CHECKSUM_LINUX_ARM64"

      def install
        bin.install "bin/rtx"
      end
    end
    if Hardware::CPU.intel?
      url "https://github.com/jdxcode/rtx/releases/download/v$RTX_VERSION/rtx-v$RTX_VERSION-linux-x64.tar.xz"
      sha256 "$RTX_CHECKSUM_LINUX_X86_64"

      def install
        bin.install "bin/rtx"
      end
    end
  end

  test do
    system "#{bin}/rtx --version"
    assert_match "it works!", shell_output("#{bin}/rtx exec nodejs@18 -- node -p 'it works!'")
  end
end
