"""Stage Rust 1.92.0 GNU locally; no registry, PATH or machine installation changes."""
import hashlib
import json
import pathlib
import shutil
import subprocess
import tarfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = "1.92.0"


def download(url, path):
    subprocess.run(["curl.exe", "--fail", "--location", "--silent", "--show-error",
                    "--retry", "2", "--output", str(path), url], check=True)


def main():
    downloads = ROOT / "tools/downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    components = json.loads((ROOT / "tools/rust-lock.json").read_text())
    prefix = ROOT / "tools/toolchain"
    prefix.mkdir(parents=True, exist_ok=True)
    for item in components:
        name, url, expected = item["component"], item["url"], item["sha256"]
        archive = downloads / url.rsplit("/", 1)[1]
        if not archive.exists():
            download(url, archive)
        actual = hashlib.file_digest(archive.open("rb"), "sha256").hexdigest()
        if actual != expected:
            raise RuntimeError(f"Checksum mismatch: {archive}")
        unpack = ROOT / "tools/build" / archive.name.removesuffix(".tar.xz")
        unpack.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive) as tar:
            tar.extractall(unpack, filter="data")
        base = next(unpack.iterdir())
        for component in (base / "components").read_text().splitlines():
            for child in (base / component).iterdir():
                dest = prefix / child.name
                if child.is_dir():
                    shutil.copytree(child, dest, dirs_exist_ok=True)
                elif child.name != "manifest.in":
                    shutil.copy2(child, dest)
        print(f"Staged {name} {VERSION}", flush=True)


if __name__ == "__main__":
    main()
