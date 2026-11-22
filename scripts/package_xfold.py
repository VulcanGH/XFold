"""Assemble an offline x64 installer and its pinned third-party source kit."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

from stage_tools import ROOT, acquire, sha

TOOLS = ROOT / "tools"
TARGET = ROOT / "target"
MANIFEST = tomllib.loads((TOOLS / "manifest.toml").read_text(encoding="utf-8"))


def archive_tree(zip_file, directory, prefix):
    for path in sorted(directory.rglob("*")):
        if path.is_file() and "__pycache__" not in path.parts:
            zip_file.write(path, f"{prefix}/{path.relative_to(directory).as_posix()}")


def notices_from_crate(archive, notices):
    with tarfile.open(archive, "r:gz") as crate:
        for member in crate.getmembers():
            path = Path(member.name)
            if (member.isfile() and len(path.parts) == 2
                    and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "AUTHORS"))):
                target = notices / path.parts[0] / path.name
                target.parent.mkdir(parents=True, exist_ok=True)
                with crate.extractfile(member) as content, target.open("wb") as out:
                    shutil.copyfileobj(content, out)


def harletty_packages(lock):
    by_name = {}
    for package in lock["package"]:
        by_name.setdefault(package["name"], []).append(package)
    pending = [by_name["harletty"][0]]
    visited = {}
    while pending:
        package = pending.pop()
        key = (package["name"], package["version"], package.get("source"))
        if key in visited:
            continue
        visited[key] = package
        for reference in package.get("dependencies", []):
            name, *qualifier = reference.split()
            # The standalone release builds the default CLI without IAMF.
            if package["name"] == "harletty" and name in {"iamf-codecs", "iamf-dec", "iamf-obu"}:
                continue
            choices = by_name[name]
            if qualifier:
                choices = [item for item in choices if item["version"] == qualifier[0]]
            if len(choices) != 1:
                raise ValueError(f"ambiguous dependency {reference}")
            pending.append(choices[0])
    return list(visited.values())


def prepare_harletty(notices, offline):
    project = next((TOOLS / "source/harletty-exact").iterdir())
    packages = harletty_packages(tomllib.loads((project / "Cargo.lock").read_text(encoding="utf-8")))
    def fetch(package):
        name = package["name"]
        version = package["version"]
        file = f"{name}-{version}.crate"
        old_cache = next((TOOLS / "cargo-home/registry/cache").glob(f"*/{file}"), None)
        url = f"https://static.crates.io/crates/{name}/{file}"
        expected = package["checksum"]
        path = old_cache if old_cache and sha(old_cache) == expected else acquire(url, expected, file, offline)
        notices_from_crate(path, notices)
        return path
    registry = [pkg for pkg in packages if pkg.get("source", "").startswith("registry+")]
    with ThreadPoolExecutor(max_workers=8) as pool:
        crates = list(pool.map(fetch, registry))
    git_sources = []
    for pin in sorted({pkg["source"] for pkg in packages if pkg.get("source", "").startswith("git+")}):
        repo = pin.split("?", 1)[0].removeprefix("git+https://github.com/")
        revision = pin.rsplit("#", 1)[1]
        spec = MANIFEST["tools"]["harletty"]
        if repo != "harletty/truehdd" or revision != spec["truehd_decoder_commit"]:
            raise ValueError(f"Unpinned Harletty Git dependency: {pin}")
        url = f"https://api.github.com/repos/{repo}/zipball/{revision}"
        path = acquire(url, spec["truehd_decoder_source_sha256"], f"truehdd-{revision}.zip", offline)
        with zipfile.ZipFile(path) as archive:
            if archive.testzip() is not None:
                raise ValueError(f"Corrupt immutable Git archive: {path}")
            for member in archive.namelist():
                parts = Path(member).parts
                if len(parts) == 2 and parts[1].upper().startswith(("LICENSE", "COPYING", "NOTICE")):
                    git_notice = notices / f"{repo.split('/')[-1]}-{revision}"
                    git_notice.mkdir(parents=True, exist_ok=True)
                    (git_notice / parts[1]).write_bytes(archive.read(member))
        git_sources.append(path)
    return crates, git_sources


def build_source_kit(payload, offline):
    source_dir = payload / "source"
    source_dir.mkdir()
    harletty_notices = payload / "tools/bin/harletty/notices/dependencies"
    crates, git_sources = prepare_harletty(harletty_notices, offline)
    supplement = ROOT / "packaging/notices/indicatif-log-bridge-0.2.3.txt"
    notice_dir = harletty_notices / supplement.stem
    notice_dir.mkdir(exist_ok=True)
    shutil.copy2(supplement, notice_dir / "NOTICE.txt")
    projects = MANIFEST["tools"]
    mkv = projects["mkvtoolnix"]
    mkv_source = acquire(mkv["source_url"], mkv["source_sha256"], "mkvtoolnix-103.0.tar.xz", offline)
    harletty = projects["harletty"]
    h_source = acquire(harletty["source_url"], harletty["source_sha256"],
                       f"harletty-{harletty['commit']}-source.zip", offline)
    encoder = projects["harlettizer"]
    encoder_source = acquire(encoder["url"], encoder["sha256"], f"harlettizer-{encoder['commit']}.zip", offline)
    vendor = TARGET / "package-vendor"
    cargo_config = TARGET / "package-vendor-config.toml"
    if not vendor.is_dir() or not cargo_config.is_file():
        raise RuntimeError("Prepare the vendored Cargo sources using the release instructions.")
    config = cargo_config.read_bytes()
    config = config.decode("utf-16" if config.startswith(b"\xff\xfe") else "utf-8-sig")
    config = config.replace('directory = "target/package-vendor"', 'directory = "vendor"')
    (source_dir / "REBUILD.md").write_bytes((ROOT / "packaging/REBUILD.md").read_bytes())
    with zipfile.ZipFile(source_dir / "xfold-sources.zip", "w", zipfile.ZIP_DEFLATED,
                         compresslevel=3, strict_timestamps=False) as archive:
        for path in (h_source, encoder_source, mkv_source, *git_sources):
            archive.write(path, f"upstream/{path.name}")
        for path in crates:
            archive.write(path, f"harletty-crates/{path.name}")
        archive.writestr("cargo-vendor-config.toml", config)
        archive.writestr("upstream/SHA256SUMS.txt", "".join(
            f"{sha(path)}  {path.name}\n" for path in (h_source, encoder_source, mkv_source, *git_sources)))
        archive_tree(archive, vendor, "vendor")
        for name in ("src", "ui", "scripts", "packaging"):
            archive_tree(archive, ROOT / name, f"xfold/{name}")
        for name in ("Cargo.lock", "Cargo.toml", "build.rs", "LICENSE", "README.md", "tools/manifest.toml", "docs/TOOL_NOTICES.md"):
            archive.write(ROOT / name, f"xfold/{name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--output-dir", type=Path, default=TARGET / "release-artifacts")
    args = parser.parse_args()
    version = subprocess.check_output([str(args.exe.resolve()), "--version"], text=True).strip().removeprefix("xfold ")
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError(f"Unexpected XFold version: {version}")
    compiler = TOOLS / "build/w64devkit/bin/makensis.exe"
    if sha(compiler) != "0ee6007dd310047ccb1ee73c79052db235cb8c31999b4dd325fd76c50bcfdd8f":
        raise ValueError("Installer compiler differs from the pinned w64devkit copy.")
    subprocess.run(["python", str(ROOT / "scripts/verify_bundle.py")], check=True)
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / f"XFold-{version}-windows-x64-setup.exe"
    sources = output_dir / f"XFold-{version}-sources.zip"
    temporary_output = output.with_suffix(".pending.exe")
    with tempfile.TemporaryDirectory(dir=TARGET, prefix="installer-") as scratch:
        payload = Path(scratch) / "payload"
        for tool in MANIFEST["tools"].values():
            directory = tool["directory"]
            shutil.copytree(TOOLS / "bin" / directory, payload / "tools/bin" / directory)
        shutil.copy2(args.exe, payload / "xfold.exe")
        shutil.copy2(ROOT / "packaging/update.ps1", payload / "update.ps1")
        shutil.copy2(TOOLS / "manifest.toml", payload / "tools/manifest.toml")
        shutil.copy2(ROOT / "docs/TOOL_NOTICES.md", payload / "THIRD-PARTY-NOTICES.md")
        shutil.copy2(ROOT / "README.md", payload / "README.md")
        shutil.copy2(ROOT / "LICENSE", payload / "LICENSE")
        runtime = TOOLS / "toolchain/share/doc/rust"
        shutil.copytree(runtime / "licenses", payload / "notices/rust/licenses")
        for name in ("COPYRIGHT.html", "COPYRIGHT-library.html"):
            shutil.copy2(runtime / name, payload / "notices/rust" / name)
        build_source_kit(payload, args.offline)
        # Cargo vendor retains the actual dependency license files. Make their
        # copies accessible alongside the binaries as well.
        for package in (TARGET / "package-vendor").iterdir():
            for path in package.iterdir():
                if path.is_file() and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "AUTHORS")):
                    dest = payload / "notices/cargo" / package.name
                    dest.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(path, dest / path.name)
        (payload / ".xfold-payload").write_text(version + "\n")
        hashes = {file.relative_to(payload).as_posix(): sha(file) for file in sorted(payload.rglob("*")) if file.is_file()}
        (payload / "bundle-sha256.json").write_text(json.dumps(hashes, indent=2) + "\n")
        subprocess.run([str(compiler), "-V2", f"-DVERSION={version}", f"-DPAYLOAD={payload}",
                        f"-DOUTPUT={temporary_output}", str(ROOT / "packaging/xfold.nsi")], check=True)
        source_copy = sources.with_suffix(".pending.zip")
        shutil.copyfile(payload / "source/xfold-sources.zip", source_copy)
        source_copy.replace(sources)
    temporary_output.replace(output)
    for artifact in (output, sources):
        artifact.with_suffix(".sha256").write_text(f"{sha(artifact)}  {artifact.name}\n", encoding="ascii")
    print(output, flush=True)
    print(sources, flush=True)


if __name__ == "__main__":
    main()
