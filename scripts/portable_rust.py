"""Stage Rust 1.92.0 GNU locally; no registry, PATH or machine installation changes."""
import hashlib
import json
import pathlib
import shutil
import subprocess
import tarfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-gnu"
VERSION = "1.92.0"


def download(url, path):
    subprocess.run(["curl.exe", "--fail", "--location", "--silent", "--show-error",
                    "--retry", "2", "--output", str(path), url], check=True)


def main():
    downloads = ROOT / "tools/downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    manifest = downloads / f"channel-rust-{VERSION}.toml"
    if not manifest.exists():
        download(f"https://static.rust-lang.org/dist/channel-rust-{VERSION}.toml", manifest)
    data = tomllib.loads(manifest.read_text())
    prefix = ROOT / "tools/toolchain"
    prefix.mkdir(parents=True, exist_ok=True)
    records = []
    for name in ("rustc", "rust-std", "rust-mingw", "cargo"):
        package = data["pkg"][name]["target"][TARGET]
        url, expected = package["xz_url"], package["xz_hash"]
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
        records.append(dict(component=name, url=url, sha256=actual))
        print(f"Staged {name} {VERSION}", flush=True)
    (ROOT / "docs/evidence/phase0/rust-components.json").write_text(
        json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
