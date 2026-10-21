class Trulang < Formula
  desc "High-performance backend language with dual-region memory arenas"
  homepage "https://gagn.dev/trulang/"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/gaganprajapati/trulang/releases/download/v1.0.0/trulang-darwin-arm64.tar.gz"
      sha256 "c8ea6987acab0f7389bbd8aa1b739bf0b89167ec0fa324c2be816dc99ca149c1"
    end
    on_intel do
      url "https://github.com/gaganprajapati/trulang/releases/download/v1.0.0/trulang-darwin-x86_64.tar.gz"
      sha256 "12628246a4dd091c6f148fedcc864ccda282fe689018c254bf58e191f17a77af"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/gaganprajapati/trulang/releases/download/v1.0.0/trulang-linux-x86_64.tar.gz"
      sha256 "6f3fc4aee58e0150385d58ca7f1de8fff8c3540caa6f7d7ea561c36b7ea7b380"
    end
    on_intel do
      url "https://github.com/gaganprajapati/trulang/releases/download/v1.0.0/trulang-linux-x86_64.tar.gz"
      sha256 "6f3fc4aee58e0150385d58ca7f1de8fff8c3540caa6f7d7ea561c36b7ea7b380"
    end
  end

  def install
    bin.install "trulang"
  end

  test do
    assert_match "trulang", shell_output("#{bin}/trulang --version")
  end
end
