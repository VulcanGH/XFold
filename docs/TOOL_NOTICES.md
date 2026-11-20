# Exact-bundle notices and rebuild obligations

This is a technical distribution inventory, not a completed release review.
Pins and acquisition commands live in [the manifest](../tools/manifest.toml).
Keep notices alongside the tools when installing; do not package only EXEs.

| Package | Exact-binary findings | Distribution work |
|---|---|---|
| Harletty standalone CLI v0.9.0, embedded commit `9556bce` | Upstream's exact-commit README and CLI Cargo.toml distinguish Apache-2.0 CLI from GPL bridge plugins. Only the CLI ZIP is used. ZIP contains just `harletty.exe`; fetched source supplies LICENSE, README, Cargo files. | Retain Apache licence, copyright/attribution and applicable dependency notices. Complete transitive notice inventory from its exact lockfile before shipping. Apache terms do not impose a general corresponding-source delivery obligation. No GPL bridge DLL is included. |
| Harlettizer commit `d3eceec` | GPL-3.0-or-later, Rust source built without optional Opus. Source LICENSE, LICENSES, LEGAL, provenance/encoding/DTS docs and Cargo files copied alongside binary. Resolved workspace licence inventory and available dependency texts also retained; optional/test packages in this superset are not necessarily linked. | Retain notices and corresponding source for the exact binary, including used dependencies, build/install scripts and toolchain details. Source ZIP and Cargo cache remain local, ignored. A public upstream link alone is not a completed source-delivery arrangement. Assemble and validate the complete source/rebuild distribution in Phase 5. |
| MKVToolNix 103.0 Windows x64 portable | GPL-2.0-or-later and additional bundled licences; original `doc/COPYING.txt`, `doc/licenses/`, data, translations, tools and all archive contents retained. CLI smoke mux/extract passed. | Retain copyright/licence notices and provide compliant exact-version corresponding source including relevant bundled-library materials and build scripts. Official source URL is pinned in manifest; source closure and clean-machine runtime companion tests are still required before distribution. |

Harlettizer upstream LEGAL.md records FFmpeg-derived LGPL material used under
GPLv3, Apache material, BSD renderer material and attribution requirements. The
exact source's `LICENSES/` is preserved, including the libopus notice even though
this build does not enable Opus. GNU Rust runtime and linker runtime attribution
must be included in the final dependency review; build-only w64devkit itself is
not part of the installed tool bundle.

The build uses unmodified pinned source and Cargo.lock, release optimisation,
`x86_64-pc-windows-gnu`, Rust 1.92.0 and `-C link-self-contained=yes`. Runtime
libraries are resolved from Rust's matched self-contained directory. GCC/dlltool
come from verified w64devkit 2.11.0. The executable SHA-256 and every staged file
hash are recorded in committed evidence. Rebuilding in another path/toolchain
can change binary bytes; qualify any new binary instead of silently adopting it.

TrueHD's protection-field key is **not supplied by upstream or this bundle**.
The qualification output explicitly uses an unkeyed constant. See the staged
`notices/encode.md`, section “The protection field, and the key that fills it”.
Software decoding and metadata detection passed; no licensed receiver acceptance,
certification, trademark or playback guarantee follows from these tests.
