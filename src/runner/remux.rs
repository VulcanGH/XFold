use super::{Job, Preset, RESERVE, Result, Stage, check_cancel, compare_payload, read_matroska};
use crate::mkv::{Identification, MkvMode, MkvTools, TRACK_FLAGS, Track};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub(super) struct Plan {
    report: Identification,
    index: usize,
    mode: MkvMode,
    scale: u64,
    offset_ms: u64,
    timing: Timing,
    timestamps: Vec<(u32, PathBuf)>,
    chapters: Option<PathBuf>,
}

struct Timing {
    first: u64,
    end: u64,
}

impl Job<'_> {
    pub(super) fn plan_remux(
        &mut self,
        mkv: &MkvTools,
        input: &Path,
        report: Identification,
        id: u32,
        mode: MkvMode,
    ) -> Result<Plan> {
        let scale = report
            .timestamp_scale()
            .ok_or("Full MKV output requires timestamp precision of 1 ms or better.")?;
        if report.linked() {
            return Err(
                "Linked MKV segments cannot be remuxed safely. Export standalone audio instead."
                    .into(),
            );
        }
        let index = report
            .tracks
            .iter()
            .position(|track| track.id == id)
            .ok_or("Missing selected track.")?;
        if matches!(mode, MkvMode::Replace)
            && (tag_count(&report, id) > 0 || report.tracks[index].properties.has_custom_tags())
        {
            return Err(
                "This track has track-specific tags. Use Add converted track to preserve them."
                    .into(),
            );
        }
        let chapters = if report.chapters.is_empty() {
            None
        } else {
            let path = self.work.join("source-chapters.xml");
            self.step(
                Stage::Inspect,
                "source-chapters",
                mkv.chapters(input, &path),
            )?;
            if matches!(mode, MkvMode::Replace)
                && fs::read_to_string(&path)?.contains("<ChapterTrackUID>")
            {
                return Err("This MKV has track-specific chapters. Use Add converted track to preserve their references.".into());
            }
            Some(path)
        };
        let timestamps = timestamp_paths(&self.work, &report, "source");
        self.step(
            Stage::Inspect,
            "source-timestamps",
            mkv.timestamps(input, &timestamps),
        )?;
        let timing = read_timing(&timestamps[index].1, 512, scale, self.cancel)?;
        let offset_ms = (timing.first + 500_000) / 1_000_000;
        if timing.first.abs_diff(offset_ms * 1_000_000) > scale.saturating_add(1) {
            return Err("The selected track starts at a fractional offset XFold cannot preserve. Export standalone audio instead.".into());
        }
        Ok(Plan {
            report,
            index,
            mode,
            scale,
            offset_ms,
            timing,
            timestamps,
            chapters,
        })
    }

    pub(super) fn remux(
        &mut self,
        mkv: &MkvTools,
        source: &Path,
        raw: &Path,
        plan: Plan,
        preset: Preset,
    ) -> Result<PathBuf> {
        let Plan {
            report,
            index,
            mode,
            scale,
            offset_ms,
            timing,
            timestamps,
            chapters,
        } = plan;
        let replacing = matches!(mode, MkvMode::Replace);
        let selected = &report.tracks[index];
        let new_index = if replacing {
            index
        } else {
            report.tracks.len()
        };
        let space = fs::metadata(source)?
            .len()
            .saturating_add(fs::metadata(raw)?.len().saturating_mul(3));
        self.space(RESERVE.saturating_add(space))?;
        let container = self.work.join("export.mkv");
        self.step(
            Stage::Mux,
            "remux",
            mkv.remux(
                source,
                raw,
                &container,
                &report.tracks,
                selected,
                mode,
                offset_ms,
                scale,
            ),
        )?;
        let identified = self.step(Stage::Verify, "identify-remux", mkv.identify(&container))?;
        let output = read_matroska(&identified)?;
        if output.tracks.len() != report.tracks.len() + usize::from(!replacing)
            || output.timestamp_scale() != Some(scale)
            || report.attachments != output.attachments
            || report.chapters != output.chapters
            || report.global_tags != output.global_tags
        {
            return Err("The new MKV failed its track or container metadata check.".into());
        }
        if let Some(source_chapters) = chapters {
            let output_chapters = self.work.join("output-chapters.xml");
            self.step(
                Stage::Verify,
                "output-chapters",
                mkv.chapters(&container, &output_chapters),
            )?;
            compare_payload(&source_chapters, &output_chapters, self.cancel)
                .map_err(|error| format!("MKV chapter verification failed: {error}"))?;
        }
        let converted = &output.tracks[new_index];
        let (codec, samples) = match preset {
            Preset::DtsHdMa => ("A_DTS", 512),
            Preset::TrueHd => ("A_TRUEHD", 40),
        };
        if !converted.has_codec(codec)
            || TRACK_FLAGS.iter().any(|&(flag, _, default)| {
                if flag.starts_with("flag_") {
                    return selected.properties.flag_value(flag)
                        != converted.properties.flag_value(flag);
                }
                let expected = selected.properties.flag(flag, default)
                    && (replacing || (flag != "default_track" && flag != "forced_track"));
                converted.properties.flag(flag, default) != expected
            })
        {
            return Err("The converted MKV track failed its codec or track flag check.".into());
        }

        let output_timestamps = timestamp_paths(&self.work, &output, "output");
        self.step(
            Stage::Verify,
            "output-timestamps",
            mkv.timestamps(&container, &output_timestamps),
        )?;
        for (i, source_track) in report.tracks.iter().enumerate() {
            if replacing && i == index {
                continue;
            }
            let output_track = &output.tracks[i];
            if !retained_track(source_track, output_track)
                || tag_count(&report, source_track.id) != tag_count(&output, output_track.id)
            {
                return Err(
                    format!("Retained track {} changed during muxing.", source_track.id).into(),
                );
            }
            if !same_timestamps(
                &timestamps[i].1,
                &output_timestamps[i].1,
                scale,
                self.cancel,
            )? {
                return Err(
                    format!("Retained timing changed for track {}.", source_track.id).into(),
                );
            }
        }
        let converted_timing =
            read_timing(&output_timestamps[new_index].1, samples, scale, self.cancel)?;
        // TrueHD may pad its last access unit by up to 39 samples.
        if converted_timing.first.abs_diff(timing.first) > scale * 2
            || converted_timing.end.abs_diff(timing.end) > scale * 2 + 812_500
        {
            return Err("The converted track did not keep its source timing. Export standalone audio instead.".into());
        }
        let extracted = self
            .work
            .join(format!("verified-remux.{}", preset.extension()));
        self.step(
            Stage::Verify,
            "extract-remux",
            mkv.extract(&container, converted.id, &extracted),
        )?;
        compare_payload(raw, &extracted, self.cancel)?;
        Ok(container)
    }
}

fn timestamp_paths(work: &Path, report: &Identification, prefix: &str) -> Vec<(u32, PathBuf)> {
    report
        .tracks
        .iter()
        .map(|track| {
            (
                track.id,
                work.join(format!("{prefix}-{}.timestamps", track.id)),
            )
        })
        .collect()
}

fn retained_track(source: &Track, output: &Track) -> bool {
    let (a, b) = (&source.properties, &output.properties);
    source.kind == output.kind
        && a.codec_id == b.codec_id
        && a.language == b.language
        && a.language_ietf == b.language_ietf
        && a.track_name == b.track_name
        && a.extra.get("uid") == b.extra.get("uid")
        && TRACK_FLAGS.iter().all(|&(flag, _, default)| {
            if flag.starts_with("flag_") {
                a.flag_value(flag) == b.flag_value(flag)
            } else {
                a.flag(flag, default) == b.flag(flag, default)
            }
        })
}

fn tag_count(report: &Identification, id: u32) -> u64 {
    report
        .track_tags
        .iter()
        .filter(|tag| tag["track_id"].as_u64() == Some(u64::from(id)))
        .filter_map(|tag| tag["num_entries"].as_u64())
        .sum()
}

fn read_timing(
    path: &Path,
    samples_per_packet: u64,
    scale: u64,
    cancel: &AtomicBool,
) -> Result<Timing> {
    let unsupported = "The track has discontinuous, negative, or unsupported timestamps. Export standalone audio instead.";
    let nanos = |packets: u64| (packets * samples_per_packet * 1_000_000_000) / 48_000;
    let mut first = None;
    let mut count = 0;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if count % 4096 == 0 {
            check_cancel(cancel)?;
        }
        let value = line.trim().parse::<f64>().map_err(|_| unsupported)?;
        if !(0.0..=86_400_000.0).contains(&value) {
            return Err(unsupported.into());
        }
        let timestamp = (value * 1_000_000.0).round() as u64;
        let start = *first.get_or_insert(timestamp);
        if timestamp.abs_diff(start + nanos(count)) > scale.saturating_add(2) {
            return Err(unsupported.into());
        }
        count += 1;
    }
    let first = first.ok_or(unsupported)?;
    if count < 2 {
        return Err(unsupported.into());
    }
    // MKVToolNix appends an end boundary after the last packet timestamp.
    Ok(Timing {
        first,
        end: first + nanos(count - 1),
    })
}

fn same_timestamps(a: &Path, b: &Path, scale: u64, cancel: &AtomicBool) -> Result<bool> {
    let mut a = BufReader::new(File::open(a)?).lines();
    let mut b = BufReader::new(File::open(b)?).lines();
    if a.next().transpose()? != b.next().transpose()? {
        return Ok(false);
    }
    let mut count = 0;
    loop {
        if count % 4096 == 0 {
            check_cancel(cancel)?;
        }
        match (a.next().transpose()?, b.next().transpose()?) {
            (None, None) => return Ok(true),
            (Some(a), Some(b)) => {
                let a = a.trim().parse::<f64>()?;
                let b = b.trim().parse::<f64>()?;
                if !a.is_finite()
                    || !b.is_finite()
                    || (a - b).abs() * 1_000_000.0 > (scale + 2) as f64
                {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
        count += 1;
    }
}
