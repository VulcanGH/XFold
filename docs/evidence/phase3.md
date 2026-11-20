# Phase 3 MKV input and standalone audio verification

The Windows x64 release build passed the Phase 3 tests on a synthetic MKV with
one video track and three named audio tracks. The selected DTS:X track had a
nonzero MKVToolNix ID distinct from its Matroska track number. Paths and track
names contained spaces and Unicode.

- Track listing returned audio tracks with their IDs, codecs, language, and names.
- Missing IDs, video, TrueHD, and a plain DTS-HD MA track were rejected without
  publishing a file.
- Extracting the selected track reproduced the original DTS:X fixture byte for
  byte, and Harletty identified its spatial profile.
- Both presets produced their expected 7.1, 48 kHz, 24-bit profiles in raw and
  standalone MKA output. Each MKA contained one audio track and its extracted
  payload matched the corresponding raw export byte for byte.
- The source MKV hash remained unchanged. The browser retained the selected
  track and MKA format through reload and published a standalone MKA export.

With the Phase 0 fixture and tools staged and FFmpeg and ffprobe on PATH, build
the release binary and run `python scripts/test_mkv_audio.py`. To also exercise
the browser, build the Rust test executable as in [Phase 2](phase2.md), install
Playwright, and pass that executable path to `test_mkv_audio.py`.

The browser test uses fixture paths in place of interaction with native dialogs.
Expanded DTS:X profile coverage, receiver playback, and feature-length A/V sync
remain unverified. The standalone export begins at its own audio timeline.
