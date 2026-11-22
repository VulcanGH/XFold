"""Download, verify and stage the manifest-pinned Windows bundle. Python 3.11+.

Run with --build to build Harlettizer with the repo-local GNU toolchain.
Use --offline to require the downloaded cache. Never accepts a changed hash.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = tomllib.loads((ROOT / "tools/manifest.toml").read_text())
DOWNLOADS = ROOT / "tools/downloads"
EVIDENCE = ROOT / "docs/evidence/phase0"


def sha(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def acquire(url, expected, name, offline):
    path = DOWNLOADS / name
    if not path.exists():
        if offline:
            raise RuntimeError(f"Offline cache missing: {path}")
        partial = path.with_suffix(path.suffix + ".partial")
        subprocess.run(["curl.exe", "--fail", "--location", "--silent", "--show-error",
                        "--retry", "2", "--output", str(partial), url], check=True)
        if sha(partial) != expected:
            raise RuntimeError(f"Downloaded checksum mismatch: {url}")
        partial.replace(path)
    if sha(path) != expected:
        raise RuntimeError(f"Cached checksum mismatch: {path}; inspect and remove manually")
    return path


def unzip(archive, destination):
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as z:
        for member in z.infolist():
            if not (destination / member.filename).resolve().is_relative_to(destination.resolve()):
                raise RuntimeError("Unsafe archive path")
        z.extractall(destination)


def sevenzip(archive, destination):
    candidates = [shutil.which("7z"), r"C:\Program Files\7-Zip\7z.exe"]
    exe = next((p for p in candidates if p and Path(p).exists()), None)
    if not exe:
        raise RuntimeError("7-Zip CLI required to extract official portable packages")
    subprocess.run([exe, "x", str(archive), f"-o{destination}", "-y"], check=True,
                   stdout=subprocess.DEVNULL)


def notices(source, destination):
    destination.mkdir(parents=True, exist_ok=True)
    for name in ("LICENSE", "LICENSES", "NOTICE", "NOTICE.txt", "README.md", "LEGAL.md",
                 "Cargo.lock", "Cargo.toml"):
        item = source / name
        if item.is_dir():
            shutil.copytree(item, destination / name, dirs_exist_ok=True)
        elif item.exists():
            shutil.copy2(item, destination / name)
    for name in ("encode.md", "provenance.md", "dts.md"):
        item = source / "docs" / name
        if item.exists():
            shutil.copy2(item, destination / name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--evidence-dir", type=Path, default=EVIDENCE)
    args = parser.parse_args()
    evidence = args.evidence_dir
    DOWNLOADS.mkdir(parents=True, exist_ok=True)
    evidence.mkdir(parents=True, exist_ok=True)
    specs = MANIFEST["tools"]
    h = specs["harletty"]
    binary = acquire(h["url"], h["sha256"], h["url"].rsplit("/", 1)[1], args.offline)
    unzip(binary, ROOT / "tools/bin/harletty")
    source = acquire(h["source_url"], h["source_sha256"],
                     f"harletty-{h['commit']}-source.zip", args.offline)
    unpack = ROOT / "tools/source/harletty-exact"
    unzip(source, unpack)
    notices(next(unpack.iterdir()), ROOT / "tools/bin/harletty/notices")
    m = specs["mkvtoolnix"]
    archive = acquire(m["url"], m["sha256"], m["url"].rsplit("/", 1)[1], args.offline)
    sevenzip(archive, ROOT / "tools/bin/mkvtoolnix")
    hz = specs["harlettizer"]
    archive = acquire(hz["url"], hz["sha256"], f"harlettizer-{hz['commit']}.zip", args.offline)
    unpack = ROOT / "tools/source/harlettizer"
    unzip(archive, unpack)
    source = next(unpack.iterdir())
    if args.build:
        rust = ROOT / "tools/toolchain/bin"
        if not (rust / "cargo.exe").exists():
            if args.offline:
                raise RuntimeError("Stage local Rust first: python scripts/portable_rust.py")
            subprocess.run(["python", str(ROOT / "scripts/portable_rust.py")], check=True)
        w = MANIFEST["build"]["w64devkit"]
        archive = acquire(w["url"], w["sha256"], w["url"].rsplit("/", 1)[1], args.offline)
        if not (ROOT / "tools/build/w64devkit/bin/dlltool.exe").exists():
            sevenzip(archive, ROOT / "tools/build")
        env = os.environ.copy()
        target = MANIFEST["build"]["rust"]["target"]
        runtime = ROOT / "tools/toolchain/lib/rustlib" / target / "lib/self-contained"
        env["PATH"] = os.pathsep.join([str(rust),
                                      str(ROOT / "tools/build/w64devkit/bin"), env["PATH"]])
        env["LIBRARY_PATH"] = str(runtime)
        # Rust's bundled MinGW runtime matches its standard library. The external
        # kit supplies GCC/dlltool; LIBRARY_PATH keeps unwind linkage matched.
        env["RUSTFLAGS"] = "-C link-self-contained=yes"
        env["CARGO_HOME"] = str(ROOT / "tools/cargo-home")
        env["CARGO_TARGET_DIR"] = str(ROOT / "tools/build/harlettizer-target")
        command = [str(rust / "cargo.exe"), "build", "--locked", "--release", "--target",
                   MANIFEST["build"]["rust"]["target"], "-p", "hz-cli", "-p", "xtask"]
        if args.offline:
            command.append("--offline")
        build_log = evidence / "harlettizer-build.txt"
        with build_log.open("w") as log:
            result = subprocess.run(command, cwd=source, env=env, stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            print(build_log.read_text(encoding="utf-8", errors="replace")[-4000:], flush=True)
            result.check_returncode()
        output = Path(env["CARGO_TARGET_DIR"]) / MANIFEST["build"]["rust"]["target"] / "release/harlettizer.exe"
        shutil.copy2(output, ROOT / "tools/bin/harlettizer/harlettizer.exe")
        # This is the workspace's resolved dependency superset, including tools
        # and optional packages, not a claim that every entry is linked in the CLI.
        metadata_command = [str(rust / "cargo.exe"), "metadata", "--locked",
                            "--format-version", "1", "--filter-platform", target]
        if args.offline:
            metadata_command.append("--offline")
        metadata = subprocess.run(metadata_command, cwd=source, env=env, stdout=subprocess.PIPE, check=True)
        packages = json.loads(metadata.stdout)["packages"]
        licence_rows = []
        for package in packages:
            dest = ROOT / "tools/bin/harlettizer/notices/dependencies" / f"{package['name']}-{package['version']}"
            base = Path(package["manifest_path"]).parent
            texts = [p for p in base.iterdir() if p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE"))]
            for item in texts:
                dest.mkdir(parents=True, exist_ok=True)
                if item.is_dir():
                    shutil.copytree(item, dest / item.name, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, dest / item.name)
            licence_rows.append({"name": package["name"], "version": package["version"],
                                 "licence": package["license"], "source": package["source"],
                                 "notice_files": [p.name for p in texts]})
        (evidence / "harlettizer-workspace-licences.json").write_text(json.dumps(licence_rows, indent=2) + "\n")
    notices(source, ROOT / "tools/bin/harlettizer/notices")
    inventory = {}
    for name, spec in specs.items():
        base = ROOT / "tools/bin" / spec["directory"]
        inventory[name] = {str(p.relative_to(base)).replace("\\", "/"): sha(p)
                           for p in sorted(base.rglob("*")) if p.is_file()}
    record = {"files": inventory, "harlettizer_lock_sha256": sha(source / "Cargo.lock"),
              "harlettizer_commit": hz["commit"], "qualification": "See qualification.json; staging is not qualification"}
    (evidence / "bundle.json").write_text(json.dumps(record, indent=2) + "\n")
    print("Staged verified artifacts; qualification is a separate gate.")


if __name__ == "__main__":
    main()
