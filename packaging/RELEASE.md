# XFold Windows releases

The offline Windows x64 setup includes the XFold host, its loopback browser UI,
the pinned Harletty CLI, Harlettizer and the complete MKVToolNix portable tree.
The Rust compiler and other build tools are needed when rebuilding.

## Build the candidate

Stage and verify the pinned tools and sources using [the tool manifest](../tools/manifest.toml)
and [Phase 0 staging instructions](../docs/evidence/phase0/README.md).
Generate the Cargo vendor directory as described in [the installer evidence](../docs/evidence/phase5.md).
Then build and package from the repository root.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cargo-xfold.ps1 build --release --locked --offline --target x86_64-pc-windows-gnu
python scripts/package_xfold.py --offline --exe target/x86_64-pc-windows-gnu/release/xfold.exe
```

The output contains `XFold-<version>-windows-x64-setup.exe`,
`XFold-<version>-sources.zip`, and a SHA-256 sidecar for each. The standalone
ZIP is the same archive included in Setup. For a signed installer, regenerate
its checksum **after** signing and verify the final signature and checksum
before uploading. Test install, upgrade, conversion and uninstall on a clean
Windows x64 machine from an elevated terminal with
`scripts/test_installer.py` and the staged fixtures.

## GitHub and in-app updates

The update feed is `VulcanGH/xfold`. A user checks for updates in **Settings**
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
- Resolve the exact Windows MKVToolNix build dependencies and complete the
  codec redistribution review described in [third-party notices](../docs/TOOL_NOTICES.md).
  A fresh package still contains the current local evaluation bundle.
- Document the clean Windows install and signed update results. TrueHD receiver
  playback remains unverified.
