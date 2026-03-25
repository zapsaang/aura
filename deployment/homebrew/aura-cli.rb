# Documentation: https://docs.brew.sh/Formula-Cookbook
# PLEASE REMOVE ALL GENERATED COMMENTS BEFORE SUBMITTING
class AuraCli < Formula
  desc "AURA CLI - nanosecond-level system telemetry consumer"
  homepage "https://github.com/zapsaang/aura"
  url "https://github.com/zapsaang/aura.git", tag: "v#{version}"
  license "MIT OR Apache-2.0"
  head "https://github.com/zapsaang/aura.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--package", "aura-cli"
    bin.install "target/release/aura-cli"
  end

  def caveats
    <<~EOS
      aura-cli reads telemetry from shared memory.

      Start the daemon first:
        aura-daemon &

      Then query telemetry:
        aura-cli -m cpu
        aura-cli -m mem
        aura-cli -m all

      Shared memory path: /dev/shm/aura_state.dat
    EOS
  end

  test do
    assert_match "aura-cli", shell_output("#{bin}/aura-cli --version 2>&1", 1)
  end
end
