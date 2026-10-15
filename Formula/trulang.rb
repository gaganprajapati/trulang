class Trulang < Formula
  desc "High-performance memory-safe backend language with dual-region memory arenas"
  homepage "https://github.com/gaganprajapati/trulang"
  version "0.1.0"
  license "MIT"

  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/gaganprajapati/trulang/releases/download/v0.1.0/trulang-darwin-arm64.tar.gz"
    # sha256 "REPLACE_WITH_SHA256_AFTER_RELEASE"
  elsif OS.mac? && Hardware::CPU.intel?
    url "https://github.com/gaganprajapati/trulang/releases/download/v0.1.0/trulang-darwin-x86_64.tar.gz"
    # sha256 "REPLACE_WITH_SHA256_AFTER_RELEASE"
  elsif OS.linux?
    url "https://github.com/gaganprajapati/trulang/releases/download/v0.1.0/trulang-linux-x86_64.tar.gz"
    # sha256 "REPLACE_WITH_SHA256_AFTER_RELEASE"
  end

  def install
    bin.install "trulang"
  end

  test do
    system "#{bin}/trulang", "--version"
  end
end
