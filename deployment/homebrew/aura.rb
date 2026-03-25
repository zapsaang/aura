class Aura < Formula
  desc "AURA - Nanosecond-level system telemetry probe"
  homepage "https://github.com/zapsaang/aura"
  url "https://github.com/zapsaang/aura.git", tag: "v#{version}"
  license "MIT OR Apache-2.0"
  head "https://github.com/zapsaang/aura.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--workspace"
    bin.install "target/release/aura-daemon"
    bin.install "target/release/aura-cli"
  end

  def caveats
    <<~EOS
      AURA telemetry probe installed.

      Start the daemon:
        aura-daemon &

      Query telemetry:
        aura-cli -m cpu
        aura-cli -m mem
        aura-cli -m all

      Shared memory: /dev/shm/aura_state.dat
    EOS
  end

  plist_options startupreason: "AURA telemetry daemon"

  def plist
    <<~EOS
      <?xml version="1.0" encoding="UTF-8"?>
      <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
      <plist version="1.0">
      <dict>
        <key>Label</key>
        <string>#{plist_name}</string>
        <key>ProgramArguments</key>
        <array>
          <string>#{opt_bin}/aura-daemon</string>
        </array>
        <key>RunAtLoad</key>
        <true/>
        <key>KeepAlive</key>
        <dict>
          <key>SuccessfulExit</key>
          <false/>
        </dict>
        <key>StandardOutPath</key>
        <string>/tmp/aura-daemon.log</string>
        <key>StandardErrorPath</key>
        <string>/tmp/aura-daemon.log</string>
      </dict>
      </plist>
    EOS
  end

  test do
    assert_match "aura", shell_output("#{bin}/aura-cli --version 2>&1", 1)
  end
end
