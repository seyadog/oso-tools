//! Sounds the window plays (the window's audio does the playing; this only finds and reads the files): a sound of the
//! desktop's theme ("complete", "message"), or a file the person chose. Read only, small files, audio extensions only.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// A sound as the window gets it: the type and the bytes in base64 (a `data:` address for an audio element).
#[derive(serde::Serialize)]
pub struct SoundData {
    pub mime: String,
    pub b64: String,
}

/// The biggest sound file that is read.
pub const MAX_BYTES: u64 = 3 * 1024 * 1024;

/// The names of the freedesktop sound theme that oso uses.
pub const SYSTEM_NAMES: [&str; 2] = ["complete", "message"];

fn mime_of(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_lowercase().as_str() {
        "ogg" | "oga" => Some("audio/ogg"),
        "wav" => Some("audio/wav"),
        "mp3" => Some("audio/mpeg"),
        "flac" => Some("audio/flac"),
        _ => None,
    }
}

/// The data folders of the desktop (`$XDG_DATA_DIRS`, and the user's own), where `sounds/` lives.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(h) = directories::BaseDirs::new() {
        out.push(h.data_dir().to_path_buf());
    }
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    out.extend(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    out.extend(["/usr/local/share", "/usr/share"].map(PathBuf::from));
    out
}

/// A sound of the theme: `sounds/freedesktop/stereo/<name>.oga` in the first data folder that has it (then any other theme).
pub fn find_system(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if !SYSTEM_NAMES.contains(&name) {
        return None;
    }
    let file = |dir: &Path| ["oga", "ogg", "wav"].iter().map(|e| dir.join(format!("{name}.{e}"))).find(|p| p.is_file());
    for d in dirs {
        if let Some(p) = file(&d.join("sounds").join("freedesktop").join("stereo")) {
            return Some(p);
        }
    }
    for d in dirs {
        let Ok(themes) = std::fs::read_dir(d.join("sounds")) else { continue };
        let mut themes: Vec<PathBuf> = themes.flatten().map(|t| t.path()).collect();
        themes.sort();
        if let Some(p) = themes.iter().find_map(|t| file(&t.join("stereo"))) {
            return Some(p);
        }
    }
    None
}

/// The file, as `(mime type, bytes)`.
pub fn load(path: &Path) -> Result<(&'static str, Vec<u8>)> {
    let Some(mime) = mime_of(path) else { bail!("that is not a sound file (ogg, wav, mp3 or flac)") };
    let meta = std::fs::metadata(path).map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    if !meta.is_file() {
        bail!("{} is not a file", path.display());
    }
    if meta.len() > MAX_BYTES {
        bail!("that sound is too big (at most 3 MB)");
    }
    Ok((mime, std::fs::read(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-sounds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_sound_of_the_theme_is_found_by_its_name_and_only_those_names() {
        let a = temp("a");
        let b = temp("b");
        let theme = b.join("sounds/freedesktop/stereo");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("complete.oga"), b"x").unwrap();
        assert_eq!(find_system("complete", &[a.clone(), b.clone()]), Some(theme.join("complete.oga")));
        assert!(find_system("message", &[a.clone(), b.clone()]).is_none());
        assert!(find_system("../../etc/passwd", &[b.clone()]).is_none() && find_system("other", &[b.clone()]).is_none(), "only the names oso uses");
        // another theme is used when the freedesktop one is not there
        let other = a.join("sounds/ocean/stereo");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("message.ogg"), b"x").unwrap();
        assert_eq!(find_system("message", &[a.clone()]), Some(other.join("message.ogg")));
        std::fs::remove_dir_all(a).ok();
        std::fs::remove_dir_all(b).ok();
    }

    #[test]
    fn only_small_sound_files_are_read() {
        let d = temp("load");
        std::fs::write(d.join("a.ogg"), b"OggS").unwrap();
        std::fs::write(d.join("b.WAV"), b"RIFF").unwrap();
        std::fs::write(d.join("c.txt"), b"hello").unwrap();
        std::fs::write(d.join("big.mp3"), vec![0u8; (MAX_BYTES + 1) as usize]).unwrap();
        assert_eq!(load(&d.join("a.ogg")).unwrap(), ("audio/ogg", b"OggS".to_vec()));
        assert_eq!(load(&d.join("b.WAV")).unwrap().0, "audio/wav");
        assert!(load(&d.join("c.txt")).unwrap_err().to_string().contains("not a sound file"), "a text file is not read, whatever it holds");
        assert!(load(&d.join("big.mp3")).unwrap_err().to_string().contains("too big"));
        assert!(load(&d.join("nope.ogg")).is_err() && load(&d).is_err());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn the_real_sound_of_this_desktop_is_found_when_there_is_one() {
        if std::path::Path::new("/usr/share/sounds/freedesktop/stereo").is_dir() {
            let p = find_system("complete", &data_dirs()).expect("complete");
            assert!(load(&p).unwrap().1.len() > 100);
        }
    }
}
