#!/usr/bin/env python3
"""Native smoke test for a packaged aura release archive.

Verifies the archive digests, extracts it no-follow into a mode-0700 temp
dir, runs the packaged daemon against a private mode-0700 runtime directory,
polls the packaged CLI until ready, exercises every format/color contract,
then proves clean SIGTERM shutdown and the exact offline transition. All
artifacts are written only beneath --evidence; stdout is exactly
{"checks":1,"status":"ok"}.
"""
import argparse
import ctypes.util
import hashlib
import json
import os
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time

sys.dont_write_bytecode = True

BINARIES = ("aura-cli", "aura-daemon")
SUMS_NAME = "SHA256SUMS"
OFFLINE_LINE = b"[AURA: OFFLINE]\n"
DIMENSIONS = ("cpu", "process", "memory", "storage", "network", "meta", "gpu")
FORMATS = ("human", "json", "value")
COLORS = ("none", "ansi", "tmux", "zellij")


class SmokeError(RuntimeError):
    pass


def _write(evidence: str, name: str, data: bytes) -> None:
    path = os.path.join(evidence, name)
    with open(path, "wb") as handle:
        handle.write(data)


def _extract(archive: str, dest: str, evidence: str) -> dict[str, str]:
    with open(archive, "rb") as handle:
        archive_bytes = handle.read()
    archive_sha = hashlib.sha256(archive_bytes).hexdigest()
    _write(evidence, "archive-sha256.txt", f"{archive_sha}\n".encode("ascii"))
    try:
        with tarfile.open(archive, "r:gz") as tar:
            members = {member.name: member for member in tar.getmembers()}
            payloads: dict[str, bytes] = {}
            for name in (*BINARIES, SUMS_NAME):
                member = members.get(name)
                if member is None or not member.isreg():
                    raise SmokeError(f"archive lacks regular member {name}")
                if member.name.startswith("/") or ".." in member.name.split("/"):
                    raise SmokeError(f"unsafe member path {member.name}")
                extracted = tar.extractfile(member)
                if extracted is None:
                    raise SmokeError(f"cannot extract member {name}")
                payloads[name] = extracted.read()
    except (OSError, tarfile.TarError) as error:
        raise SmokeError(f"cannot read archive {archive}: {error}") from error

    expected = {}
    for line in payloads[SUMS_NAME].decode("ascii").splitlines():
        digest, _, name = line.partition("  ")
        expected[name] = digest
    for name in BINARIES:
        actual = hashlib.sha256(payloads[name]).hexdigest()
        if expected.get(name) != actual:
            raise SmokeError(f"digest mismatch for member {name}")

    for name in BINARIES:
        path = os.path.join(dest, name)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o700)
        with os.fdopen(fd, "wb") as handle:
            handle.write(payloads[name])
    return {"archive_sha256": archive_sha}


def _run_cli(binary: str, state: str, extra: list[str]) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [binary, "--shm-path", state, *extra],
        check=False,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def _require_ready(payload: bytes, platform: str) -> dict:
    try:
        data = json.loads(payload)
    except json.JSONDecodeError as error:
        raise SmokeError(f"readiness output is not valid JSON: {error}") from error
    if not isinstance(data, dict):
        raise SmokeError("readiness JSON is not an object")
    if data.get("version") != 2:
        raise SmokeError(f"expected JSON version 2, got {data.get('version')!r}")
    for dimension in DIMENSIONS:
        if not isinstance(data.get(dimension), dict):
            raise SmokeError(f"missing or invalid dimension {dimension!r}")
    capabilities = data.get("capabilities")
    if not isinstance(capabilities, dict) or len(capabilities) != 33:
        raise SmokeError("capabilities object must hold exactly 33 booleans")
    if not all(isinstance(value, bool) for value in capabilities.values()):
        raise SmokeError("capabilities values must be booleans")
    if capabilities.get("gpu_enumeration") is not False:
        raise SmokeError("gpu_enumeration capability must be clear on this host")
    if platform == "linux":
        process = data["process"]
        if capabilities.get("process_total") is not True or not process.get("total", 0) > 0:
            raise SmokeError("linux readiness requires a populated Process dimension")
    else:
        for capability in ("process_total", "process_running", "process_blocked",
                           "process_sleeping", "process_top_cpu", "process_top_memory"):
            if capabilities.get(capability) is not False:
                raise SmokeError(f"macos capability {capability} must be clear")
        for dimension in ("cpu", "memory", "storage", "network", "meta"):
            if not data[dimension]:
                raise SmokeError(f"macos dimension {dimension} must be populated")
    return data


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", required=True)
    parser.add_argument("--platform", choices=("linux", "macos"), required=True)
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()

    os.makedirs(args.evidence, exist_ok=True)
    timings: dict[str, float] = {"start": time.monotonic()}
    daemon: subprocess.Popen[bytes] | None = None
    try:
        host = {"linux": "linux", "macos": "darwin"}[args.platform]
        if not sys.platform.startswith(host):
            raise SmokeError(
                f"--platform {args.platform} cannot run on host {sys.platform}"
            )
        if args.platform == "linux" and ctypes.util.find_library("nvidia-ml") is not None:
            raise SmokeError("linux smoke requires nvidia-ml to be absent")

        temp_dir = tempfile.mkdtemp(prefix="aura-smoke-")
        os.chmod(temp_dir, 0o700)
        runtime_dir = tempfile.mkdtemp(prefix="aura-smoke-runtime-")
        os.chmod(runtime_dir, 0o700)
        mode = stat.S_IMODE(os.stat(runtime_dir).st_mode)
        _write(args.evidence, "temp-path-mode.txt", f"{mode:04o}\n".encode("ascii"))
        state = os.path.join(runtime_dir, "state.dat")

        identity = _extract(args.archive, temp_dir, args.evidence)
        daemon_bin = os.path.join(temp_dir, "aura-daemon")
        cli_bin = os.path.join(temp_dir, "aura-cli")

        env = {
            "HOME": temp_dir,
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "TMPDIR": temp_dir,
            "RUST_LOG": "off",
        }
        daemon = subprocess.Popen(
            [daemon_bin, "--shm-path", state, "--heartbeat-ms", "100"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
        )

        deadline = time.monotonic() + 5.0
        ready: dict | None = None
        last_exit: int | None = None
        while time.monotonic() < deadline and ready is None:
            if daemon.poll() is not None:
                raise SmokeError(f"daemon exited early with {daemon.returncode}")
            probe = _run_cli(cli_bin, state, ["--format", "json", "-m", "all", "--color", "none"])
            last_exit = probe.returncode
            if probe.returncode == 0 and probe.stderr == b"":
                try:
                    ready = _require_ready(probe.stdout, args.platform)
                except SmokeError:
                    ready = None
            time.sleep(0.1)
        if ready is None:
            raise SmokeError(f"daemon never became ready (last cli exit {last_exit})")
        timings["ready_after"] = time.monotonic() - timings["start"]
        _write(
            args.evidence,
            "readiness.json",
            json.dumps({"capabilities": ready["capabilities"]}, sort_keys=True, indent=2).encode()
            + b"\n",
        )

        contract_rows = []
        for output_format in FORMATS:
            for color in COLORS:
                result = _run_cli(
                    cli_bin,
                    state,
                    ["--format", output_format, "-m", "all", "--color", color],
                )
                row = {
                    "format": output_format,
                    "color": color,
                    "exit": result.returncode,
                    "stderr_empty": result.stderr == b"",
                }
                if result.returncode != 0 or result.stderr != b"":
                    raise SmokeError(f"cli contract failed: {row!r}")
                if output_format == "json":
                    json.loads(result.stdout)
                contract_rows.append(row)
        _write(
            args.evidence,
            "cli-contract.json",
            json.dumps(contract_rows, indent=2).encode() + b"\n",
        )

        daemon.send_signal(signal.SIGTERM)
        try:
            out, err = daemon.communicate(timeout=3.0)
        except subprocess.TimeoutExpired as error:
            daemon.kill()
            daemon.communicate()
            raise SmokeError("daemon did not exit within 3s of SIGTERM") from error
        timings["shutdown"] = time.monotonic() - timings["start"]
        exit_code = daemon.returncode
        daemon = None
        _write(args.evidence, "daemon.stdout", out)
        _write(args.evidence, "daemon.stderr", err)
        _write(args.evidence, "daemon-exit-code", f"{exit_code}\n".encode("ascii"))
        if exit_code != 0:
            raise SmokeError(f"daemon exited {exit_code} after SIGTERM, expected 0")
        if out != b"" or err != b"":
            raise SmokeError("daemon produced output on stdout/stderr")

        time.sleep(2.1)
        offline = _run_cli(cli_bin, state, ["--format", "json", "-m", "all", "--color", "none"])
        _write(args.evidence, "offline.stdout", offline.stdout)
        _write(args.evidence, "offline.stderr", offline.stderr)
        if offline.returncode != 1 or offline.stderr != b"" or offline.stdout != OFFLINE_LINE:
            raise SmokeError(
                f"offline contract failed: exit {offline.returncode},"
                f" stdout {offline.stdout!r}, stderr {offline.stderr!r}"
            )
        timings["offline_after"] = time.monotonic() - timings["start"]
        _write(
            args.evidence,
            "timings.json",
            json.dumps({**identity, **timings}, sort_keys=True, indent=2).encode() + b"\n",
        )
    except (OSError, SmokeError, json.JSONDecodeError) as error:
        if daemon is not None and daemon.poll() is None:
            daemon.kill()
            daemon.communicate()
        sys.stderr.write(f"smoke-release: {error}\n")
        return 1

    sys.stdout.write('{"checks":1,"status":"ok"}\n')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
