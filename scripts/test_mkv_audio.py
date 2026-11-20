"""Focused Phase 3 checks with a synthetic, multi-track Matroska fixture."""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target/x86_64-pc-windows-gnu"
EXE = TARGET / "release/xfold.exe"
MKV = ROOT / "tools/bin/mkvtoolnix/mkvtoolnix"
HARLETTY = ROOT / "tools/bin/harletty/harletty.exe"
FIXTURES = ROOT / "tools/fixtures"


def invoke(*args):
    return subprocess.run([str(arg) for arg in args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=60)


def checked(*args):
    result = invoke(*args)
    assert result.returncode == 0, f"{args}: {result.stdout}\n{result.stderr}"
    return result.stdout


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def make_fixture(root):
    video = root / "test video.mkv"
    checked("ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=size=64x64:rate=24",
            "-t", "2", "-c:v", "ffv1", video)
    source = root / "源 multi-track ü.mkv"
    checked(MKV / "mkvmerge.exe", "-o", source, video,
            "--track-name", "0:Plain DTS", FIXTURES / "export-7.1.dts",
            "--track-name", "0:TrueHD decoy", FIXTURES / "export-spatial.thd",
            "--language", "0:jpn", "--track-name", "0:空間 DTS:X", FIXTURES / "synthetic-dtsx.dts")
    return source


def verify_cli(root, source):
    host = (EXE, "--state-dir", root / "state")
    source_hash = digest(source)
    tracks = json.loads(checked(*host, "tracks", source))
    assert len(tracks) == 3 and all(track["type"] == "audio" for track in tracks)
    by_name = {track["properties"]["track_name"]: track for track in tracks}
    chosen = by_name["空間 DTS:X"]
    assert chosen["properties"]["language_ietf"] == "ja"
    track_id = chosen["id"]
    assert track_id != tracks[0]["id"] and track_id != chosen["properties"]["number"]

    invalid = root / "rejected.dts"
    for id, diagnostic in (
        (None, "--track ID"),
        (9999, "absent"),
        (0, "Select a DTS:X audio track"),
        (by_name["TrueHD decoy"]["id"], "Select a DTS:X audio track"),
        (by_name["Plain DTS"]["id"], "not recognized as 48 kHz DTS:X"),
    ):
        selection = () if id is None else ("--track", id)
        error = invoke(*host, "convert", "dts-hd-ma", *selection, source, invalid)
        assert error.returncode and diagnostic in error.stderr, error.stderr
        assert not invalid.exists()

    extracted = root / "extracted 源.dts"
    checked(MKV / "mkvextract.exe", source, "tracks", f"{track_id}:{extracted}")
    assert digest(extracted) == digest(FIXTURES / "synthetic-dtsx.dts")
    assert json.loads(checked(HARLETTY, "info", extracted, "--json"))["spatial"]["kind"] == "dtsx"

    for preset, extension, profile, matroska_codec in (
        ("dts-hd-ma", "dts", "DTS-HD MA", "A_DTS"),
        ("true-hd", "thd", "Dolby TrueHD + Dolby Atmos", "A_TRUEHD"),
    ):
        raw = root / f"raw 音.{extension}"
        mka = root / f"{preset} 音.mka"
        for output in (raw, mka):
            attempt = json.loads(checked(*host, "convert", preset, "--track", track_id, source, output))
            assert attempt["stage"] == "succeeded" and attempt["track_id"] == track_id
            probe = json.loads(checked("ffprobe", "-v", "error", "-show_streams", "-of", "json", output))
            stream, = probe["streams"]
            assert stream["profile"] == profile and stream["channel_layout"] == "7.1"
            assert stream["sample_rate"] == "48000" and stream["bits_per_raw_sample"] == "24"
        report = json.loads(checked(MKV / "mkvmerge.exe", "-J", mka))
        export_track, = report["tracks"]
        assert export_track["properties"]["codec_id"] == matroska_codec
        roundtrip = root / f"roundtrip 音.{extension}"
        checked(MKV / "mkvextract.exe", mka, "tracks", f'{export_track["id"]}:{roundtrip}')
        assert digest(raw) == digest(roundtrip)

    assert digest(source) == source_hash
    assert not list(root.glob(".xfold-*"))
    return track_id


def verify_browser(root, source, track_id, test_binary):
    from playwright.sync_api import expect, sync_playwright
    from test_web_ui import host, verify_boundaries

    edge = os.environ.get("XFOLD_TEST_BROWSER",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
    env = {**os.environ, "XFOLD_BROWSER_TEST_ROOT": str(root), "XFOLD_BROWSER_TEST_SOURCE": source.name}
    args = [test_binary, "--ignored", "--exact", "web::tests::browser_fixture", "--nocapture"]
    with host(args, env=env) as url, sync_playwright() as playwright:
        verify_boundaries(url)
        browser = playwright.chromium.launch(executable_path=edge, headless=True)
        page = browser.new_page()
        page.goto(url)
        page.get_by_role("button", name="Choose file").click()
        picker = page.get_by_label("MKV audio track")
        expect(picker).to_be_enabled()
        assert picker.locator("option", has_text="TrueHD decoy").is_disabled()
        start = page.get_by_role("button", name="Start conversion")
        expect(start).to_be_disabled()
        picker.select_option(str(track_id))
        fmt = page.get_by_label("Output format")
        expect(fmt).to_be_enabled()
        fmt.select_option("mka")
        save = page.get_by_role("button", name="Save as")
        expect(save).to_be_enabled()
        save.click()
        expect(page.locator("#output-path")).to_contain_text("export.mka")
        page.reload()
        expect(picker).to_have_value(str(track_id))
        expect(fmt).to_have_value("mka")
        start.click()
        expect(page.get_by_role("status")).to_contain_text("Complete", timeout=20_000)
        assert (root / "export.mka").exists()
        browser.close()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="phase3-", dir=ROOT / "target") as directory:
        root = Path(directory)
        source = make_fixture(root)
        track_id = verify_cli(root, source)
        print("Passed: explicit track selection, rejection cases, bit-identical DTS:X extraction, four standalone exports and source preservation.")
        if len(sys.argv) > 1:
            verify_browser(root, source, track_id, Path(sys.argv[1]).resolve())
            print("Passed: authenticated browser track selection, MKA selection, reload and export.")
