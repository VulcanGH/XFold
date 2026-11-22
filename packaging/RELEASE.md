# XFold Windows releases

Public Windows x64 setup includes the XFold host, its loopback browser UI,
and the pinned Harletty CLI and Harlettizer. Setup downloads the official
MKVToolNix portable archive and checks its hash before installation. The
application runs offline after setup.

## Build the candidate

Use Python 3.12 or later, curl, 7-Zip, FFmpeg, and ffprobe on a Windows x64
machine. From the repository root, stage the [pinned tools](../tools/manifest.toml),
qualify the rebuilt encoder, and vendor the locked dependencies.

```powershell
python scripts/stage_tools.py --build
python scripts/qualify_tools.py
$env:CARGO_HOME = "$PWD\tools\cargo-home"
$encoder = Get-ChildItem tools/source/harlettizer -Directory | Select-Object -First 1
& tools/toolchain/bin/cargo.exe vendor --locked --manifest-path (Join-Path $encoder.FullName Cargo.toml) --sync Cargo.toml target/package-vendor > target/package-vendor-config.toml
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cargo-xfold.ps1 build --release --locked --offline --target x86_64-pc-windows-gnu
python scripts/package_xfold.py --exe target/x86_64-pc-windows-gnu/release/xfold.exe --evidence-dir target/tool-evidence --download-mkvtoolnix
```

The output contains `XFold-<version>-windows-x64-setup.exe`,
`XFold-<version>-sources.zip`, and a SHA-256 sidecar for each. The standalone
ZIP is the same archive included in Setup. For a signed installer, regenerate
its checksum **after** signing and verify the final signature and checksum
before uploading. Test install, upgrade, conversion and uninstall on a clean
Windows x64 machine from an elevated terminal with
`scripts/test_installer.py` and the staged fixtures.

## GitHub Actions

Run **Actions → XFold release draft** on the default branch with the desired
version in `Cargo.toml`. GitHub builds on Windows x64, qualifies the encoder
built from the pinned source, then verifies setup, offline conversions, and
uninstall. The draft points to the tested commit. Clear **Create a GitHub
release draft after verification** for a build and test run. Select
**Refresh the existing unpublished draft for this version** to replace its
assets after the new files pass verification. Publish from the draft after review.

GitHub builds are unsigned until Windows signing is configured. Setup records
the checksum of the encoder actually packaged. Test logs stay in the runner
workspace, and the Actions run reports the outcome.

## In-app updates

The update feed is `VulcanGH/XFold`. A user checks for updates in **Settings**
and chooses **Update and restart** once the workspace is idle. Windows asks
for administrator approval before Setup starts. XFold checks
the release asset digest, validates the installer's Windows Authenticode
signature and pins the SHA-256 fingerprint of the signing certificate's DER
bytes. A build without the pin keeps the update controls disabled.

1. Set `XFOLD_UPDATE_CERT_SHA256` in the build environment to the 64-character
   SHA-256 digest of the trusted public certificate in DER format. That pin
   must match the certificate used to sign Setup. Keep the private signing
   key in the signing service or protected certificate store.
2. Build and sign the installer, then recalculate its SHA-256 sidecar. On a
   clean Windows test system, verify **Update and restart** across two signed
   versions using the same certificate. Verify that settings survive, and
   that an unsigned installer or a different signer is rejected.
3. Tag the source as `v<version>` and attach the installer, source ZIP and
   both checksums to a GitHub draft. The updater uses the public latest
   stable release and its GitHub-provided asset digest. Drafts and pre-releases
   are for manual testing. Keep the canonical project and asset names.
4. Review the complete draft before publishing. When changing the pinned
   certificate, distribute a manually installed update with the new pin.

## Before publishing

- Confirm the public GitHub repository. XFold's own code is MIT-licensed.
- Use the verified official MKVToolNix download during setup for public
  releases. See the [third-party notices](../docs/TOOL_NOTICES.md) for the
  codec tools and the offline evaluation package.
- Document the clean Windows install and signed update results. TrueHD receiver
  playback remains unverified.
