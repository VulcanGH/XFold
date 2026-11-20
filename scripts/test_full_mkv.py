"""Validate add/replace on rights-cleared Matroska media with a 125 ms audio offset."""
from pathlib import Path
import argparse
import json
import os
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as XML

from test_mkv_audio import ROOT, FIXTURES, MKV, digest

DEFAULT_EXE = ROOT / "target/finish-build/x86_64-pc-windows-gnu/release/xfold.exe"


def invoke(*args):
    return subprocess.run([str(arg) for arg in args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=1200)


def checked(*args):
    result = invoke(*args)
    assert result.returncode == 0, (args, result.stdout, result.stderr)
    return result.stdout


def identify(source):
    return json.loads(checked(MKV / "mkvmerge.exe", "-J", source))


def fixture(root, minutes):
    audio = FIXTURES / "synthetic-dtsx.dts"
    repeats = max(1, minutes * 60 // 2)
    if repeats > 1:
        audio = root / "long spatial.dts"
        packet_data = (FIXTURES / "synthetic-dtsx.dts").read_bytes()
        with audio.open("wb") as out:
            for _ in range(repeats):
                out.write(packet_data)
    video = root / "video.mkv"
    seconds = 2.00533 * repeats + 0.125
    checked("ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=size=32x32:rate=5",
            "-t", seconds, "-c:v", "ffv1", video)
    subtitle = root / "subtitle.srt"
    subtitle.write_text("1\n00:00:00,000 --> 00:00:01,000\nXFOLD fixture\n", encoding="utf-8")
    attachment = root / "note.txt"
    attachment.write_text("Original XFold test data.", encoding="utf-8")
    chapters = root / "chapters.txt"
    chapters.write_text("CHAPTER01=00:00:00.000\nCHAPTER01NAME=Opening\n", encoding="utf-8")
    tags = root / "tags.xml"
    tags.write_text('<Tags><Tag><Targets/><Simple><Name>COMMENT</Name><String>XFold fixture</String></Simple></Tag></Tags>',
                    encoding="utf-8")
    retained_tags = root / "retained-tags.xml"
    retained_tags.write_text('<Tags><Tag><Targets/><Simple><Name>TITLE</Name><String>Retained subtitle tag</String></Simple></Tag></Tags>',
                             encoding="utf-8")
    source = root / "Test 源.mkv"
    checked(MKV / "mkvmerge.exe", "--disable-track-statistics-tags", "-o", source,
            "--chapters", chapters, "--global-tags", tags,
            "--attachment-mime-type", "text/plain", "--attach-file", attachment,
            video, "--sync", "0:125", "--language", "0:jpn", "--track-name", "0:Spatial",
            "--default-track-flag", "0:1", "--original-flag", "0:0",
            "--commentary-flag", "0:1", audio, "--tags", f"0:{retained_tags}", subtitle)
    return source, seconds


def dump_xml(source, mode, path):
    checked(MKV / "mkvextract.exe", source, mode, path)
    return sorted(XML.canonicalize(XML.tostring(item, encoding="unicode"), strip_text=True)
                  for item in XML.parse(path).getroot())


def signature(source, index):
    return checked("ffmpeg", "-v", "error", "-i", source, "-map", f"0:{index}",
                   "-c", "copy", "-f", "streamhash", "-hash", "sha256", "-").strip().split("=")[-1]


def points(source, track_id, path):
    checked(MKV / "mkvextract.exe", source, "timestamps_v2", f"{track_id}:{path}")
    data = path.read_text().splitlines()
    return [float(data[index]) for index in (1, len(data) // 2, len(data) - 1)]


def verify(source, root, exe, minutes):
    original_hash = digest(source)
    input_meta = identify(source)
    tracks = input_meta["tracks"]
    subtitles = signature(source, 2)
    video = signature(source, 0)
    chapters = dump_xml(source, "chapters", root / "source-chapters.xml")
    tags = dump_xml(source, "tags", root / "source-tags.xml")
    hash_audio = signature(source, 1)
    reference = points(source, 1, root / "source-times.txt")
    video_points = points(source, 0, root / "source-video.txt")
    assert abs(reference[0] - 125) <= 1, reference
    host = (exe, "--state-dir", root / "state")
    for mode, preset, codec in (("add", "dts-hd-ma", "A_DTS"), ("replace", "true-hd", "A_TRUEHD")):
        # DTS for both long runs keeps the feature-length test practical.
        if minutes and mode == "replace":
            preset, codec = "dts-hd-ma", "A_DTS"
        output = root / f"{mode} 音.mkv"
        result = invoke(*host, "convert", preset, "--track", 1, "--mkv-mode", mode, source, output)
        assert result.returncode == 0, (result.stdout, result.stderr)
        assert digest(source) == original_hash
        report = identify(output)
        index = 3 if mode == "add" else 1
        stream = report["tracks"][index]
        assert stream["properties"]["codec_id"] == codec
        assert stream["properties"]["default_track"] is (mode == "replace")
        assert stream["properties"]["language_ietf"] == "ja"
        if "flag_original" in tracks[1]["properties"]:
            assert stream["properties"].get("flag_original") == tracks[1]["properties"]["flag_original"]
        assert stream["properties"].get("flag_commentary") is True
        assert [t["type"] for t in report["tracks"]] == ["video", "audio", "subtitles"] + (["audio"] if mode == "add" else [])
        assert signature(output, 0) == video and signature(output, 2) == subtitles
        assert dump_xml(output, "chapters", root / f"{mode}-chapters.xml") == chapters
        assert dump_xml(output, "tags", root / f"{mode}-tags.xml") == tags
        attached = root / f"{mode}-attachment.txt"
        checked(MKV / "mkvextract.exe", output, "attachments", f'{report["attachments"][0]["id"]}:{attached}')
        assert attached.read_bytes() == (root / "note.txt").read_bytes()
        if mode == "add":
            assert signature(output, 1) == hash_audio
            assert report["tracks"][1]["properties"]["default_track"] == tracks[1]["properties"]["default_track"]
        actual = points(output, stream["id"], root / f"{mode}-audio.txt")
        actual_video = points(output, 0, root / f"{mode}-video.txt")
        assert actual_video == video_points
        if preset == "true-hd":
            assert abs(actual[0] - reference[0]) <= 2 and abs(actual[2] - reference[2]) <= 3
            assert abs(actual[1] - reference[1]) <= 12
        else:
            assert all(abs(a - b) <= 1.001 for a, b in zip(actual, reference))
        print(f"{mode} {preset}: audio timestamps {actual} ms, retained video {actual_video} ms", flush=True)

    # A 20 ms gap after the first packet must fail before extraction/encoding.
    broken = root / "gap.mkv"
    tc = root / "gap.txt"
    tc.write_text("# timestamp format v2\n0\n30.666667\n" +
                  "".join(f"{20 + n * 512 / 48:.6f}\n" for n in range(2, 188)))
    checked(MKV / "mkvmerge.exe", "-o", broken, root / "video.mkv",
            "--timestamps", f"0:{tc}", FIXTURES / "synthetic-dtsx.dts")
    invalid = root / "bad.mkv"
    result = invoke(*host, "convert", "dts-hd-ma", "--track", 1, "--mkv-mode", "add", broken, invalid)
    assert result.returncode and "unsupported timestamps" in result.stderr and not invalid.exists(), result.stderr

    if not minutes:
        tagged = root / "track tags.mkv"
        shutil.copyfile(source, tagged)
        checked(MKV / "mkvpropedit.exe", tagged, "--tags", f"track:a1:{root / 'retained-tags.xml'}")
        result = invoke(*host, "convert", "dts-hd-ma", "--track", 1, "--mkv-mode", "replace", tagged, invalid)
        assert result.returncode and "track-specific tags" in result.stderr and not invalid.exists(), result.stderr


def verify_browser(root, source, test_binary):
    from playwright.sync_api import expect, sync_playwright
    from test_web_ui import host
    edge = os.environ.get("XFOLD_TEST_BROWSER",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
    env = {**os.environ, "XFOLD_BROWSER_TEST_ROOT": str(root), "XFOLD_BROWSER_TEST_SOURCE": source.name}
    command = [test_binary, "--ignored", "--exact", "web::tests::browser_fixture", "--nocapture"]
    with host(command, env=env) as url, sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=edge, headless=True)
        page = browser.new_page()
        page.goto(url)
        page.get_by_role("button", name="Choose file").click()
        track = page.get_by_label("MKV audio track")
        expect(track).to_be_enabled()
        track.select_option("1")
        fmt = page.get_by_label("Output format")
        stage = page.locator("#stage")
        for mode, count in (("add", 4), ("replace", 3)):
            expect(fmt).to_be_enabled()
            fmt.select_option(mode)
            expect(page.locator("#mkv-mode-help")).to_be_visible()
            save = page.get_by_role("button", name="Save as")
            save.click()
            expect(page.locator("#output-path")).to_contain_text("export.mkv")
            previous = page.evaluate("""async () => {
                const token = sessionStorage.getItem('xfold-session');
                const reply = await fetch('/api/status', {headers:{Authorization:'Bearer ' + token}});
                return (await reply.json()).attempt?.id ?? null;
            }""")
            page.locator("#convert").click()
            page.wait_for_function("""async previous => {
                const token = sessionStorage.getItem('xfold-session');
                const reply = await fetch('/api/status', {headers:{Authorization:'Bearer ' + token}});
                const state = await reply.json();
                return !state.running && state.attempt?.id !== previous && state.attempt?.output.endsWith('export.mkv');
            }""", arg=previous, timeout=60_000)
            expect(stage).to_have_text("Complete", timeout=15_000)
            exported = root / "export.mkv"
            assert len(identify(exported)["tracks"]) == count
            exported.replace(root / f"browser-{mode}.mkv")
        browser.close()
    print("Passed both full-MKV modes through the browser.", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=DEFAULT_EXE)
    parser.add_argument("--minutes", type=int, default=0)
    parser.add_argument("--browser-test-binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="phase4-", dir=ROOT / "target") as temp:
        root = Path(temp)
        source, duration = fixture(root, args.minutes)
        verify(source, root, args.exe.resolve(), args.minutes)
        if args.browser_test_binary:
            verify_browser(root, source, args.browser_test_binary.resolve())
        print(f"Verified add and replace for a {duration:.3f} second fixture.", flush=True)
