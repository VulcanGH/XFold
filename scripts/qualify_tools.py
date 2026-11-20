"""Run Phase 0 Windows CLI qualification with an original two-second tone fixture.

Requires staging with --build plus independent ffprobe/ffmpeg on PATH.
All audio stays under ignored tools/fixtures; small diagnostic samples are retained.
"""
import hashlib
import json
from pathlib import Path
import shutil
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/evidence/phase0"
WORK = ROOT / "tools/fixtures"
HARLETTY = ROOT / "tools/bin/harletty/harletty.exe"
HZ = ROOT / "tools/bin/harlettizer/harlettizer.exe"
MKV = ROOT / "tools/bin/mkvtoolnix/mkvtoolnix"
XTASK = ROOT / "tools/build/harlettizer-target/x86_64-pc-windows-gnu/release/xtask.exe"


def run(name, command):
    args = [str(a) for a in command]
    started = time.monotonic()
    result = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)
    (OUT / f"{name}.stdout.txt").write_bytes(result.stdout)
    (OUT / f"{name}.stderr.txt").write_bytes(result.stderr)
    RECORD["commands"].append({"name": name, "argv": args, "exit_code": result.returncode,
                               "elapsed_seconds": round(time.monotonic() - started, 3)})
    if result.returncode:
        raise RuntimeError(f"{name} failed ({result.returncode}): {result.stderr.decode('utf-8', 'replace')[-1500:]}")
    return result.stdout.decode("utf-8")


def probe(name, path):
    data = json.loads(run(name, [FFPROBE, "-v", "error", "-show_streams", "-show_format", "-of", "json", path]))
    return data["streams"][0]


def check(condition, message):
    if not condition:
        raise RuntimeError(message)
    RECORD["passed"].append(message)


def main():
    global FFPROBE, RECORD
    OUT.mkdir(parents=True, exist_ok=True)
    WORK.mkdir(parents=True, exist_ok=True)
    RECORD = {"gate": "open", "commands": [], "passed": [],
              "fixture": "Original synthetic 2-second 48 kHz tones, four moving objects over a 7.1.2 master, encoded to standard-profile DTS:X 7.1.4 by pinned Harlettizer; no third-party media",
              "limitations": ["Synthetic standard-profile DTS:X only; commercial/dynamic-object source diversity untested",
                              "Receiver playback unavailable", "Clean-machine packaging untested",
                              "Distribution source and dependency-notice closure pending Phase 5"]}
    try:
        RECORD["binary_sha256"] = {}
        for exe in (HARLETTY, HZ, MKV / "mkvmerge.exe", MKV / "mkvextract.exe"):
            with exe.open("rb") as f:
                RECORD["binary_sha256"][exe.name] = hashlib.file_digest(f, "sha256").hexdigest()
        FFPROBE = shutil.which("ffprobe")
        ffmpeg = shutil.which("ffmpeg")
        check(bool(FFPROBE and ffmpeg), "Independent FFmpeg and ffprobe available")
        for name, exe in (("harletty", HARLETTY), ("harlettizer", HZ),
                          ("mkvmerge", MKV / "mkvmerge.exe"), ("mkvextract", MKV / "mkvextract.exe")):
            run(f"{name}-version", [exe, "--version"])
            run(f"{name}-help", [exe, "--help"])
        for cmd in ("encode", "dts"):
            run(f"harlettizer-{cmd}-help", [HZ, cmd, "--help"])
        run("ffprobe-version", [FFPROBE, "-version"])
        run("ffmpeg-version", [ffmpeg, "-version"])
        master = WORK / "synthetic.atmos"
        fixture = WORK / "synthetic-dtsx.dts"
        run("make-master", [XTASK, "make-master", master, "--objects", "4", "--bed", "7.1.2", "--seconds", "2"])
        run("make-dtsx", [HZ, "dts", master, "--out", fixture, "--layout", "7.1.4", "--bits", "24", "--progress"])
        run("source-info", [HARLETTY, "info", fixture, "--json"])
        source = probe("source-probe", fixture)
        check("DTS:X" in source.get("profile", ""), "Independent ffprobe identifies source as DTS:X")
        decoded = WORK / "decoded"
        run("decode-dtsx", [HARLETTY, "decode", fixture, "--output-path", decoded, "--bed-conform", "--log-format", "json"])
        recovered = WORK / "decoded.atmos"
        check(recovered.is_file(), "Harletty produces a master set")
        dts, thd = WORK / "export-7.1.dts", WORK / "export-spatial.thd"
        run("encode-dts", [HZ, "dts", recovered, "--out", dts, "--layout", "7.1", "--bits", "24", "--progress"])
        run("encode-truehd", [HZ, "encode", recovered, "--out", thd, "--cluster", "12", "--fold-depth", "24", "--progress"])
        for name, path, codec in (("dts", dts, "dts"), ("truehd", thd, "truehd")):
            stream = probe(f"{name}-probe", path)
            check(stream.get("codec_name") == codec, f"{name}: expected codec")
            check(int(stream.get("sample_rate", 0)) == 48000, f"{name}: 48 kHz")
            if name == "dts":
                check(stream.get("profile") == "DTS-HD MA", "DTS: Master Audio profile without DTS:X")
                check(stream.get("channels") == 8 and stream.get("channel_layout") == "7.1", "DTS: 7.1 layout")
                check(int(stream.get("bits_per_raw_sample", 0)) == 24, "DTS: 24-bit lossless layer")
            else:
                check("Atmos" in stream.get("profile", ""), "TrueHD: independent ffprobe identifies spatial profile")
                check(stream.get("channels") == 8 and stream.get("channel_layout") == "7.1", "TrueHD: 7.1 compatible presentation")
            raw = WORK / f"{name}-decoded.s32"
            run(f"{name}-independent-decode", [ffmpeg, "-v", "error", "-xerror", "-y", "-i", path,
                                               "-map", "0:a:0", "-c:a", "pcm_s32le", "-f", "s32le", raw])
            samples = raw.stat().st_size // (stream["channels"] * 4)
            duration = samples / 48000
            # Raw DTS has whole 512-sample frames and cannot carry trim padding.
            check(abs(duration - 2) <= 1024 / 48000, f"{name}: decoded duration within raw-frame padding tolerance")
            RECORD[f"{name}_decoded_duration"] = duration
            info = json.loads(run(f"{name}-harletty-info", [HARLETTY, "info", path, "--json"]))
            if name == "truehd":
                check(info.get("truehd", {}).get("atmos") is True and
                      info.get("spatial", {}).get("kind") == "atmos", "TrueHD: independent Harletty sees spatial metadata")
            # Tool-only container smoke check; no XFold container implementation.
            container = WORK / f"{name}-smoke.mka"
            if container.exists():
                container.unlink()
            run(f"{name}-mkvmerge", [MKV / "mkvmerge.exe", "-o", container, path])
            identified = json.loads(run(f"{name}-mkv-identify", [MKV / "mkvmerge.exe", "-J", container]))
            track = identified["tracks"][0]["id"]
            extracted = WORK / f"{name}-extracted{path.suffix}"
            run(f"{name}-mkvextract", [MKV / "mkvextract.exe", "tracks", container, f"{track}:{extracted}"])
            check(extracted.read_bytes() == path.read_bytes(), f"{name}: MKVToolNix mux/extract retains payload byte for byte")
        roundtrip = WORK / "truehd-roundtrip"
        run("truehd-spatial-decode", [HARLETTY, "decode", thd, "--output-path", roundtrip])
        config = roundtrip.with_suffix(".atmos").read_text()
        check("objects:" in config and len(re.findall(r"- ID:", config)) == 11,
              "TrueHD: independent decode recovers eleven spatial objects")
        RECORD["software_exports"] = "passed"
        RECORD["gate"] = "passed for synthetic standard-profile fixture"
        RECORD["limitations"].append("TrueHD protection field is unkeyed; receiver acceptance is unverified. No secret key is bundled.")
        RECORD["artifact_sha256"] = {p.name: hashlib.file_digest(p.open("rb"), "sha256").hexdigest()
                                       for p in (fixture, dts, thd)}
    except Exception as error:
        RECORD["blocker"] = str(error)
        raise
    finally:
        (OUT / "qualification.json").write_text(json.dumps(RECORD, indent=2) + "\n")


if __name__ == "__main__":
    main()
