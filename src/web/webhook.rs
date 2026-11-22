use super::Result;
use crate::{
    desktop,
    mkv::MkvMode,
    runner::{Attempt, Preset},
    windows,
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr::null,
    slice,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

const FILE: &str = "discord-webhook.dat";
const PREFIX: &str = "https://discord.com/api/webhooks/";

pub(super) fn completion_embed(attempt: &Attempt) -> Value {
    let source = &attempt.source_metadata;
    let title = source
        .title
        .as_deref()
        .and_then(|title| embed_text(title, 256))
        .or_else(|| embed_text(&attempt.input.file_stem()?.to_string_lossy(), 256))
        .unwrap_or_else(|| "Untitled source".to_owned());
    let codec = match attempt.preset {
        Preset::DtsHdMa => "DTS-HD Master Audio",
        Preset::TrueHd => "TrueHD with spatial metadata",
    };
    let format = match attempt.mkv_mode {
        Some(MkvMode::Add) => "New MKV · added audio track",
        Some(MkvMode::Replace) => "New MKV · replaced audio track",
        None if super::has_extension(&attempt.output, "mka") => "Matroska audio (.mka)",
        None if attempt.preset == Preset::DtsHdMa => "Raw audio (.dts)",
        None => "Raw audio (.thd)",
    };
    let seconds = attempt.elapsed_seconds();
    let elapsed = if seconds >= 3600 {
        format!(
            "{}h {}m {}s",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds == 0 {
        "Under 1s".to_owned()
    } else {
        format!("{seconds}s")
    };
    let mut fields = vec![
        json!({"name": "Audio", "value": format!("DTS:X → {codec}"), "inline": true}),
        json!({"name": "Export", "value": format, "inline": true}),
        json!({"name": "Conversion time", "value": elapsed, "inline": true}),
    ];
    if let Some(bytes) = attempt.output_bytes {
        let size = if bytes >= 1 << 30 {
            format!("{:.2} GiB", bytes as f64 / (1u64 << 30) as f64)
        } else if bytes >= 1 << 20 {
            format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
        } else {
            format!("{:.1} KiB", bytes as f64 / 1024.0)
        };
        fields.push(json!({"name": "Export size", "value": size, "inline": true}));
    }
    if let Some(language) = source
        .language
        .as_deref()
        .and_then(|language| embed_text(language, 128))
    {
        fields.push(json!({"name": "Source language", "value": language, "inline": true}));
    }
    if let Some(track) = source
        .track_name
        .as_deref()
        .and_then(|track| embed_text(track, 256))
    {
        fields.push(json!({"name": "Source track", "value": track, "inline": true}));
    }
    json!({
        "allowed_mentions": {"parse": []},
        "embeds": [{
            "title": title,
            "description": "Conversion complete",
            "color": 0x2eaa71,
            "fields": fields,
            "footer": {"text": concat!("XFold v", env!("CARGO_PKG_VERSION"))},
        }],
    })
}

fn embed_text(text: &str, limit: usize) -> Option<String> {
    let mut value = String::new();
    let mut length = 0;
    let chars = text
        .split_whitespace()
        .flat_map(|word| word.chars().chain(std::iter::once(' ')));
    for ch in chars {
        if ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            continue;
        }
        let escape = matches!(
            ch,
            '\\' | '*' | '_' | '~' | '`' | '|' | '[' | ']' | '<' | '>' | '#'
        );
        length += ch.len_utf16() + usize::from(escape);
        if length > limit {
            break;
        }
        if escape {
            value.push('\\');
        }
        value.push(ch);
    }
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[derive(Clone)]
pub(super) struct Webhook(String);

impl Webhook {
    pub fn parse(value: &str) -> Option<Self> {
        let url = value.trim();
        let (id, token) = url.strip_prefix(PREFIX)?.split_once('/')?;
        if id.is_empty()
            || id.len() > 20
            || !id.bytes().all(|b| b.is_ascii_digit())
            || token.is_empty()
            || token.len() > 256
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return None;
        }
        Some(Self(url.to_owned()))
    }

    pub fn load(dir: &Path) -> Result<Option<Self>> {
        match fs::read(dir.join(FILE)) {
            Ok(mut data) => {
                let raw = String::from_utf8(crypt(&mut data, false)?)?;
                Self::parse(&raw)
                    .map(Some)
                    .ok_or_else(|| "Stored webhook address is invalid.".into())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let encrypted = crypt(&mut self.0.as_bytes().to_vec(), true)?;
        let temp = dir.join(format!("webhook-{}.tmp", desktop::token()?));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = file.write_all(&encrypted).and_then(|()| file.sync_all());
        drop(file);
        let result = result.and_then(|()| windows::rename(&temp, &dir.join(FILE), true));
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        Ok(result?)
    }

    pub fn remove(dir: &Path) -> Result<()> {
        match fs::remove_file(dir.join(FILE)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn notify(&self, payload: &Value) -> bool {
        self.deliver(payload).unwrap_or(false)
    }

    fn deliver(&self, payload: &Value) -> io::Result<bool> {
        let system = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("Windows directory unavailable"))?;
        let curl = PathBuf::from(system).join("System32/curl.exe");
        // Pass the private URL through stdin. Keep response bodies and curl's
        // diagnostics out of the UI, captured logs, and process command line.
        let mut child = Command::new(curl)
            .args([
                "--disable",
                "--config",
                "-",
                "--silent",
                "--proto",
                "=https",
                "--connect-timeout",
                "5",
                "--max-time",
                "15",
                "--output",
                "NUL",
                "--write-out",
                "%{http_code}",
                "--header",
                "Content-Type: application/json",
            ])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let written = write_request(child.stdin.take().unwrap(), &self.0, payload);
        let response = child.wait_with_output()?;
        written?;
        Ok(response.status.success()
            && response.stdout.len() == 3
            && response.stdout[0] == b'2'
            && response.stdout[1..].iter().all(u8::is_ascii_digit))
    }
}

fn write_request(mut config: impl Write, url: &str, payload: &Value) -> io::Result<()> {
    let body = serde_json::to_string(&payload.to_string())?;
    writeln!(config, "url = \"{url}?wait=true\"\ndata-binary = {body}")
}

fn crypt(bytes: &mut [u8], protect: bool) -> io::Result<Vec<u8>> {
    let size = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: size,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let ok = if protect {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if output.pbData.is_null() {
            return Err(io::Error::other("Windows returned no credential data"));
        }
        let data = slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_metadata_is_bounded_and_curl_config_escapes_json() {
        assert_eq!(
            embed_text("  *Film*\n🎞️\u{202e}  ", 256).as_deref(),
            Some("\\*Film\\* 🎞️")
        );
        assert_eq!(
            embed_text(&"😀".repeat(200), 255)
                .unwrap()
                .encode_utf16()
                .count(),
            254
        );
        assert!(embed_text("\t\u{202e}", 256).is_none());
        let payload = json!({"embeds": [{"title": "A \"quoted\" title \\ 日本"}]});
        let mut config = Vec::new();
        write_request(
            &mut config,
            "https://discord.com/api/webhooks/123/token",
            &payload,
        )
        .unwrap();
        let config = String::from_utf8(config).unwrap();
        let mut lines = config.lines();
        assert_eq!(
            lines.next(),
            Some("url = \"https://discord.com/api/webhooks/123/token?wait=true\"")
        );
        let body = lines
            .next()
            .unwrap()
            .strip_prefix("data-binary = ")
            .unwrap();
        let decoded: String = serde_json::from_str(body).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&decoded).unwrap(), payload);
        assert!(lines.next().is_none());
    }
}
