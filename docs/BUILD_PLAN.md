# XFold build plan

Updated: 20 November 2026. Repository: `xfold`; future Windows x64 host: `xfold.exe`.

## Scope and decisions

Work one phase at a time and stop at its gate. This first run performs **Phase 0 only**.
There is no app implementation yet. The intended host is one Rust crate with tool
adapters (resolution, arguments, parsing), a job runner (sequencing, cancellation,
workspaces, publication), and a web layer (HTTP and compact TypeScript/CSS UI).
Split crates only when a concrete dependency or testing need warrants it.

Primary output renders DTS:X objects/heights into DTS-HD Master Audio 7.1,
48 kHz, 24-bit. The second preset encodes TrueHD with spatial metadata. Standalone
audio precedes MKV extraction and new-file muxing; source files must stay intact.
One active job, status polling, Windows Job Objects, no-overwrite publication,
interruption recovery and authenticated loopback access belong to later phases.

Tools resolve relative to `tools/bin/` in development and the same layout under an
app-private installed root. Keep the entire MKVToolNix archive tree, including its
`mkvtoolnix/` top directory. Do not resolve codec tools from PATH. ffprobe/ffmpeg
are independent qualification tools only and are not app runtime dependencies.

## Phase status

| Phase | Status | Exit gate / next work |
|---|---|---|
| 0: tools | **Software gate passed on synthetic fixture** | All packages staged; both exports independently decode and identify correctly. Limits below. |
| 1: audio runner | Not started | Both presets; cancellation, corrupt input, disk shortage, conflicts, restart recovery; no partial final output. |
| 2: local UI | Not started | Conversion/reload/cancel/retry, keyboard/status accessibility, session and file-scope protections. |
| 3: MKV input / audio export | Not started | Correct track, preserved DTS:X extraction, both presets, spaces/Unicode, MKA exports. |
| 4: new MKV output | Not started | Add then replace; payload/structure preservation and feature-length A/V timing. |
| 5: offline installer | Not started | Distribution review, source/notices closure, clean install/offline/upgrade/uninstall tests. |

## Phase 0 result

Pins, official URLs, archive digests, binary hashes, commands and layout are in
[tools/manifest.toml](../tools/manifest.toml). Release ZIP/7z SHA-256 values match
published integrity data. Harlettizer v0.1.0 was rejected: its actual Windows CLI
has `encode`, but no `dts`. Built unmodified commit
`d3eceec774780e9b1fb844e33ed01a53f7cf7e39` with repository-local Rust 1.92.0 GNU,
its checked-in lockfile, and the pinned w64devkit linker utilities. No global
toolchain installation or PATH changes. Harletty's embedded commit is
`9556bcedfd6e71461d49fd3f03fcfdf87351b67f`; its exact source was fetched because
the current release-tag source resolves differently.

A locally generated two-second tone master (7.1.2 bed, four moving objects) was
rendered to standard-profile DTS:X 7.1.4 as the rights-cleared input. Harletty
decoded it to a master, then the staged encoder produced both required exports.
Independent FFmpeg/ffprobe verified DTS-HD MA, 7.1, 48 kHz, 24-bit and TrueHD with
Atmos metadata, 7.1 compatible presentation, 48 kHz. Harletty independently decoded
the TrueHD output and recovered eleven spatial objects. Both decode to 2.005333 s;
the difference from 2 s is whole-frame padding in raw DTS. Both streams survived
MKVToolNix mux/extract byte for byte. Offline restaging/rebuild and hash inventory
verification passed. All four CLIs launch with only Windows System32 on PATH.

Evidence and reproducible commands: [Phase 0 report](evidence/phase0/README.md),
[qualification](evidence/phase0/qualification.json),
[bundle inventory](evidence/phase0/bundle.json),
[licence notes](TOOL_NOTICES.md). Fixture media, tools and caches are ignored.

## Limits and next gate

No blocker remains for the **synthetic software gate**. Receiver playback is
unverified. TrueHD Evolution protection is unkeyed (upstream emits a constant
without a separately supplied key); no key is bundled or invented. This output
parses and decodes in the tested software, but receiver acceptance is unproven.
Commercial/dynamic-object DTS:X variants and long programmes are untested.
Clean-machine dependency minimisation and distribution-source/notice closure
remain Phase 5 gates; retaining the full portable archive avoids premature pruning.

**Stop here.** On a subsequent named Phase 1 run, first verify the staged inventory,
then create the single Rust host crate and implement only the audio runner. Use
`dts --layout 7.1 --bits 24`; qualification's TrueHD arguments are
`encode --cluster 12 --fold-depth 24`. Preserve diagnostics about unkeyed protection
and do not promise receiver playback. Harletty `info --json` is parseable; its
`--log-format json` stderr contains unquoted timestamps in this exact build and
must be retained as text instead of assumed to be valid JSON.
