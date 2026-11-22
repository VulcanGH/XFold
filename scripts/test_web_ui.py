"""Windows Phase 2 browser test. Requires Playwright and an installed Chromium."""
from contextlib import contextmanager
from pathlib import Path
import ctypes
from ctypes import wintypes
import http.client
import json
import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from urllib.parse import urlsplit

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
TARGET = ROOT / "target/x86_64-pc-windows-gnu"
FIXTURE = ROOT / "tools/fixtures/synthetic-dtsx.dts"
MOCK_WEBHOOK = "https://discord.com/api/webhooks/123456/test_token-only"


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
    assert request("POST", "/api/settings/theme/light")[0] == 401
    assert request("POST", "/api/settings/theme/light", authorization)[0] == 403
    assert request("POST", "/api/settings/theme/invalid", approved)[0] == 400
    webhook_path = "/api/settings/webhook"
    json_headers = {**approved, "Content-Type": "application/json"}
    assert request("POST", webhook_path, {"Origin": origin})[0] == 401
    assert request("POST", webhook_path, authorization)[0] == 403
    assert request("POST", webhook_path, json_headers, b'{"url":"https://example.test"}')[0] == 400
    assert request("POST", webhook_path, json_headers, b" " * 513)[0] == 400
    route = "/api/choose-input"
    assert request("POST", route)[0] == 401
    assert request("POST", route, authorization)[0] == 403
    assert request("POST", route, json_headers, b'{"marker":"invalid"}')[0] == 400
    assert request("POST", route, json_headers, b'{"path":"C:/arbitrary.dts"}')[0] == 400
    assert request("POST", "/api/convert", approved)[0] == 400
    assert request("POST", "/api/reset")[0] == 401
    assert request("POST", "/api/reset", authorization)[0] == 403
    assert request("GET", "/api/diagnostics")[0] == 401
    assert request("GET", "/api/status", authorization)[0] == 200
    code, body, headers = request("GET", "/")
    assert code == 200 and b' id="convert-title"' in body and parsed.fragment.encode() not in body
    assert "frame-ancestors 'none'" in headers["Content-Security-Policy"]
    assert headers["Cache-Control"] == "no-store"


def select_file(page, path, keyboard=False):
    with page.expect_response(lambda response: response.url.endswith("/api/choose-input")) as selected:
        button = page.locator("#input")
        if keyboard:
            button.press("Enter")
        else:
            button.click()
    assert selected.value.ok, selected.value.text()
    assert selected.value.json()["selected"]
    expect(page.locator("#input-path")).to_contain_text(path.name)
    expect(button).to_be_enabled()


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
        announce = page.locator("#announcement")
        choose = page.get_by_role("button", name="Select file")
        start = page.locator("#convert")
        expect(choose).to_be_enabled()
        assert "#" not in page.url
        page.keyboard.press("Tab")
        expect(page.get_by_role("link", name="XFold home")).to_be_focused()
        page.keyboard.press("Tab")
        expect(page.get_by_role("link", name="Convert", exact=True)).to_be_focused()
        expect(page.get_by_role("link", name="Convert", exact=True)).to_have_attribute("aria-current", "page")
        page.keyboard.press("Tab")
        expect(page.get_by_role("link", name="Settings")).to_be_focused()
        page.keyboard.press("Tab")
        expect(page.get_by_role("button", name="Light mode")).to_be_focused()
        page.keyboard.press("Space")
        expect(page.locator("html")).to_have_attribute("data-theme", "light")
        expect(page.get_by_role("button", name="Dark mode")).to_be_focused()
        page.keyboard.press("Space")
        expect(page.locator("html")).to_have_attribute("data-theme", "dark")
        expect(page.get_by_role("button", name="Light mode")).to_be_focused()
        page.keyboard.press("Tab")
        expect(choose).to_be_focused()
        select_file(page, source, keyboard=True)
        expect(page.locator("#output-path")).to_contain_text("source - DTS-HD MA.dts")
        expect(start).to_be_enabled()
        page.get_by_role("link", name="Settings").click()
        page.get_by_label("Discord webhook URL").fill(MOCK_WEBHOOK)
        page.get_by_role("button", name="Save webhook").click()
        expect(page.get_by_text("Webhook saved.")).to_be_visible()
        page.get_by_role("link", name="Convert", exact=True).click()
        expect(page.get_by_role("heading", name="Convert DTS:X")).to_be_focused()
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Failed")
        expect(announce).to_contain_text("Failed")
        expect(start).to_have_text("Retry conversion")
        expect(start).to_be_enabled()
        assert not (root / "source - DTS-HD MA.dts").exists()
        assert not (root / "mock-notification.json").exists()
        shutil.copyfile(FIXTURE, source)
        select_file(page, source)
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Complete", timeout=20_000)
        expect(announce).to_contain_text("Complete")
        assert (root / "source - DTS-HD MA.dts").stat().st_size > 0
        expect(page.locator("#notification-status")).to_have_text("Discord notification sent.")
        payload = json.loads((root / "mock-notification.json").read_text(encoding="utf-8"))
        embed, = payload["embeds"]
        assert embed["title"] == "source"
        assert embed["description"] == "Conversion complete"
        assert payload["allowed_mentions"] == {"parse": []}
        fields = {field["name"]: field["value"] for field in embed["fields"]}
        assert fields["Audio"] == "DTS:X → DTS-HD Master Audio"
        assert fields["Export"] == "Raw audio (.dts)"
        assert fields["Export size"] and fields["Conversion time"]
        assert "Source language" not in fields and str(root) not in str(payload)
        page.get_by_text("Diagnostics", exact=True).focus()
        page.keyboard.press("Enter")
        expect(page.locator("#log")).to_contain_text("encode.stdout.txt")

        # A longer fixture keeps decode active across reload so the draft can
        # reset while the original job remains available to cancel.
        segment = FIXTURE.read_bytes()
        with source.open("wb") as file:
            for _ in range(384):
                file.write(segment)
        select_file(page, source)
        page.locator("#dts").focus()
        page.keyboard.press("ArrowDown")
        expect(page.locator("#truehd")).to_be_checked()
        expect(page.locator("#output-path")).to_contain_text("source - TrueHD.thd")
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Decoding DTS:X")
        expect(page.locator("#percent")).to_have_text(re.compile(r"^(?:[1-9]|[1-8][0-9]|9[0-8])%$"))
        waiting = root / "waiting.dts"
        waiting.write_bytes(segment)
        picker_choice = root / "picker-source.txt"
        picker_choice.write_text(waiting.name, encoding="utf-8")
        select_file(page, waiting)
        picker_choice.unlink()
        page.get_by_role("button", name="Add to queue").click()
        waiting_row = page.locator("#queue-items li").filter(has_text=waiting.name)
        expect(waiting_row).to_have_attribute("data-state", "waiting")
        page.reload()
        cancel = page.get_by_role("button", name="Cancel job")
        expect(cancel).to_be_visible()
        expect(waiting_row).to_have_count(0)
        expect(page.locator("#queue-items li")).to_have_count(1)
        expect(page.locator("#input-path")).to_have_text("Choose a .dts or .mkv file")
        expect(page.locator("#output-path")).to_have_text("Select a source to suggest a destination")
        expect(page.locator("#current-source")).to_contain_text("source.dts")
        cancel.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Cancelled", timeout=15_000)
        expect(announce).to_contain_text("Cancelled")
        progress_log = max((root / "browser-state/logs").glob("*/decode-progress.txt"),
                           key=lambda log: log.stat().st_mtime_ns)
        estimates = [int(value) for value in re.findall(r"progress (\d+)%", progress_log.read_text())]
        assert len(estimates) >= 2 and estimates == sorted(set(estimates)) and max(estimates) < 100
        expect(start).to_be_disabled()
        assert not (root / "source - TrueHD.thd").exists()
        assert not (root / "waiting - TrueHD.thd").exists()
        shutil.copyfile(FIXTURE, source)
        select_file(page, source)
        page.locator("#truehd").click()
        expect(page.locator("#output-path")).to_contain_text("source - TrueHD.thd")
        start.focus()
        page.keyboard.press("Enter")
        expect(stage).to_have_text("Complete", timeout=20_000)
        expect(page.locator("#detail")).to_contain_text("unkeyed")
        assert (root / "source - TrueHD.thd").stat().st_size > 0
        expect(page.locator("#notification-status")).to_have_text("Discord notification sent.")
        payload = json.loads((root / "mock-notification.json").read_text(encoding="utf-8"))
        fields = {field["name"]: field["value"] for field in payload["embeds"][0]["fields"]}
        assert fields["Audio"] == "DTS:X → TrueHD with spatial metadata"
        assert fields["Export"] == "Raw audio (.thd)"
        assert not list(root.glob(".xfold-*"))

        for extension, profile in (("dts", "DTS-HD MA"), ("thd", "Dolby TrueHD + Dolby Atmos")):
            result = subprocess.check_output(
                ["ffprobe", "-v", "error", "-show_streams", "-of", "json", root / ("source - DTS-HD MA.dts" if extension == "dts" else "source - TrueHD.thd")],
                text=True,
            )
            assert json.loads(result)["streams"][0]["profile"] == profile

        second = root / "sequel.dts"
        shutil.copyfile(FIXTURE, second)
        select_file(page, second)
        expect(page.locator("#output-path")).to_contain_text("sequel - TrueHD.thd")
        expect(stage).to_have_text("Ready")
        start.click()
        expect(stage).to_have_text("Complete", timeout=20_000)
        assert (root / "sequel - TrueHD.thd").stat().st_size > 0
        expect(page.get_by_role("button", name="Copy output path")).to_be_visible()
        page.get_by_role("button", name="Save as").click()
        expect(page.locator("#output-path")).to_contain_text("export.thd")
        assert second.read_bytes() == FIXTURE.read_bytes()
        page.screenshot(path=ROOT / "target/phase2-desktop.png")
        page.set_viewport_size({"width": 390, "height": 850})
        assert page.evaluate("document.documentElement.scrollWidth") == 390
        page.screenshot(path=ROOT / "target/phase2-narrow.png", full_page=True)
        assert not errors, errors
        browser.close()


def verify_queue(url, root, chromium):
    segment = FIXTURE.read_bytes()
    first = root / "first.dts"
    with first.open("wb") as file:
        for _ in range(512):
            file.write(segment)
    bad = root / "bad.dts"
    bad.write_bytes(b"invalid DTS:X")
    truehd = root / "spatial.dts"
    removed = root / "removed.dts"
    last = root / "last.dts"
    for source in (truehd, removed, last):
        source.write_bytes(segment)

    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=str(chromium), headless=True)
        page = browser.new_page(viewport={"width": 900, "height": 800})
        page.goto(url)
        expect(page.get_by_role("heading", name="Queue", exact=True)).to_be_visible()
        expect(page.get_by_role("button", name="Add to queue")).to_be_disabled()
        def select(source):
            (root / "picker-source.txt").write_text(source.name, encoding="utf-8")
            select_file(page, source)

        def row(source):
            return page.locator("#queue-items li").filter(has_text=source.name)

        def add(source):
            page.get_by_role("button", name="Add to queue").click()
            expect(row(source)).to_have_attribute("data-state", "waiting")

        select(first)
        add(first)
        expect(page.get_by_role("button", name="Cancel job")).to_be_hidden()
        select(bad)
        add(bad)
        expect(page.locator("#queue-items li")).to_have_count(2)
        expect(row(first)).to_have_attribute("data-state", "waiting")
        expect(row(bad)).to_have_attribute("data-state", "waiting")
        page.get_by_role("button", name="Start queue").click()
        expect(row(first)).to_have_attribute("data-state", "running")
        expect(page.get_by_role("button", name="Add another file")).to_be_enabled()
        expect(page.get_by_role("button", name="Add to queue")).to_be_disabled()
        select(first)
        expect(page.locator("#output-path")).to_contain_text("first - DTS-HD MA (2).dts")
        select(truehd)
        page.locator("#truehd").click()
        expect(page.locator("#output-path")).to_contain_text("spatial - TrueHD.thd")
        add(truehd)
        select(removed)
        page.locator("#dts").click()
        expect(page.locator("#output-path")).to_contain_text("removed - DTS-HD MA.dts")
        add(removed)
        page.get_by_role("button", name="Remove job 04, removed.dts").click()
        expect(row(removed)).to_have_count(0)
        select(last)
        add(last)
        expect(page.locator("#current-source")).to_have_text("01 · first.dts")
        expect(page.locator("#source-number")).to_have_text("05")
        expect(row(truehd)).to_contain_text("TrueHD · THD")
        expect(row(last)).to_contain_text("DTS-HD MA · DTS")
        expect(page.locator("#queue-items li")).to_have_count(4)
        page.get_by_role("button", name="Cancel job").click()
        expect(row(first)).to_have_attribute("data-state", "cancelled", timeout=15_000)
        expect(row(bad)).to_have_attribute("data-state", "failed", timeout=15_000)
        expect(row(bad).locator(".queue-error")).not_to_be_empty()
        expect(row(truehd)).to_have_attribute("data-state", "succeeded", timeout=30_000)
        expect(row(last)).to_have_attribute("data-state", "succeeded", timeout=30_000)
        expect(page.get_by_role("button", name="Select file")).to_be_enabled()
        assert (root / "spatial - TrueHD.thd").stat().st_size > 0
        assert (root / "last - DTS-HD MA.dts").stat().st_size > 0
        assert not (root / "removed - DTS-HD MA.dts").exists()
        stamps = [entry.name for entry in sorted((root / "browser-state/logs").iterdir())]
        assert len(stamps) == 4, stamps
        select(removed)
        add(removed)
        expect(page.get_by_role("button", name="Start queue")).to_be_enabled()
        page.reload()
        expect(page.locator("#queue-empty")).to_be_visible()
        expect(page.locator("#queue-items li")).to_have_count(0)
        expect(page.locator("#input-path")).to_have_text("Choose a .dts or .mkv file")
        expect(page.locator("#dts")).to_be_checked()
        expect(page.get_by_role("button", name="Start conversion")).to_be_disabled()
        assert (root / "last - DTS-HD MA.dts").is_file()
        browser.close()


def verify_browser_picker_focus(url, chromium):
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    foreground = user32.GetForegroundWindow
    foreground.restype = wintypes.HWND
    owner = user32.GetWindow
    owner.argtypes = (wintypes.HWND, wintypes.UINT)
    owner.restype = wintypes.HWND
    get_class = user32.GetClassNameW
    get_class.argtypes = (wintypes.HWND, wintypes.LPWSTR, ctypes.c_int)
    get_class.restype = ctypes.c_int
    get_title = user32.GetWindowTextW
    get_title.argtypes = get_class.argtypes
    get_title.restype = ctypes.c_int
    callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    enumerate_windows = user32.EnumWindows
    enumerate_windows.argtypes = (callback, wintypes.LPARAM)
    enumerate_windows.restype = wintypes.BOOL
    activate = user32.SetForegroundWindow
    activate.argtypes = (wintypes.HWND,)
    activate.restype = wintypes.BOOL

    def window_class(hwnd):
        name = ctypes.create_unicode_buffer(256)
        get_class(hwnd, name, len(name))
        return name.value

    def window_title(hwnd):
        title = ctypes.create_unicode_buffer(256)
        get_title(hwnd, title, len(title))
        return title.value

    def test_browser_window():
        matches = []

        @callback
        def collect(hwnd, _):
            if window_class(hwnd) == "Chrome_WidgetWin_1" and "XFold browser-picker focus test" in window_title(hwnd):
                matches.append(hwnd)
            return True

        enumerate_windows(collect, 0)
        return matches[0] if matches else None

    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=str(chromium), headless=False)
        try:
            page = browser.new_page()
            page.goto(url)
            page.evaluate("document.title = 'XFold browser-picker focus test'")
            page.bring_to_front()
            button = page.get_by_role("button", name="Select file")
            expect(button).to_be_enabled()
            deadline = time.monotonic() + 3
            browser_window = test_browser_window()
            while not browser_window and time.monotonic() < deadline:
                time.sleep(0.05)
                browser_window = test_browser_window()
            assert browser_window, "The headful test browser window was not found."
            activate(browser_window)
            deadline = time.monotonic() + 3
            while foreground() != browser_window and time.monotonic() < deadline:
                time.sleep(0.05)
            if foreground() != browser_window:
                raise AssertionError(f"The test browser did not acquire initial focus. Active window: {window_title(foreground())!r}")
            button.click(no_wait_after=True)
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                active = foreground()
                if (window_class(active) == "#32770" and window_title(active) == "Select XFold source"
                        and owner(active, 4) == browser_window):  # GW_OWNER
                    return
                time.sleep(0.05)
            raise AssertionError(f"The owned Windows dialog did not acquire focus; active window was {window_title(foreground())!r}.")
        finally:
            browser.close()


def verify_settings(test_binary, workspace, chromium):
    command = [test_binary, "--ignored", "--exact", "web::tests::browser_fixture", "--nocapture"]
    env = {**os.environ, "XFOLD_BROWSER_TEST_ROOT": str(workspace)}

    def expect_saved(page):
        expect(page.get_by_label("Theme")).to_have_value("light")
        expect(page.get_by_label("Default preset")).to_have_value("true-hd")
        expect(page.get_by_label("Default format for MKV sources")).to_have_value("raw")
        expect(page.locator("html")).to_have_attribute("data-theme", "light")
        expect(page.get_by_text("Webhook saved.")).to_be_visible()
        expect(page.get_by_label("Discord webhook URL")).to_have_value("")

    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=str(chromium), headless=True)
        page = browser.new_page()
        with host(command, env=env) as url:
            page.goto(url)
            expect(page.locator("#version")).to_have_text(f"v{VERSION}")
            page.get_by_role("link", name="Settings").click()
            expect(page.get_by_role("heading", name="Settings")).to_be_focused()
            expect(page.get_by_role("link", name="Settings")).to_have_attribute("aria-current", "page")
            for label, value in (("Theme", "light"), ("Default preset", "true-hd"),
                                 ("Default format for MKV sources", "raw")):
                selector = page.get_by_label(label, exact=True)
                selector.select_option(value)
                expect(selector).to_be_enabled()
                expect(selector).to_have_value(value)
            page.get_by_label("Discord webhook URL").fill(MOCK_WEBHOOK)
            page.get_by_role("button", name="Save webhook").click()
            expect_saved(page)
            assert MOCK_WEBHOOK.encode() not in (workspace / "browser-state/discord-webhook.dat").read_bytes()
            page.reload()
            expect_saved(page)
        with host(command, env=env) as url:
            page.goto(url)
            expect(page.locator("#truehd")).to_be_checked()
            page.get_by_role("link", name="Settings").click()
            expect_saved(page)
            page.get_by_role("button", name="Remove webhook").click()
            expect(page.get_by_text("Notifications off.")).to_be_visible()
            assert not (workspace / "browser-state/discord-webhook.dat").exists()
            page.get_by_role("link", name="Convert", exact=True).click()
            expect(page.get_by_role("heading", name="Convert DTS:X")).to_be_focused()
        browser.close()


if __name__ == "__main__":
    chromium = Path(os.environ.get("XFOLD_TEST_BROWSER",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"))
    test_binary = Path(sys.argv[1]).resolve()
    release = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else TARGET / "release/xfold.exe"
    with tempfile.TemporaryDirectory(prefix="phase2-", dir=ROOT / "target") as temporary:
        workspace = Path(temporary)
        with host([test_binary, "--ignored", "--exact", "web::tests::browser_fixture", "--nocapture"],
                  env={**os.environ, "XFOLD_BROWSER_TEST_ROOT": str(workspace),
                       "XFOLD_BROWSER_TEST_NEXT_SOURCE": "sequel.dts"}) as url:
            verify_boundaries(url)
            verify_workflow(url, workspace, chromium)
        verify_settings(test_binary, workspace / "settings", chromium)
        queue_root = workspace / "queue"
        with host([test_binary, "--ignored", "--exact", "web::tests::browser_fixture", "--nocapture"],
                  env={**os.environ, "XFOLD_BROWSER_TEST_ROOT": str(queue_root)}) as url:
            verify_queue(url, queue_root, chromium)
        with host([release, "--state-dir", workspace / "production",
                   "serve", "--no-browser"]) as url:
            verify_boundaries(url)
            if os.environ.get("XFOLD_TEST_NATIVE") == "1":
                verify_browser_picker_focus(url, chromium)
                print("Passed: the owned Windows file dialog acquired foreground focus.")
    print("Passed: picker request and path handling, both presets, direct exports, FIFO queue, reload, cancellation, settings, and HTTP boundaries.")
