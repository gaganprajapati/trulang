class Trulang < Formula
  desc "High-performance backend language with dual-region memory arenas"
  homepage "https://trulang.dev"
  license "MIT"

  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/gaganprajapati/trulang/releases/download/v0.1.0/trulang-darwin-arm64.tar.gz"
    sha256 "2e150238c9adacb73f6e45921cbc079372e9fc81daac8011fb5a0e032482c567"
  elsif OS.mac? && Hardware::CPU.intel?
    url "https://github.com/gaganprajapati/trulang/releases/download/v0.1.0/trulang-darwin-x86_64.tar.gz"
    sha256 "7297f70a703bb6f4c880f457aac91d7a4f1949420b9a188bb95d3f72c9a98fb6"
  elsif OS.linux?
    url "https://github.com/gaganprajapati/trulang/releases/download/v0.1.0/trulang-linux-x86_64.tar.gz"
    sha256 "6761caa7ade1021668c9fb2369c6cce013f4e751436fb1fe96e7b85e07bbd941"
  end

  def install
    bin.install "trulang"
  end

  test do
    assert_match "trulang", shell_output("#{bin}/trulang --version")
  end
end
