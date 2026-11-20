# Phase 0 evidence — 20 November 2026

**Result:** software exit gate passed using the staged Windows x64 bundle on an
original synthetic standard-profile DTS:X fixture. No app code was implemented.

Reproduce from the repository root with Python 3.11+, curl and 7-Zip available:

```powershell
python scripts/stage_tools.py --build
python scripts/qualify_tools.py
python scripts/verify_bundle.py
```

Independent ffprobe/ffmpeg must be on PATH for qualification. They are not bundled
runtime tools. For cached acquisition and rebuild use
`python scripts/stage_tools.py --build --offline`; a cold offline machine needs
the acquired toolchain, source/dependency cache and archives transferred first.
Normal installed app use will not need Rust, Python, FFmpeg or build caches.
The offline rebuild changed executable bytes; bit-for-bit build determinism is
not established. The final rebuilt binary was requalified, and its SHA-256 is
pinned in both the manifest and qualification report. Future changed rebuilds
need requalification and an explicit manifest-pin update before verification passes.

- `qualification.json`: exact argv, exit statuses, checks, duration and media hashes.
- `*.stdout.txt` / `*.stderr.txt`: real version/help/diagnostic/progress/probe samples
  for later adapters, including both successful conversions and metadata decode.
- `bundle.json`: every staged binary, notice and companion-file SHA-256, plus lockfile hash.
- `verification.json`: inventory and minimal-PATH version launches.
- `rust-components.json`: official Rust component URLs and verified published hashes.
- `harlettizer-build.txt`: successful unmodified-source Windows build.
- `harlettizer-workspace-licences.json`: resolved workspace dependency superset.
- `harletty-release-assets.html` / `mkvtoolnix-sha256sums.txt`: saved published integrity evidence.

The synthetic source master contains 48 kHz tones, four moving objects and a
7.1.2 bed. The fixture DTS:X is a rendered fixed-height 7.1.4 representation;
it does not establish support for every dynamic-object DTS:X profile. Regenerate
it with the pinned upstream `xtask make-master` command captured in qualification.
No downloaded commercial media was used. Audio and generated media are ignored.

The raw DTS frame length is 512 samples. Both output decodes contain 96,256
samples (2.005333 s), within one frame of the original 96,000 samples. ffprobe
identifies the DTS export as DTS-HD MA, 7.1, 48 kHz and 24-bit; TrueHD as
Dolby TrueHD + Dolby Atmos with a 7.1 compatible presentation at 48 kHz. Harletty
reports Atmos metadata and recovers eleven spatial objects in a full decode.
MKVToolNix's mux/extract smoke checks retain both elementary payloads byte for byte.

Limits: receiver playback and clean-machine installation untested; TrueHD's
protection word is unkeyed. Long programmes, other DTS:X profiles and audio
quality/listening comparisons remain unverified. Distribution source and
dependency notice closure remain open for Phase 5. These limits do not establish
a failure of the measured synthetic software exports and must travel with any
claim based on this evidence.
