# XFold build plan

Updated 20 November 2026. Windows x64 host executable `xfold.exe`.

## Scope and decisions

The host is one Rust crate with tool
adapters (resolution, arguments, parsing), a job runner (sequencing, cancellation,
workspaces, publication), and a web layer (HTTP and compact TypeScript/CSS UI).
Split crates only when a concrete dependency or testing need warrants it.

Primary output renders DTS:X objects/heights into DTS-HD Master Audio 7.1,
48 kHz, 24-bit. The second preset encodes TrueHD with spatial metadata. Standalone
audio precedes MKV extraction and new-file muxing; source files must stay intact.
Phase 1 uses Windows Job Objects, state snapshots, and publication that refuses
to overwrite an existing file. Phase 2 adds authenticated loopback access.

Tools resolve relative to `tools/bin/` in development and the same layout under an
app-private installed root. Keep the entire MKVToolNix archive tree, including its
`mkvtoolnix/` top directory. Do not resolve codec tools from PATH. ffprobe/ffmpeg
are independent qualification tools only and are not app runtime dependencies.

## Phase status

| Phase | Status | Exit gate / next work |
|---|---|---|
| 0: tools | **Software gate passed on synthetic fixture** | All packages staged; both exports independently decode and identify correctly. Limits below. |
| 1: audio runner | **Passed** | Both presets verified on Windows, including cancellation, corrupt input, disk shortage, conflicts and restart recovery. |
| 2: local UI | Implemented, manual check deferred | Browser conversion/reload/cancel/retry, keyboard/status accessibility and session protections passed. Native dialogs opened. Interactive selection was deferred. |
| 3: MKV input / audio export | **Passed** | Explicit track selection, byte-identical DTS:X extraction and both presets as raw or MKA passed on Windows with spaces and Unicode in paths. |
| 4: new MKV output | **Passed on synthetic fixtures** | Both remux modes preserved payloads, metadata and timing. DTS-HD MA passed the feature-length A/V timing check in both modes. |
| 5: offline installer | Local candidate | Fresh-directory install, installed workflows, upgrade and uninstall checked on this host. Clean-machine and public distribution gates remain open. |

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

## Phase 1 through 5 results

The CLI accepts raw 48 kHz DTS:X and runs one conversion per state directory.
Each tool starts inside a Windows Job Object. Attempts persist their current stage
and codec logs, and report interruption on the next status query after a forced
exit. XFold estimates space from source size, checks again using the actual master
size, and monitors free space while tools run. It validates the codec profile
and decodes the export before publishing it by rename on the destination volume.
Both presets and the failure paths passed the [Windows runner tests](evidence/phase1.md).

The Windows host embeds a TypeScript/CSS interface and opens it on loopback.
Its per-launch token is passed in the URL fragment and retained in the browser
tab for reloads. Requests verify Host, token and Origin. Paths are chosen through
Windows dialogs and retained by the host. The browser API does not take paths.
The UI shows the current stage, elapsed time, reported encode progress and
on-demand log tails. See the [browser and API evidence](evidence/phase2.md).

For MKV input, XFold lists audio tracks using MKVToolNix IDs and extracts the
selected stream without re-encoding. Harletty checks the extracted audio for
48 kHz DTS:X. For MKA output, XFold muxes the verified raw stream and checks that
extracting the single output track reproduces the encoded bytes before publication.
See the [Phase 3 verification](evidence/phase3.md).

Full MKV output retains the selected track's offset and checks the converted
track and retained timing before publication. Add keeps the original track and
its default flag. Replace places the exported audio in the original position.
See [Phase 4 evidence](evidence/phase4.md).

The per-user Windows x64 installer includes the pinned offline bundle, notices
and source kit. See [Phase 5 evidence](evidence/phase5.md).

Receiver playback is
unverified. TrueHD Evolution protection is unkeyed (upstream emits a constant
without a separately supplied key); no key is bundled or invented. This output
parses and decodes in the tested software, but receiver acceptance is unproven.
Commercial and dynamic-object DTS:X coverage is limited to the profiles tested.
Interactive Windows file selection, independent clean-machine installation,
hardware playback and public distribution sign-off remain open.
