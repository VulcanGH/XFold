use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use crate::tools::{Invocation, bundle_root};

pub struct MkvTools {
    merge: PathBuf,
    extract: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MkvMode {
    Add,
    Replace,
}

impl MkvTools {
    pub fn resolve() -> io::Result<Self> {
        let root = bundle_root()?.join("mkvtoolnix/mkvtoolnix");
        Ok(Self {
            merge: root.join("mkvmerge.exe").canonicalize()?,
            extract: root.join("mkvextract.exe").canonicalize()?,
        })
    }

    pub fn identify(&self, input: &Path) -> Invocation {
        Invocation {
            exe: self.merge.clone(),
            args: vec![
                "--output-charset".into(),
                "UTF-8".into(),
                "-J".into(),
                input.into(),
            ],
        }
    }

    pub fn extract(&self, input: &Path, id: u32, output: &Path) -> Invocation {
        let mut target = OsString::from(format!("{id}:"));
        target.push(output);
        Invocation {
            exe: self.extract.clone(),
            args: vec![input.into(), "tracks".into(), target],
        }
    }

    pub fn mux_audio(&self, input: &Path, output: &Path) -> Invocation {
        Invocation {
            exe: self.merge.clone(),
            args: vec!["-o".into(), output.into(), input.into()],
        }
    }

    pub fn timestamps(&self, input: &Path, targets: &[(u32, PathBuf)]) -> Invocation {
        let mut args = vec![input.into(), "timestamps_v2".into()];
        args.extend(targets.iter().map(|(id, path)| {
            let mut target = OsString::from(format!("{id}:"));
            target.push(path);
            target
        }));
        Invocation {
            exe: self.extract.clone(),
            args,
        }
    }

    pub fn chapters(&self, input: &Path, output: &Path) -> Invocation {
        Invocation {
            exe: self.extract.clone(),
            args: vec![input.into(), "chapters".into(), output.into()],
        }
    }

    pub fn remux(
        &self,
        source: &Path,
        audio: &Path,
        output: &Path,
        tracks: &[Track],
        selected: &Track,
        mode: MkvMode,
        offset_ms: u64,
        scale: u64,
    ) -> Invocation {
        let replacing = matches!(mode, MkvMode::Replace);
        let mut order = tracks
            .iter()
            .map(|track| {
                if replacing && track.id == selected.id {
                    "1:0".into()
                } else {
                    format!("0:{}", track.id)
                }
            })
            .collect::<Vec<_>>();
        if !replacing {
            order.push("1:0".into());
        }
        let mut args = vec![
            "--disable-track-statistics-tags".into(),
            "--engage".into(),
            "keep_track_statistics_tags".into(),
            "--normalize-language-ietf".into(),
            "off".into(),
            "--timestamp-scale".into(),
            scale.to_string().into(),
            "--track-order".into(),
            order.join(",").into(),
            "-o".into(),
            output.into(),
        ];
        if replacing {
            args.extend(["--audio-tracks".into(), format!("!{}", selected.id).into()]);
        }
        args.push(source.into());
        args.extend(["--sync".into(), format!("0:{offset_ms}").into()]);
        let props = &selected.properties;
        let language = props
            .language_ietf
            .as_deref()
            .or(props.language.as_deref())
            .unwrap_or("und");
        args.extend(["--language".into(), format!("0:{language}").into()]);
        if replacing {
            if let Some(name) = &props.track_name {
                args.extend(["--track-name".into(), format!("0:{name}").into()]);
            }
        } else {
            args.extend(["--track-name".into(), "0:XFold converted audio".into()]);
        }
        for (property, option, default) in TRACK_FLAGS {
            if property.starts_with("flag_") && props.flag_value(property).is_none() {
                continue;
            }
            let original = props.flag(property, default);
            let value = if replacing || property != "default_track" && property != "forced_track" {
                original
            } else {
                false
            };
            args.extend([option.into(), format!("0:{}", u8::from(value)).into()]);
        }
        args.push(audio.into());
        Invocation {
            exe: self.merge.clone(),
            args,
        }
    }
}

pub const TRACK_FLAGS: [(&str, &str, bool); 8] = [
    ("default_track", "--default-track-flag", true),
    ("enabled_track", "--track-enabled-flag", true),
    ("forced_track", "--forced-display-flag", false),
    ("flag_hearing_impaired", "--hearing-impaired-flag", false),
    ("flag_visual_impaired", "--visual-impaired-flag", false),
    ("flag_text_descriptions", "--text-descriptions-flag", false),
    ("flag_original", "--original-flag", false),
    ("flag_commentary", "--commentary-flag", false),
];

#[derive(Deserialize)]
pub struct Identification {
    container: Container,
    pub tracks: Vec<Track>,
    pub attachments: Vec<Value>,
    pub chapters: Vec<Value>,
    pub global_tags: Vec<Value>,
    pub track_tags: Vec<Value>,
    errors: Vec<String>,
}

#[derive(Deserialize)]
struct Container {
    #[serde(rename = "type")]
    kind: String,
    recognized: bool,
    supported: bool,
    properties: Value,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Track {
    pub id: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub codec: String,
    pub properties: TrackProperties,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TrackProperties {
    pub codec_id: Option<String>,
    pub number: Option<u64>,
    pub language: Option<String>,
    pub language_ietf: Option<String>,
    pub track_name: Option<String>,
    pub audio_channels: Option<u32>,
    pub audio_sampling_frequency: Option<f64>,
    pub default_track: Option<bool>,
    #[serde(flatten, skip_serializing)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

impl Identification {
    pub fn is_matroska(&self) -> bool {
        self.container.recognized
            && self.container.supported
            && self.container.kind == "Matroska"
            && self.errors.is_empty()
    }

    pub fn audio_tracks(self) -> Vec<Track> {
        self.tracks.into_iter().filter(Track::is_audio).collect()
    }

    pub fn timestamp_scale(&self) -> Option<u64> {
        self.container.properties["timestamp_scale"]
            .as_u64()
            .filter(|v| (1000..=1_000_000).contains(v))
    }

    pub fn linked(&self) -> bool {
        ["previous_segment_uid", "next_segment_uid", "segment_family"]
            .iter()
            .any(|key| {
                self.container
                    .properties
                    .get(key)
                    .is_some_and(|v| !v.is_null())
            })
    }
}

impl TrackProperties {
    pub fn flag_value(&self, field: &str) -> Option<bool> {
        if field == "default_track" {
            self.default_track
        } else {
            self.extra.get(field).and_then(Value::as_bool)
        }
    }

    pub fn flag(&self, field: &str, default: bool) -> bool {
        self.flag_value(field).unwrap_or(default)
    }

    pub fn has_custom_tags(&self) -> bool {
        self.extra.keys().any(|key| {
            key.starts_with("tag_")
                && ![
                    "tag_bps",
                    "tag_duration",
                    "tag_number_of_frames",
                    "tag_number_of_bytes",
                    "tag__statistics_tags",
                    "tag__statistics_writing_app",
                    "tag__statistics_writing_date_utc",
                ]
                .contains(&key.as_str())
        })
    }
}

impl Track {
    pub fn is_audio(&self) -> bool {
        self.kind == "audio"
    }

    pub fn has_codec(&self, codec_id: &str) -> bool {
        self.is_audio() && self.properties.codec_id.as_deref() == Some(codec_id)
    }
}
