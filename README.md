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

Get the Windows x64 installer from [GitHub Releases](https://github.com/VulcanGH/XFold/releases).

Run the installer on Windows 10 version 1809 or later, then open **XFold**
from the Start menu. Setup requests administrator approval and defaults
to `C:\Program Files\XFold`. Setup bundles Harletty and Harlettizer and
downloads and verifies the pinned MKVToolNix build during setup. If the
download fails, save the official archive beside the installer and run Setup
again. Conversions run offline, and source files stay in place.
[MKVToolNix 103.0 for Windows x64](https://mkvtoolnix.download/windows/releases/103.0/mkvtoolnix-64-bit-103.0.7z)

If you have an earlier per-user preview installed, uninstall it from Windows
Installed apps before installing the machine-wide package. Your XFold
preferences and exported files are stored separately.

Choose a source in **Files & queue**. For MKV input, check the selected DTS
track. Choose a preset, format, and destination in **Export setup**. Start the
conversion immediately or add more files to the queue and start them together.
XFold converts queued files one at a time.

The blue X in the Windows notification area reopens the dashboard. Hover
over it for a tooltip, or right-click for **Open XFold** and **Quit XFold**.
Finish or remove queued jobs before quitting.

Set **Settings → Browser port** to keep a fixed local address across launches,
or leave it blank to choose an available port automatically. If a fixed port
is unavailable at launch, XFold uses an automatic port and reports it in Settings.

Signed, update-enabled installations can use **Settings → Check for updates**,
then **Update and restart** when the workspace is idle. For a manual upgrade or
uninstall, close XFold first.

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

XFold opens its local conversion screen in your default browser. Use **Select
file** to open a Windows dialog attached to the active XFold browser window.
XFold reads the selected file in place and suggests an export beside it.
For MKV input, check the selected DTS track and choose an output format.
Use **Save as** to change the destination before starting conversion.
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
