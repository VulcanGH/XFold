# Source and rebuild materials

The installer includes the pinned upstream Harlettizer, Harletty and MKVToolNix
source archives, the XFold source and installer recipe, and the Cargo dependency
sources used for XFold and Harlettizer. The archive also contains the locked
Harletty CLI crates and its exact TrueHD decoder fork.

Extract `xfold-sources.zip`. Apply `cargo-vendor-config.toml` as Cargo config
with the vendor directory resolved from the extracted archive root. Preserve
existing upstream config options when merging. Build XFold with Rust 1.92.0
for `x86_64-pc-windows-gnu`. Build Harlettizer with the command and toolchain
listed in `xfold/tools/manifest.toml`. All code dependencies for these two
builds are in `vendor/`. The manifest pins the toolchain and linker.

MKVToolNix's release archive includes its Windows build and packaging recipes
under `packaging/windows/`. These reference the upstream MXE environment and
external library sources. The exact MXE dependency closure for the official
Windows binaries has not been verified. The Harletty build also depends on the
upstream workspace and release environment described by its own workflow files.
The source kit is intended for local evaluation and rebuild review.

See [RELEASE.md](RELEASE.md) for creating the installer and the matching
standalone source archive.
