# XFold

XFold converts DTS:X audio on Windows x64 from a raw stream or a selected MKV
audio track. Export DTS-HD Master Audio 7.1 at 48 kHz and 24-bit or TrueHD with
spatial metadata. For MKV input, add the converted track or replace the selected
track in a new copy. Both presets also support raw audio and standalone MKA.

On Windows 10 or later, run the local candidate installer in
`target/release-artifacts/` and open **XFold** from the Start menu. Close a
running installed copy before upgrading or uninstalling. See the
[installer evidence](docs/evidence/phase5.md) for validation and release limits.

With the Phase 0 tools staged, build from the repository root and open XFold:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cargo-xfold.ps1 build --release --locked --offline --target x86_64-pc-windows-gnu
$xfold = '.\target\x86_64-pc-windows-gnu\release\xfold.exe'
& $xfold
```

XFold opens its local conversion screen in your default browser. Choose a
source and destination through the Windows file pickers. For MKV input, select
the DTS:X audio track. Choose raw audio or Matroska audio as the output format.
Keep the host running
while you use the browser. To open the launcher URL yourself, run
`& $xfold serve --no-browser`. Treat the printed URL as private to that session.

The command line remains available:

```powershell
& $xfold convert dts-hd-ma input.dts output.dts
& $xfold convert true-hd input.dts output.thd
& $xfold tracks movie.mkv
& $xfold convert dts-hd-ma --track 3 movie.mkv output.mka
& $xfold convert dts-hd-ma --track 3 --mkv-mode add movie.mkv added.mkv
& $xfold convert true-hd --track 3 --mkv-mode replace movie.mkv replaced.mkv
& $xfold status
```

Use the `id` reported by `tracks` for your source. The example uses track ID 3.

Use Ctrl+C to cancel. The destination folder must already exist. XFold keeps
status and codec logs under `%LOCALAPPDATA%\XFold`. Pass
`--state-dir DIRECTORY` before the subcommand to select a different state folder.

See [the build plan](docs/BUILD_PLAN.md) for phase status and the next gate,
[the tool manifest](tools/manifest.toml) for pins and layout, and
[full-MKV test evidence](docs/evidence/phase4.md) for remux behavior and supported timing.

TrueHD's protection field is unkeyed, and receiver playback remains unverified.
