# Phase 4 full-MKV verification

XFold captures source timestamps with MKVToolNix before audio extraction. The
new MKV retains the timestamp scale and the converted track receives the source
audio offset. Before publication the runner checks retained track order and
identified metadata, timestamps, chapter XML, the converted audio bytes and
the converted track's codec and flags. Retained timestamps may change by up to
one tick of the source timestamp scale as Matroska blocks are re-laced.

Full MKV output supports continuous 48 kHz DTS:X with 512-sample packets and
an offset representable in whole milliseconds within the container precision.
Linked segments, timestamp gaps, incompatible fractional offsets and unsafe
replacements are rejected. Use a standalone export for unsupported timing.
Add retains original audio and flags. Replace keeps its position, language,
name and playback flags. Use Add when the selected track has custom tags
or the MKV contains track-specific chapter references.

The short synthetic fixture contained video, DTS:X with a 125 ms start
offset, subtitles, a chapter, an attachment, and global and retained track
tags. DTS-HD MA add and spatial TrueHD replace passed. Independent stream
hashes, chapter and tag XML, attachment bytes, track order and default flags
matched expectations. A deliberate audio gap was rejected without publication.
Both modes also passed in the browser using fixture paths from the test picker.

The feature-length DTS-HD MA runs passed both Add and Replace on a synthetic
4,812.917-second fixture. MKVToolNix measured the same converted audio
timestamps in both outputs at the start (125.000 ms), middle and end.
The retained video timestamps were unchanged at those points, and the
retained payload and metadata checks passed. The final 0.2.0 installer
passed the short fixture in both modes. Feature-length TrueHD playback
and receiver decoding remain unverified.

Run `python scripts/test_full_mkv.py --exe PATH_TO_XFOLD` to repeat the short
test. `--minutes 80` exercises feature-length add and replace with DTS-HD MA
and checks audio and video timing at the beginning, middle and end.
