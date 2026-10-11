//! Bringing secrets in from where they already are: `.env` files in projects (`oso scan <dir>`) and the export of
//! Proton Pass (CSV or JSON). Reading and previewing change nothing and show no value; importing puts the chosen
//! ones in the vault; deleting the originals is a separate step that is only taken when the person says so.

use crate::cli::Term;
use crate::engine::Engine;
use crate::vault::{SecretKind, SecretMeta};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ .env files

/// `KEY=value` lines of a `.env` file: `export` prefixes, quotes and `#` comments are understood.
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((k, v)) = line.split_once('=') else { continue };
        let key = k.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
            continue;
        }
        let v = v.trim();
        let value = if let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') {
            match v[1..].find(q) {
                Some(end) => v[1..1 + end].to_string(),
                None => v[1..].to_string(),
            }
        } else {
            // an unquoted value ends at ` #`
            v.split(" #").next().unwrap_or("").trim().to_string()
        };
        out.push((key.to_string(), value));
    }
    out
}

/// Is this variable a secret, and of which kind? Placeholders and plain settings are not.
pub fn classify(name: &str, value: &str) -> Option<SecretKind> {
    let v = value.trim();
    let low = v.to_lowercase();
    let placeholder = v.len() < 4 || ["changeme", "change_me", "your_", "your-", "xxx", "example", "todo", "placeholder", "<", "${", "null", "none", "undefined", "secret_here", "replace"].iter().any(|p| low.contains(p)) || ["true", "false"].contains(&low.as_str()) || v.chars().all(|c| c.is_ascii_digit()) && v.len() < 10;
    if placeholder {
        return None;
    }
    let n = name.to_uppercase();
    let has = |s: &[&str]| s.iter().any(|x| n.contains(x));
    if has(&["PASSWORD", "PASSWD", "PWD", "PASSPHRASE"]) {
        Some(SecretKind::Password)
    } else if has(&["TOKEN", "BEARER", "JWT", "COOKIE"]) {
        Some(SecretKind::Token)
    } else if has(&["API_KEY", "APIKEY", "ACCESS_KEY", "SECRET", "PRIVATE_KEY", "_KEY", "CREDENTIAL", "AUTH"]) && !has(&["PUBLIC", "KEY_ID", "KEYID"]) {
        Some(SecretKind::ApiKey)
    } else if (has(&["_URL", "_URI", "DSN", "CONNECTION"]) || n == "DATABASE_URL") && v.contains("://") && v.split("://").nth(1).is_some_and(|rest| rest.split('/').next().is_some_and(|auth| auth.contains(':') && auth.contains('@'))) {
        // a connection string with the password inside
        Some(SecretKind::Token)
    } else {
        None
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EnvEntry {
    pub name: String,
    pub kind: SecretKind,
    /// How long the value is (never the value).
    pub length: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EnvFile {
    pub path: String,
    /// The folder it is in (the project).
    pub project: String,
    pub entries: Vec<EnvEntry>,
    /// Variables that are not secrets (ports, hosts…), only counted.
    pub others: usize,
}

fn is_env_file(name: &str) -> bool {
    (name == ".env" || name.starts_with(".env.")) && !["example", "sample", "template", "dist", "defaults", "schema"].iter().any(|s| name.ends_with(s))
}

fn walk(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    if depth > 4 || out.len() >= 200 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(t) = e.file_type() else { continue };
        if t.is_symlink() {
            continue;
        }
        let path = e.path();
        if t.is_dir() {
            if !matches!(name.as_str(), "node_modules" | ".git" | "target" | ".venv" | "venv" | "__pycache__" | "dist" | "build" | ".cache") {
                walk(&path, depth + 1, out);
            }
        } else if is_env_file(&name) {
            out.push(path);
        }
    }
}

/// The `.env` files under `dir` that hold secrets, with what is in them (names and lengths only).
pub fn scan(dir: &Path) -> Vec<EnvFile> {
    let mut files = vec![];
    walk(dir, 0, &mut files);
    let mut out = vec![];
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut entries = vec![];
        let mut others = 0;
        for (name, value) in parse_env(&text) {
            match classify(&name, &value) {
                Some(kind) => entries.push(EnvEntry { name, kind, length: value.len() }),
                None => others += 1,
            }
        }
        if entries.is_empty() {
            continue;
        }
        let project = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(EnvFile { path: path.to_string_lossy().into_owned(), project, entries, others });
    }
    out
}

#[derive(Serialize, Default, Debug)]
pub struct ImportSummary {
    pub added: Vec<String>,
    pub skipped: Vec<String>,
    /// The files whose secrets all went in (for the question about deleting them).
    pub originals: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct EnvChoice {
    pub path: String,
    /// Names to bring in; empty: all of them.
    #[serde(default)]
    pub names: Vec<String>,
}

impl Engine {
    pub fn env_preview(&self, dir: &str) -> Vec<EnvFile> {
        scan(Path::new(dir))
    }

    /// Brings the chosen variables of `.env` files under `dir` into the vault as secrets named `project/VARIABLE`.
    /// They get no environment variable of their own: two projects may use the same name, and a variable in every
    /// terminal is a decision for the person.
    pub fn env_import(&mut self, dir: &str, choices: &[EnvChoice]) -> Result<ImportSummary> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        let root = Path::new(dir).canonicalize().with_context(|| format!("{dir} is not there"))?;
        let mut out = ImportSummary::default();
        let existing: Vec<(String, u64)> = self.vault.list().into_iter().map(|m| (m.name, m.id)).collect();
        for c in choices {
            let Ok(path) = Path::new(&c.path).canonicalize() else {
                out.skipped.push(format!("{}: not there", c.path));
                continue;
            };
            if !path.starts_with(&root) || !path.file_name().is_some_and(|n| is_env_file(&n.to_string_lossy())) {
                out.skipped.push(format!("{}: not a .env file of that folder", c.path));
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            let project = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mut all_in = true;
            for (name, value) in parse_env(&text) {
                let Some(kind) = classify(&name, &value) else { continue };
                if !c.names.is_empty() && !c.names.contains(&name) {
                    all_in = false;
                    continue;
                }
                let full = format!("{project}/{name}");
                if let Some((_, id)) = existing.iter().find(|(n, _)| *n == full) {
                    if self.vault.reveal(*id).as_deref() == Some(value.as_str()) {
                        out.skipped.push(format!("{full}: already in the vault"));
                        continue;
                    }
                }
                let meta = SecretMeta { id: 0, name: full.clone(), kind, profile: None, env_var: None, agent_visible: false, notes: format!("imported from {}", path.display()), updated: 0 };
                match self.vault.save_secret(meta, Some(&value)) {
                    Ok(_) => out.added.push(full),
                    Err(e) => {
                        all_in = false;
                        out.skipped.push(format!("{full}: {e:#}"));
                    }
                }
            }
            if all_in {
                out.originals.push(path.to_string_lossy().into_owned());
            }
        }
        Ok(out)
    }

    /// Deletes `.env` files of the scanned folder that the person said to delete. Nothing else is ever touched.
    pub fn env_delete(&mut self, dir: &str, paths: &[String]) -> Result<Vec<String>> {
        let root = Path::new(dir).canonicalize().with_context(|| format!("{dir} is not there"))?;
        let mut gone = vec![];
        for p in paths {
            let Ok(path) = Path::new(p).canonicalize() else { continue };
            if path.starts_with(&root) && path.file_name().is_some_and(|n| is_env_file(&n.to_string_lossy())) && std::fs::remove_file(&path).is_ok() {
                gone.push(path.to_string_lossy().into_owned());
            }
        }
        Ok(gone)
    }
}

// ------------------------------------------------------------------ Proton Pass

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct ProtonItem {
    pub name: String,
    pub username: String,
    pub url: String,
    pub note: String,
    pub has_totp: bool,
    #[serde(skip)]
    pub password: String,
    #[serde(skip)]
    pub totp: String,
}

/// Rows of a CSV file (quotes, doubled quotes and line breaks inside quotes are understood).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = vec![];
    let mut row: Vec<String> = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// Logins from a Proton Pass CSV export (`type,name,url,email,username,password,note,totp,…`).
pub fn proton_from_csv(text: &str) -> Vec<ProtonItem> {
    let rows = parse_csv(text);
    let Some(head) = rows.first() else { return vec![] };
    let col = |n: &str| head.iter().position(|h| h.trim().eq_ignore_ascii_case(n));
    let (t, name, url, email, user, pass, note, totp) = (col("type"), col("name"), col("url"), col("email"), col("username"), col("password"), col("note"), col("totp"));
    let get = |r: &Vec<String>, c: Option<usize>| c.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
    rows[1..]
        .iter()
        .filter(|r| t.is_none() || get(r, t).eq_ignore_ascii_case("login"))
        .filter(|r| !get(r, pass).is_empty() || !get(r, totp).is_empty())
        .map(|r| {
            let username = if get(r, user).is_empty() { get(r, email) } else { get(r, user) };
            ProtonItem { name: get(r, name), username, url: get(r, url), note: get(r, note), has_totp: !get(r, totp).is_empty(), password: get(r, pass), totp: get(r, totp) }
        })
        .collect()
}

/// Logins from a Proton Pass JSON export (`vaults → items → data`).
pub fn proton_from_json(text: &str) -> Vec<ProtonItem> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return vec![] };
    let mut out = vec![];
    for vault in v.get("vaults").and_then(Value::as_object).into_iter().flat_map(|o| o.values()) {
        for item in vault.get("items").and_then(Value::as_array).into_iter().flatten() {
            let d = &item["data"];
            if d["type"] != "login" {
                continue;
            }
            let c = &d["content"];
            let s = |x: &Value| x.as_str().unwrap_or("").trim().to_string();
            let username = if s(&c["itemUsername"]).is_empty() { s(&c["itemEmail"]) } else { s(&c["itemUsername"]) };
            let (password, totp) = (s(&c["password"]), s(&c["totpUri"]));
            if password.is_empty() && totp.is_empty() {
                continue;
            }
            out.push(ProtonItem { name: s(&d["metadata"]["name"]), username, url: c["urls"].as_array().and_then(|u| u.first()).map(s).unwrap_or_default(), note: s(&d["metadata"]["note"]), has_totp: !totp.is_empty(), password, totp });
        }
    }
    out
}

/// The logins of an export file, whichever of the two formats it is.
pub fn proton_read(path: &Path) -> Result<Vec<ProtonItem>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    let items = if text.trim_start().starts_with('{') { proton_from_json(&text) } else { proton_from_csv(&text) };
    if items.is_empty() {
        bail!("no logins found in that file (a Proton Pass export, CSV or JSON, without encryption)");
    }
    Ok(items)
}

impl Engine {
    /// What a Proton Pass export holds (names, never passwords).
    pub fn proton_preview(&self, path: &str) -> Result<Vec<ProtonItem>> {
        proton_read(Path::new(path))
    }

    /// Brings the chosen logins (by position in the preview) into the vault; each one's TOTP seed becomes its own secret.
    pub fn proton_import(&mut self, path: &str, which: &[usize]) -> Result<ImportSummary> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        let items = proton_read(Path::new(path))?;
        let mut out = ImportSummary::default();
        for &i in which {
            let Some(it) = items.get(i) else { continue };
            let label = if it.username.is_empty() { it.name.clone() } else { format!("{} ({})", it.name, it.username) };
            let notes = [it.url.as_str(), it.note.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n");
            if !it.password.is_empty() {
                let meta = SecretMeta { id: 0, name: label.clone(), kind: SecretKind::Password, profile: None, env_var: None, agent_visible: false, notes: notes.clone(), updated: 0 };
                match self.vault.save_secret(meta, Some(&it.password)) {
                    Ok(_) => out.added.push(label.clone()),
                    Err(e) => out.skipped.push(format!("{label}: {e:#}")),
                }
            }
            if !it.totp.is_empty() {
                let name = format!("{label} (2FA)");
                let meta = SecretMeta { id: 0, name: name.clone(), kind: SecretKind::Totp, profile: None, env_var: None, agent_visible: false, notes, updated: 0 };
                match self.vault.save_secret(meta, Some(&it.totp)) {
                    Ok(_) => out.added.push(name),
                    Err(e) => out.skipped.push(format!("{name}: {e:#}")),
                }
            }
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ the command line

/// `oso scan <dir> [--import]`: lists what is there (names and lengths); with `--import` asks, brings it in, and
/// only then asks (default no) whether to delete the files.
pub fn cli(args: &[&str], socket: &Path, t: &mut dyn Term) -> i32 {
    let (dir, import) = match args {
        [d] => (*d, false),
        [d, "--import"] | ["--import", d] => (*d, true),
        _ => {
            t.err("usage: oso scan <folder> [--import]");
            return 2;
        }
    };
    let found = scan(Path::new(dir));
    if found.is_empty() {
        t.out("No .env file with secrets in it was found.");
        return 0;
    }
    for f in &found {
        t.out(&format!("{}  ({} secret{}, {} other setting{})", f.path, f.entries.len(), if f.entries.len() == 1 { "" } else { "s" }, f.others, if f.others == 1 { "" } else { "s" }));
        for e in &f.entries {
            t.out(&format!("    {}  [{:?}, {} characters]", e.name, e.kind, e.length));
        }
    }
    if !import {
        t.out("Nothing was imported. Add --import to bring these into the vault.");
        return 0;
    }
    if !t.confirm("Bring these secrets into the vault?") {
        return 1;
    }
    let choices: Vec<Value> = found.iter().map(|f| json!({ "path": f.path, "names": [] })).collect();
    let answer = match crate::control::request(socket, &json!({ "cmd": "env_import", "dir": dir, "choices": choices })) {
        Ok(v) if v["ok"] == true => v,
        Ok(v) => {
            t.err(v["error"].as_str().unwrap_or("could not import"));
            return 1;
        }
        Err(e) => {
            t.err(&format!("{e:#}"));
            return 1;
        }
    };
    let n = |k: &str| answer[k].as_array().map(Vec::len).unwrap_or(0);
    t.out(&format!("Imported {} secret(s); {} skipped.", n("added"), n("skipped")));
    let originals: Vec<String> = answer["originals"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    if !originals.is_empty() && t.confirm(&format!("Delete the {} original file(s)? Keep a backup of the vault first.", originals.len())) {
        match crate::control::request(socket, &json!({ "cmd": "env_delete", "dir": dir, "paths": originals })) {
            Ok(v) if v["ok"] == true => t.out(&format!("Deleted {} file(s).", v["deleted"].as_array().map(Vec::len).unwrap_or(0))),
            _ => t.err("could not delete them"),
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_lines_are_read_like_a_shell_would_mostly() {
        let text = "# comment\n\nPORT=3000\nexport API_KEY=\"abc def\"  # trailing\nTOKEN='single quoted'\nURL=http://x/y # note\nBAD LINE\n=novalue\nEMPTY=\nWITH.DOT=1\n";
        let got = parse_env(text);
        let get = |k: &str| got.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("PORT"), Some("3000"));
        assert_eq!(get("API_KEY"), Some("abc def"));
        assert_eq!(get("TOKEN"), Some("single quoted"));
        assert_eq!(get("URL"), Some("http://x/y"));
        assert_eq!(get("EMPTY"), Some(""));
        assert_eq!(get("WITH.DOT"), Some("1"));
        assert!(get("BAD LINE").is_none() && got.iter().all(|(k, _)| !k.is_empty()));
    }

    #[test]
    fn secrets_are_told_from_settings_and_placeholders() {
        use SecretKind::*;
        assert_eq!(classify("STRIPE_API_KEY", "sk_live_51Habcdef"), Some(ApiKey));
        assert_eq!(classify("DB_PASSWORD", "s3cr3t-value"), Some(Password));
        assert_eq!(classify("GITHUB_TOKEN", "ghp_abcdefghijkl"), Some(Token));
        assert_eq!(classify("AWS_SECRET_ACCESS_KEY", "wJalrXUtnFEMI/K7MDENG"), Some(ApiKey));
        assert_eq!(classify("DATABASE_URL", "postgres://user:pw@host:5432/db"), Some(Token));
        assert_eq!(classify("DATABASE_URL", "postgres://host:5432/db"), None, "no password inside");
        for (k, v) in [("PORT", "3000"), ("NODE_ENV", "production"), ("API_KEY", "changeme"), ("API_KEY", "your_key_here"), ("DB_PASSWORD", ""), ("DEBUG", "true"), ("PUBLIC_KEY", "pk_live_aaaaaaaa"), ("HOST", "localhost"), ("TOKEN", "<token>"), ("SECRET", "${SECRET}")] {
            assert_eq!(classify(k, v), None, "{k}={v}");
        }
    }

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-envscan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("app")).unwrap();
        std::fs::create_dir_all(d.join("web/node_modules/x")).unwrap();
        std::fs::create_dir_all(d.join("empty")).unwrap();
        std::fs::write(d.join("app/.env"), "PORT=80\nSTRIPE_KEY=sk_live_TRAP1234\nDB_PASSWORD=TRAPpass99\n").unwrap();
        std::fs::write(d.join("app/.env.example"), "STRIPE_KEY=sk_live_EXAMPLE12\n").unwrap();
        std::fs::write(d.join("web/.env.production"), "API_TOKEN=tok_TRAP5678\n").unwrap();
        std::fs::write(d.join("web/node_modules/x/.env"), "API_TOKEN=tok_NEVER12345\n").unwrap();
        std::fs::write(d.join("empty/.env"), "PORT=1\n").unwrap();
        d
    }

    #[test]
    fn the_scan_finds_env_files_with_secrets_and_shows_no_value() {
        let d = tree("scan");
        let found = scan(&d);
        let paths: Vec<String> = found.iter().map(|f| f.path.strip_prefix(&format!("{}/", d.display())).unwrap().to_string()).collect();
        assert_eq!(paths, vec!["app/.env", "web/.env.production"], "examples, node_modules and files without secrets are left out");
        let app = &found[0];
        assert_eq!((app.project.as_str(), app.others), ("app", 1));
        assert_eq!(app.entries.iter().map(|e| (e.name.as_str(), e.length)).collect::<Vec<_>>(), vec![("STRIPE_KEY", 16), ("DB_PASSWORD", 10)]);
        let text = serde_json::to_string(&found).unwrap();
        assert!(!text.contains("TRAP"), "no value in the preview: {text}");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn csv_handles_quotes_commas_and_line_breaks_inside_cells() {
        let rows = parse_csv("a,b,c\n1,\"x,y\",\"line1\nline2\"\n\"q\"\"uote\",,end");
        assert_eq!(rows[1], vec!["1", "x,y", "line1\nline2"]);
        assert_eq!(rows[2], vec!["q\"uote", "", "end"]);
    }

    #[test]
    fn proton_pass_csv_and_json_exports_give_the_logins() {
        let csv = "type,name,url,email,username,password,note,totp,vault\nlogin,GitHub,https://github.com,me@x.com,me,pw-one,work,otpauth://totp/GitHub?secret=JBSWY3DPEHPK3PXP,Personal\nnote,Shopping list,,,,,milk,,Personal\nlogin,No password,https://x,a@b.c,,,,,Personal\nlogin,Mail,https://mail,m@x.com,,pw-two,,,Personal\n";
        let items = proton_from_csv(csv);
        assert_eq!(items.len(), 2, "notes and logins with nothing to keep are skipped");
        assert_eq!((items[0].name.as_str(), items[0].username.as_str(), items[0].has_totp), ("GitHub", "me", true));
        assert_eq!(items[1].username, "m@x.com", "the email stands in for a missing username");
        let json = r#"{"version":"1.0","encrypted":false,"vaults":{"v1":{"name":"Personal","items":[{"data":{"type":"login","metadata":{"name":"Bank","note":"n"},"content":{"itemUsername":"u","itemEmail":"e@x.com","password":"pw-bank","urls":["https://bank"],"totpUri":""}}},{"data":{"type":"note","metadata":{"name":"A note","note":"x"},"content":{}}},{"data":{"type":"creditCard","metadata":{"name":"Card","note":""},"content":{"number":"4111"}}}]}}}"#;
        let items = proton_from_json(json);
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].name.as_str(), items[0].username.as_str(), items[0].url.as_str(), items[0].password.as_str()), ("Bank", "u", "https://bank", "pw-bank"));
        let shown = serde_json::to_string(&items).unwrap();
        assert!(!shown.contains("pw-bank"), "a preview has no password: {shown}");
        assert!(proton_from_json("not json").is_empty() && proton_from_csv("").is_empty());
    }
}
