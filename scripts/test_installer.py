"""Exercise offline setup and uninstall in a fresh directory on this Windows host."""
import argparse
import ctypes
import hashlib
import http.client
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import winreg
from urllib.parse import urlsplit

import test_full_mkv
import test_mkv_audio

ROOT = test_mkv_audio.ROOT
INSTALL_KEY = r"Software\XFold"
UNINSTALL_KEY = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\XFold"


def key_exists(name):
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, name, 0,
                            winreg.KEY_READ | winreg.KEY_WOW64_64KEY):
            return True
    except FileNotFoundError:
        return False


def setup(installer, install_dir):
    # NSIS requires /D= to be the last argument and its value to be unquoted.
    return subprocess.run(f'"{installer}" /S /D={install_dir}', timeout=120).returncode


def minimal_env():
    return {**os.environ,
            "PATH": str(Path(os.environ["SystemRoot"]) / "System32"),
            "HTTP_PROXY": "http://127.0.0.1:1", "HTTPS_PROXY": "http://127.0.0.1:1",
            "NO_PROXY": "127.0.0.1"}


def verify_files(app):
    inventory = json.loads((app / "bundle-sha256.json").read_text())
    for relative, expected in inventory.items():
        with (app / relative).open("rb") as file:
            assert hashlib.file_digest(file, "sha256").hexdigest() == expected, relative
    return len(inventory)


def web_status(exe, state):
    with subprocess.Popen([exe, "--state-dir", state, "serve", "--no-browser"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                          env=minimal_env()) as process:
        try:
            url = urlsplit(process.stdout.readline().strip())
            connection = http.client.HTTPConnection(url.hostname, url.port, timeout=5)
            try:
                connection.request("GET", "/api/status", headers={"Authorization": f"Bearer {url.fragment}"})
                reply = connection.getresponse()
                assert reply.status == 200
                assert json.loads(reply.read())["running"] is False
            finally:
                connection.close()
            yield
        finally:
            process.kill()


def restrict_installed_tools(module, exe):
    previous = module.invoke
    def invoke(*args):
        if Path(args[0]) == exe:
            return subprocess.run([str(arg) for arg in args], capture_output=True, text=True,
                                  encoding="utf-8", errors="replace", timeout=120,
                                  env=minimal_env())
        return previous(*args)
    module.invoke = invoke


def uninstall(install_dir):
    subprocess.run([str(install_dir / "uninstall.exe"), "/S"], timeout=60, check=True)
    deadline = time.monotonic() + 60
    while install_dir.exists() and time.monotonic() < deadline:
        time.sleep(0.2)
    assert not install_dir.exists(), "uninstall left the installation directory"
    assert not key_exists(UNINSTALL_KEY)
    assert not key_exists(INSTALL_KEY)


def verify(installer, previous):
    if not ctypes.windll.shell32.IsUserAnAdmin():
        raise RuntimeError("Run the machine-wide installer test from an elevated terminal.")
    if key_exists(UNINSTALL_KEY) or key_exists(INSTALL_KEY):
        raise RuntimeError("An XFold installation or registration already exists for this account.")
    with tempfile.TemporaryDirectory(prefix="xfold-setup-test-") as temporary:
        work = Path(temporary)
        install_dir = work / "XFold x64"
        assert setup(previous or installer, install_dir) == 0, "fresh setup failed"
        exe = install_dir / "app/xfold.exe"
        if previous:
            print("Prior release installed:", subprocess.check_output([exe, "--version"], text=True).strip(), flush=True)
        try:
            # Simulate a file from an earlier package and ensure replacement
            # installs the new complete payload instead of overlaying files.
            obsolete = install_dir / "app/obsolete-from-old-package"
            obsolete.write_text("test marker")
            assert setup(installer, install_dir) == 0, "setup over existing install failed"
            assert not obsolete.exists()
            assert key_exists(UNINSTALL_KEY) and key_exists(INSTALL_KEY), "missing machine-wide XFold registry entries"
            count = verify_files(install_dir / "app")
            state = work / "user-state"
            launch = web_status(exe, state)
            next(launch)
            try:
                assert setup(installer, install_dir) != 0, "setup changed a running installation"
            finally:
                launch.close()
            assert verify_files(install_dir / "app") == count
            test_mkv_audio.EXE = exe
            restrict_installed_tools(test_mkv_audio, exe)
            restrict_installed_tools(test_full_mkv, exe)
            standalone = work / "standalone"
            standalone.mkdir()
            test_mkv_audio.verify_cli(standalone, test_mkv_audio.make_fixture(standalone))
            test_mkv_audio.verify_legacy_language(standalone)
            remux = work / "full"
            remux.mkdir()
            source, _ = test_full_mkv.fixture(remux, 0)
            test_full_mkv.verify(source, remux, exe, 0)
            raw = work / "raw.dts"
            subprocess.run([exe, "--state-dir", state, "convert", "dts-hd-ma",
                            test_mkv_audio.FIXTURES / "synthetic-dtsx.dts", raw],
                           env=minimal_env(), capture_output=True, check=True)
        finally:
            uninstall(install_dir)
        assert (state / "attempt.json").exists()
        assert raw.exists()
        print(f"Passed: fresh-directory install, {'version upgrade' if previous else 'reinstall'}, "
              f"{count} file hashes, running-app protection, installed CLI and loopback UI, "
              "all workflow families with System32-only PATH, and uninstall preserving user files.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("installer", type=Path)
    parser.add_argument("--previous", type=Path, help="Previous machine-wide installer")
    args = parser.parse_args()
    verify(args.installer.resolve(), args.previous.resolve() if args.previous else None)
