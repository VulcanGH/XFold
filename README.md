<img src="ui/xfold.png" width="64" height="64" alt="XFold logo">

# XFold

XFold converts DTS:X audio on Windows x64 from a raw stream or a selected MKV
audio track. Export DTS-HD Master Audio 7.1 at 48 kHz and 24-bit or TrueHD with
spatial metadata. For MKV input, add the converted track or replace the selected
track in a new copy. Both presets also support raw audio and standalone MKA.
MKA is the default for MKV input and preserves the selected track's language
tag. Full MKV exports keep it too. Raw `.dts` and `.thd` files have no Matroska
track metadata.

TrueHD uses 12 dB of headroom for its compatible speaker mixes. If the encoder
cannot produce those mixes for the full track, XFold stops before saving it.

## Install and use

**[Download XFold for Windows x64](https://github.com/VulcanGH/XFold/releases/download/v1.0.0/XFold-1.0.0-windows-x64-setup.exe)** · [Release notes and checksums](https://github.com/VulcanGH/XFold/releases/latest)

Run Setup on Windows 10 version 1809 or later and approve the administrator
prompt. Setup installs to `C:\Program Files\XFold` by default and downloads
and verifies MKVToolNix once. Conversions work offline after setup.

1. Open **XFold** from the Start menu. The dashboard opens in your default browser.
2. In **Files & queue**, click **Select file** and choose a `.dts` or `.mkv` file.
   For MKV input, XFold selects the first DTS track by default. You can choose
   a different track from **MKV audio track**.
3. In **Export setup**, choose the output preset and format. XFold suggests
   saving beside the source file. Use **Save as** to change the destination.
4. Click **Start conversion** for a single file. For a batch, click **Add to queue**,
   then use **Add another file** to configure and queue each remaining source.
   Click **Start queue** to convert them in order.

Click the blue X in the Windows notification area to reopen the dashboard.
Right-click it and choose **Quit XFold** before upgrading or uninstalling.
To use a fixed local port, set **Browser port** in **Settings**.

If Setup cannot download MKVToolNix, save the official archive beside the
installer and run Setup again.
[MKVToolNix 103.0 for Windows x64](https://mkvtoolnix.download/windows/releases/103.0/mkvtoolnix-64-bit-103.0.7z)

If you have an older per-user preview, uninstall it from Windows **Installed apps**
before running Setup. Your preferences and exports are stored separately.
For future updates from the unsigned v1.0.0 release, download and run the new
installer from GitHub Releases.

## Build from source

With Python 3.12 or later, curl, and 7-Zip installed, run these commands from
the repository root. The first build downloads pinned tool archives and Cargo
dependencies.

```powershell
python scripts/stage_tools.py --build
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cargo-xfold.ps1 build --release --locked --target x86_64-pc-windows-gnu
$xfold = '.\target\x86_64-pc-windows-gnu\release\xfold.exe'
& $xfold
```

To open the launcher URL yourself, run
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

See [the tool manifest](tools/manifest.toml) for pinned inputs and layout.
See [the release guide](packaging/RELEASE.md) for installer packaging and update signing.

TrueHD's protection field is unkeyed, and receiver playback remains unverified.

## License

XFold's own code is [MIT-licensed](LICENSE). The bundled tools and their
dependencies are covered by their [respective licenses and notices](docs/TOOL_NOTICES.md).

## Credits

- The [Harletty project](https://github.com/harletty/harletty-bridge) provides
  the standalone CLI that decodes DTS:X.
- The [Harlettizer project](https://github.com/harletty/harlettizer) provides
  the encoder used for DTS-HD Master Audio and TrueHD.
- Moritz Bunkus and the [MKVToolNix contributors](https://mkvtoolnix.download/)
  provide the tools that extract and mux Matroska tracks.

The installer includes `THIRD-PARTY-NOTICES.md`, the upstream license
files, and the matching source kit.
