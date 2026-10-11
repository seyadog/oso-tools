//! The record of what AI agents asked and what happened: `audit.jsonl`, one line each, only ever appended to.
//! It holds the command the agent sent, never a secret oso added (the sudo password is not in it).

use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Debug, Default, Clone)]
pub struct Entry {
    /// Unix seconds.
    pub ts: u64,
    pub pane: u64,
    pub agent: String,
    pub action: String,
    pub host: String,
    pub command: String,
    /// `allowed`, `allowed_without_asking`, `denied`, `blocked`, `failed`…
    pub decision: String,
    pub exit_code: Option<i32>,
    pub bytes_out: Option<usize>,
    pub note: String,
}

pub struct Audit {
    path: PathBuf,
}

impl Audit {
    pub fn new(path: PathBuf) -> Audit {
        Audit { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Adds a line. A failure to write is not a reason to refuse what was approved, but it is said on stderr.
    pub fn log(&self, mut e: Entry) {
        if e.ts == 0 {
            e.ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        }
        let Ok(mut line) = serde_json::to_vec(&e) else { return };
        line.push(b'\n');
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
            }
            f.write_all(&line)
        })();
        if let Err(err) = result {
            eprintln!("oso: could not write the audit log: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_only_added() {
        let dir = std::env::temp_dir().join(format!("oso-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Audit::new(dir.join("audit.jsonl"));
        a.log(Entry { pane: 1, action: "run".into(), host: "acme".into(), command: "df -h".into(), decision: "allowed".into(), exit_code: Some(0), ..Default::default() });
        a.log(Entry { pane: 1, action: "run".into(), host: "acme".into(), command: "reboot".into(), decision: "denied".into(), ..Default::default() });
        let text = std::fs::read_to_string(a.path()).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0]["command"].as_str(), lines[0]["decision"].as_str(), lines[0]["exit_code"].as_i64()), (Some("df -h"), Some("allowed"), Some(0)));
        assert_eq!(lines[1]["decision"], "denied");
        assert!(lines[0]["ts"].as_u64().unwrap() > 1_700_000_000, "stamped with the time");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(a.path()).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(dir).ok();
    }
}
