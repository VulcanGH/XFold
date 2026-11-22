# Exact-bundle notices and rebuild obligations

This is the inventory for the public setup and the offline evaluation package.
Pins and acquisition commands live in [the manifest](../tools/manifest.toml).
Keep notices alongside the tools when installing; do not package only EXEs.

Public Setup requests and verifies the official MKVToolNix portable archive
from the MKVToolNix download site. The upstream archive supplies the
MKVToolNix notices and license files. The offline evaluation package also
embeds those binaries.

Codec work is provided by the [Harletty](https://github.com/harletty/harletty-bridge)
and [Harlettizer](https://github.com/harletty/harlettizer) projects. Harlettizer
credits FFmpeg and the EBU ADM Renderer in its upstream `LEGAL.md` and
`docs/provenance.md`. Matroska extraction and muxing use
[MKVToolNix](https://mkvtoolnix.download/) by Moritz Bunkus and contributors.

| Package | Exact-binary findings | Distribution work |
|---|---|---|
| Harletty standalone CLI v0.9.0, embedded commit `9556bce` | Upstream's exact-commit README and CLI Cargo.toml distinguish the Apache-2.0 CLI from GPL bridge plugins. The bundle uses the standalone CLI ZIP and includes notices collected from the exact lockfile and decoder fork. | Review those notices against the dependencies in the shipped build and retain the applicable copyright and attribution text. Apache terms do not impose a general corresponding-source delivery obligation. |
| Harlettizer commit `d3eceec` | GPL-3.0-or-later, Rust source built without optional Opus. Source LICENSE, LICENSES, LEGAL, provenance/encoding/DTS docs and Cargo files are copied alongside the binary. The source kit contains the pinned encoder source, its vendored dependencies and rebuild instructions. | Publish the matching corresponding source with the installer and retain the notices. Review the archive and build instructions against the exact released binary before publishing. |
| MKVToolNix 103.0 Windows x64 portable | GPL-2.0-or-later and additional bundled licences; original `doc/COPYING.txt`, `doc/licenses/`, data, translations, tools and all archive contents retained. CLI smoke mux/extract passed. | Retain copyright/licence notices and provide compliant exact-version corresponding source including relevant bundled-library materials and build scripts. Official source URL is pinned in manifest; source closure and clean-machine runtime companion tests are still required before distribution. |

The Harletty CLI directly uses `indicatif-log-bridge` 0.2.3 by djugei.
Its [published source](https://github.com/djugei/indicatif-log-bridge/tree/03ad701b5b4bf92c03cbcd9ba542b483f5c22f1c)
declares MIT in Cargo metadata and lacks a standalone license file.
The installer supplies that attribution and the standard MIT terms with
the dependency notices.

Harlettizer's packaged source and vendor directory passed a locked Windows
x64 Cargo check for `hz-cli` and `xtask` with an empty Cargo cache.

Harlettizer upstream LEGAL.md records FFmpeg-derived LGPL material used under
GPLv3, Apache material, BSD renderer material and attribution requirements. The
exact source's `LICENSES/` is preserved, including the libopus notice even though
this build does not enable Opus. GNU Rust runtime and linker runtime attribution
must be included in the final dependency review; build-only w64devkit itself is
not part of the installed tool bundle.

The build uses unmodified pinned source and Cargo.lock, release optimisation,
`x86_64-pc-windows-gnu`, Rust 1.92.0 and `-C link-self-contained=yes`. Runtime
libraries are resolved from Rust's matched self-contained directory. GCC/dlltool
come from verified w64devkit 2.11.0. The reference hashes are recorded in
`tools/bundle-lock.json`. Rebuilding in another path/toolchain
can change binary bytes; qualify any new binary instead of silently adopting it.

The installer includes Rust runtime and dependency notices, the locked encoder
dependencies, and the Harletty CLI dependency archives and decoder fork.
The offline evaluation package also contains the official MKVToolNix 103.0
source archive. The zlib-compressed installer stub
comes from the manifest-pinned w64devkit NSIS compiler. See the
[NSIS license reference](https://nsis.sourceforge.io/License).

The MKVToolNix archive contains Windows
packaging recipes and bundled project sources but refers to an external MXE
environment. Its exact official Windows dependency inputs have not been
established. The v103.0 file `packaging/windows/setup_cross_compilation_env.sh`
clones the maintainer's MXE `master` branch without a release commit pin.
The Harletty exact-build dependency review also remains open.

The codec review is separate from software license notices. Harlettizer's
upstream legal notes discuss potential patent obligations for the formats
it implements and limits on claims about approval or certification. Public
binary distribution needs a review of which rights, if any, apply to the
features and jurisdictions involved. Use format names descriptively.

TrueHD's protection-field key is **not supplied by upstream or this bundle**.
The qualification output explicitly uses an unkeyed constant. See the staged
`notices/encode.md`, section “The protection field, and the key that fills it”.
Software decoding and metadata detection passed; no licensed receiver acceptance,
certification, trademark or playback guarantee follows from these tests.
