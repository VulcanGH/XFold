# XFold

Windows x64 DTS:X conversion project. Phase 0 has staged and qualified the codec
and container tools on a synthetic fixture. The app host is not implemented yet.

See [the build plan](docs/BUILD_PLAN.md) for phase status and the next gate,
[the tool manifest](tools/manifest.toml) for pins and layout, and
[qualification evidence](docs/evidence/phase0/README.md) for reproducible checks.

Work one named phase at a time. The intended host is `xfold.exe`, one Rust crate
serving a compact TypeScript/CSS UI on authenticated loopback. Primary output is
DTS-HD Master Audio 7.1 at 48 kHz / 24-bit; TrueHD with spatial metadata is the
second preset. Receiver acceptance is not yet verified.
