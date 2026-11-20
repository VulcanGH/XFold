"""Phase 1 integration checks against xfold.exe and the staged Windows tools."""
import ctypes
from ctypes import wintypes
import hashlib
import json
import msvcrt
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
EXE = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else (
    ROOT / "target/x86_64-pc-windows-gnu/debug/xfold.exe"
)
FIXTURE = ROOT / "tools/fixtures/synthetic-dtsx.dts"
RESULTS = {}
WIN = ctypes.WinDLL("kernel32", use_last_error=True)


def invoke(args):
    return subprocess.run([str(part) for part in args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=60)


def expect(ok, message):
    if not ok:
        raise AssertionError(message)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def sparse_file(path, length):
    ioctl = WIN.DeviceIoControl
    ioctl.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD,
                      wintypes.LPVOID, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD),
                      wintypes.LPVOID]
    move = WIN.SetFilePointerEx
    move.argtypes = [wintypes.HANDLE, ctypes.c_longlong, ctypes.c_void_p, wintypes.DWORD]
    resize = WIN.SetEndOfFile
    resize.argtypes = [wintypes.HANDLE]
    with path.open("wb") as stream:
        handle = msvcrt.get_osfhandle(stream.fileno())
        if not ioctl(handle, 0x900C4, None, 0, None, 0,
                     ctypes.byref(wintypes.DWORD()), None):
            raise ctypes.WinError(ctypes.get_last_error())
        if not move(handle, length, None, 0) or not resize(handle):
            raise ctypes.WinError(ctypes.get_last_error())


def run(base):
    state = base / "state"
    audio = base / "audio ü"
    audio.mkdir()
    source = audio / "source 音.dts"
    shutil.copyfile(FIXTURE, source)
    original = digest(source)
    host = [EXE, "--state-dir", state]

    def convert(input_path, output, preset="dts-hd-ma"):
        return invoke([*host, "convert", preset, input_path, output])

    def status():
        result = invoke([*host, "status"])
        expect(result.returncode == 0, result.stderr)
        return json.loads(result.stdout)

    def clean():
        expect(not list(audio.glob(".xfold-*")), "attempt workspace remains")

    for preset, extension, codec, profile in (
        ("dts-hd-ma", "dts", "dts", "DTS-HD MA"),
        ("true-hd", "thd", "truehd", "Dolby TrueHD + Dolby Atmos"),
    ):
        output = audio / f"converted 音.{extension}"
        result = convert(source, output, preset)
        expect(result.returncode == 0, result.stderr)
        expect(status()["stage"] == "succeeded", "missing persisted success")
        if codec == "truehd":
            expect("unkeyed" in json.loads(result.stdout)["detail"], "missing protection diagnostic")
        probe = invoke(["ffprobe", "-v", "error", "-show_streams", "-of", "json", output])
        expect(probe.returncode == 0, probe.stderr)
        stream = json.loads(probe.stdout)["streams"][0]
        for key, expected in {"codec_name": codec, "profile": profile, "channels": 8,
                              "channel_layout": "7.1", "sample_rate": "48000",
                              "bits_per_raw_sample": "24"}.items():
            expect(stream[key] == expected, f"{preset} {key}: {stream[key]}")
        pcm = base / f"{preset}.s32"
        decode = invoke(["ffmpeg", "-v", "error", "-xerror", "-i", output, "-c:a",
                         "pcm_s32le", "-f", "s32le", pcm])
        expect(decode.returncode == 0, decode.stderr)
        samples = pcm.stat().st_size // 32
        expect(abs(samples - 96_000) <= 512, f"unexpected duration: {samples} samples")
        RESULTS[preset] = {"profile": profile, "channels": 8, "sample_rate": 48000,
                           "bits": 24, "decoded_samples": samples}
        clean()

    corrupt = audio / "corrupt.dts"
    corrupt.write_bytes(b"this is not a DTS stream")
    output = audio / "failed.dts"
    result = convert(corrupt, output)
    expect(result.returncode != 0 and "input" in result.stderr.lower(), result.stderr)
    expect(status()["stage"] == "failed" and not output.exists(), "corrupt input published")
    clean()
    RESULTS["corrupt_input"] = "failed with an input diagnostic and no output"

    existing = audio / "existing.dts"
    existing.write_bytes(b"keep me")
    result = convert(source, existing)
    expect(result.returncode != 0 and "already exists" in result.stderr, result.stderr)
    expect(existing.read_bytes() == b"keep me", "existing output modified")
    result = convert(source, source)
    expect(result.returncode != 0 and digest(source) == original, "source modified")
    clean()
    RESULTS["existing_destination"] = "existing files and source preserved"

    sparse = audio / "sparse.dts"
    sparse_file(sparse, shutil.disk_usage(audio).free // 8 + 64 * 1024 * 1024)
    result = convert(sparse, output)
    expect(result.returncode != 0 and "Insufficient space" in result.stderr, result.stderr)
    expect(not output.exists() and status()["stage"] == "failed", "disk shortage published output")
    sparse.unlink()
    clean()
    RESULTS["disk_preflight"] = "real destination free space rejected sparse input estimate"

    # A private group lets Ctrl+Break use the same handler as interactive Ctrl+C.
    processes = (wintypes.DWORD * 1)()
    if not WIN.GetConsoleProcessList(processes, len(processes)):
        expect(WIN.AllocConsole(), "could not allocate integration test console")
    long_input = audio / "long.dts"
    segment = source.read_bytes()
    with long_input.open("wb") as stream:
        for _ in range(96):
            stream.write(segment)

    def active_conversion(output, on_active, wait_stage):
        old_id = status()["id"]
        with subprocess.Popen([*host, "convert", "dts-hd-ma", long_input, output],
                              creationflags=subprocess.CREATE_NEW_PROCESS_GROUP,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as job:
            try:
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline and job.poll() is None:
                    pending = status()
                    if pending["id"] != old_id and pending["stage"] == wait_stage:
                        on_active(job)
                        return job.communicate(timeout=60)
                    time.sleep(0.02)
                raise AssertionError(f"did not observe {wait_stage}: {job.communicate(timeout=5)}, {status()}")
            finally:
                if job.poll() is None:
                    job.kill()

    def cancel(job):
        job.send_signal(signal.CTRL_BREAK_EVENT)

    _, stderr = active_conversion(output, cancel, "decode")
    expect(b"Cancelled" in stderr and status()["stage"] == "cancelled", stderr)
    expect(not output.exists(), "cancelled job published output")
    clean()
    RESULTS["cancellation"] = "console Ctrl+Break cancelled a running decoder and cleaned the workspace"

    _, stderr = active_conversion(output, lambda job: job.kill(), "decode")
    expect(status()["stage"] == "interrupted", f"restart status did not recover: {stderr}")
    expect(not output.exists(), "interrupted job published output")
    clean()
    RESULTS["restart"] = "forced host exit recovered as interrupted and cleaned the workspace"

    late = audio / "late.dts"

    def create_conflict(_):
        late.write_bytes(b"late conflict")
        other = convert(source, audio / "second.dts")
        expect(other.returncode != 0 and "Another job" in other.stderr, other.stderr)

    _, stderr = active_conversion(late, create_conflict, "encode")
    expect(b"Destination already exists" in stderr and late.read_bytes() == b"late conflict", stderr)
    expect(status()["stage"] == "failed", "late conflict was not persisted")
    clean()
    RESULTS["publication_race"] = "late conflict preserved and concurrent job declined"
    expect(digest(source) == original, "source changed")
    RESULTS["source_preservation"] = "SHA-256 unchanged"


if __name__ == "__main__":
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="phase1-", dir=ROOT / "target") as scratch:
        run(Path(scratch))
    print(json.dumps(RESULTS, indent=2))
