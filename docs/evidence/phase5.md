# Phase 5 offline installer

The local Windows x64 candidate is
`target/release-artifacts/XFold-0.2.0-windows-x64-setup.exe`. It installs per
user, adds Start menu shortcuts and an uninstall entry, and includes the
pinned host and tools, licence notices and source kit. The installer is
unsigned. SHA-256:
`eff07c7696bb577a3807117bd7377887c9e76c0b6af6ba5c1d4af310968327bc`.

On Windows 11 Pro x64 (build 26300), setup was tested in a fresh directory
and upgraded from a 0.1.0 host fixture. Checks included the
installed loopback UI, raw and MKA audio, full-MKV workflows with only Windows
System32 on PATH, and verification of all 751 installed file hashes. Setup refused
to replace a running instance. Uninstall preserved user state and exports.

No clean Windows VM was available. A fresh-directory test on this host does not
establish clean-machine compatibility. Receiver playback and the exact source
closure of the upstream MKVToolNix Windows build remain open. The candidate
has not been cleared for public distribution.

The source kit was extracted from setup and checked with an empty Cargo cache.
A locked, offline `cargo check` succeeded for the host and Harlettizer CLI.

Build the release executable first. With the Phase 0 tools and sources available:

```powershell
$env:CARGO_HOME = "$PWD\tools\cargo-home"
& tools/toolchain/bin/cargo.exe vendor --locked --manifest-path tools/source/harlettizer/harletty-harlettizer-d3eceec/Cargo.toml --sync Cargo.toml target/package-vendor > target/package-vendor-config.toml
python scripts/package_xfold.py --exe target/x86_64-pc-windows-gnu/release/xfold.exe
python scripts/test_installer.py target/release-artifacts/XFold-0.2.0-windows-x64-setup.exe
```

After acquisition, `package_xfold.py --offline` uses cached pins and performs
no downloads. The installer writes a SHA-256 sidecar and records individual
installed file hashes in `bundle-sha256.json`.
