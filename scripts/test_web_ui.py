"""Windows Phase 2 browser test. Requires Playwright and an installed Chromium."""
from contextlib import contextmanager
from pathlib import Path
import http.client
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import urlsplit

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target/x86_64-pc-windows-gnu"
FIXTURE = ROOT / "tools/fixtures/synthetic-dtsx.dts"


@contextmanager
def host(command, **kwargs):
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          text=True, encoding="utf-8", **kwargs) as process:
        lines = queue.Queue()

        def collect():
            for line in process.stdout:
                lines.put(line.strip())

        threading.Thread(target=collect, daemon=True).start()
        try:
            for _ in range(20):
                line = lines.get(timeout=10)
                if line.startswith("http://127.0.0.1:"):
                    yield line
                    return
            raise AssertionError("XFold did not emit a loopback launch URL.")
        finally:
            if process.poll() is None:
                process.kill()


def verify_boundaries(url):
    parsed = urlsplit(url)
    origin = f"{parsed.scheme}://{parsed.netloc}"
    authorization = {"Authorization": f"Bearer {parsed.fragment}"}

    def request(method, path, headers=None, body=None):
        connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=5)
        try:
            connection.request(method, path, body=body, headers=headers or {})
            response = connection.getresponse()
            return response.status, response.read(), response.headers
        finally:
            connection.close()

    assert request("GET", "/api/status")[0] == 401
    assert request("GET", "/api/status", {"Authorization": "Bearer invalid"})[0] == 401
    assert request("GET", "/api/status", {**authorization, "Host": "example.test"})[0] == 403
    assert request("GET", "/api/status", {**authorization, "Origin": "https://example.test"})[0] == 403
    assert request("POST", "/api/convert", authorization)[0] == 403
    approved = {**authorization, "Origin": origin}
    assert request("POST", "/api/convert", approved)[0] == 400
    assert request("POST", "/api/choose-input", approved, b'{"path":"C:/arbitrary.dts"}')[0] == 400
    assert request("GET", "/api/diagnostics")[0] == 401
    assert request("GET", "/api/status", authorization)[0] == 200
    code, body, headers = request("GET", "/")
    assert code == 200 and b"Convert audio" in body and parsed.fragment.encode() not in body
    assert "frame-ancestors 'none'" in headers["Content-Security-Policy"]
    assert headers["Cache-Control"] == "no-store"


def verify_workflow(url, root, browser_path):
    source = root / "source.dts"
    source.write_bytes(b"invalid DTS:X")
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=str(browser_path), headless=True)
        page = browser.new_page(viewport={"width": 1100, "height": 760})
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(url)
        stage = page.locator("#stage")
        announce = page.get_by_role("status")
        choose = page.get_by_role("button", name="Choose file")
        save = page.get_by_role("button", name="Save as")
        start = page.locator("#convert")
        expect(choose).to_be_enabled()
        assert "#" not in page.url
        page.keyboard.press("Tab")
        expect(page.get_by_role("link", name="XFold home")).to_be_focused()
        page.keyboard.press("Tab")
        expect(page.get_by_role("button", name="Dark mode")).to_be_focused()
        page.keyboard.press("Space")
        expect(page.locator("html")).to_have_attribute("data-theme", "light")
        page.keyboard.press("Space")
        expect(page.locator("html")).to_have_attribute("data-theme", "dark")
        page.keyboard.press("Tab")
        expect(choose).to_be_focused()
        page.keyboard.press("Enter")
        expect(page.locator("#input-path")).to_contain_text("source.dts")
        save.focus()
        page.keyboard.press("Enter")
        expect(page.locator("#output-path")).to_contain_text("export.dts")
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Failed")
        expect(announce).to_contain_text("Failed")
        expect(start).to_have_text("Retry conversion")
        expect(start).to_be_enabled()
        assert not (root / "export.dts").exists()
        shutil.copyfile(FIXTURE, source)
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Complete", timeout=20_000)
        expect(announce).to_contain_text("Complete")
        assert (root / "export.dts").stat().st_size > 0
        page.get_by_text("Diagnostics", exact=True).focus()
        page.keyboard.press("Enter")
        expect(page.locator("#log")).to_contain_text("encode.stdout.txt")

        # A longer fixture keeps decode active across reload, then exercises
        # cancellation and retry using the selections retained by the host.
        segment = FIXTURE.read_bytes()
        with source.open("wb") as file:
            for _ in range(96):
                file.write(segment)
        page.locator("#dts").focus()
        page.keyboard.press("ArrowDown")
        expect(page.locator("#truehd")).to_be_checked()
        expect(page.locator("#output-path")).to_have_text("Choose where to save the export")
        save.focus()
        page.keyboard.press("Enter")
        expect(page.locator("#output-path")).to_contain_text("export.thd")
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Decoding DTS:X")
        page.reload()
        cancel = page.get_by_role("button", name="Cancel job")
        expect(cancel).to_be_visible()
        expect(page.locator("#input-path")).to_contain_text("source.dts")
        expect(page.locator("#output-path")).to_contain_text("export.thd")
        cancel.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Cancelled", timeout=15_000)
        expect(announce).to_contain_text("Cancelled")
        expect(start).to_be_enabled()
        assert not (root / "export.thd").exists()
        shutil.copyfile(FIXTURE, source)
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Complete", timeout=20_000)
        expect(page.locator("#detail")).to_contain_text("unkeyed")
        assert (root / "export.thd").stat().st_size > 0
        assert not list(root.glob(".xfold-*"))

        for extension, profile in (("dts", "DTS-HD MA"), ("thd", "Dolby TrueHD + Dolby Atmos")):
            result = subprocess.check_output(
                ["ffprobe", "-v", "error", "-show_streams", "-of", "json", root / f"export.{extension}"],
                text=True,
            )
            assert json.loads(result)["streams"][0]["profile"] == profile
        page.screenshot(path=ROOT / "target/phase2-desktop.png")
        page.set_viewport_size({"width": 390, "height": 850})
        assert page.evaluate("document.documentElement.scrollWidth") == 390
        page.screenshot(path=ROOT / "target/phase2-narrow.png", full_page=True)
        assert not errors, errors
        browser.close()


def verify_native_picker_opens(url, endpoint):
    parsed = urlsplit(url)
    headers = {"Authorization": f"Bearer {parsed.fragment}",
               "Origin": f"{parsed.scheme}://{parsed.netloc}"}
    result = queue.Queue()

    def request():
        connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=3)
        try:
            connection.request("POST", endpoint, headers=headers)
            response = connection.getresponse()
            result.put((response.status, response.read()))
        except OSError:
            pass
        finally:
            connection.close()

    thread = threading.Thread(target=request, daemon=True)
    thread.start()
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline and result.empty():
        connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=2)
        try:
            connection.request("GET", "/api/status", headers=headers)
            response = connection.getresponse()
            if json.loads(response.read())["picker"]:
                time.sleep(0.35)
                assert result.empty(), result.queue
                return
        finally:
            connection.close()
        time.sleep(0.03)
    raise AssertionError(f"Native picker did not remain active for {endpoint}: {result.queue}")


if __name__ == "__main__":
    chromium = Path(os.environ.get("XFOLD_TEST_BROWSER",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"))
    test_binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="phase2-", dir=ROOT / "target") as temporary:
        workspace = Path(temporary)
        with host([test_binary, "--ignored", "--exact", "web::tests::browser_fixture", "--nocapture"],
                  env={**os.environ, "XFOLD_BROWSER_TEST_ROOT": str(workspace)}) as url:
            verify_boundaries(url)
            verify_workflow(url, workspace, chromium)
        with host([TARGET / "release/xfold.exe", "--state-dir", workspace / "production",
                   "serve", "--no-browser"]) as url:
            verify_boundaries(url)
        if os.environ.get("XFOLD_TEST_NATIVE") == "1":
            for endpoint in ("/api/choose-input", "/api/choose-output"):
                with host([TARGET / "release/xfold.exe", "--state-dir", workspace / "production",
                           "serve", "--no-browser"]) as url:
                    verify_native_picker_opens(url, endpoint)
            print("Passed: both production native picker requests entered the modal wait.")
    print("Passed: HTTP boundaries on production and test hosts, keyboard, theme, both presets, reload, cancellation, retry, live status, and diagnostics.")
