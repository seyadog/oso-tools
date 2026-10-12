//! `oso integration install <agent>`: tells an AI agent that oso exists, in two places: its list of MCP servers (so it
//! can use `oso mcp`) and its global instructions file (so it knows to call `oso_context` first). `uninstall`
//! takes back exactly what was added. Nothing is written without a copy of the old file next to it, and the plan
//! (with what changes line by line) can be seen before anything is done.
//!
//! The formats are those the agents use today:
//! * Claude Code: `mcpServers` in `~/.claude.json`, instructions in `~/.claude/CLAUDE.md`;
//! * OpenCode: `mcp` in `~/.config/opencode/opencode.json` (or `.jsonc`), instructions in `AGENTS.md` beside it;
//! * Codex: a `[mcp_servers.oso]` table in `~/.codex/config.toml`, instructions in `~/.codex/AGENTS.md`.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const BEGIN: &str = "<!-- oso:begin -->";
pub const END: &str = "<!-- oso:end -->";
const TOML_BEGIN: &str = "# oso:begin";
const TOML_END: &str = "# oso:end";

/// What the instructions file gets.
pub const INSTRUCTIONS: &str = "If the environment variable `OSO=1` is set, call `oso_context` before working with servers, credentials or secrets. Never ask the user for passwords or keys: use oso's tools.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agent {
    Claude,
    Opencode,
    Codex,
}

impl Agent {
    pub fn parse(name: &str) -> Option<Agent> {
        match name {
            "claude" => Some(Agent::Claude),
            "opencode" => Some(Agent::Opencode),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Opencode => "opencode",
            Agent::Codex => "codex",
        }
    }
}

/// One file and what it would become.
#[derive(Debug, Clone)]
pub struct Change {
    pub path: PathBuf,
    pub before: Option<String>,
    /// `None`: the file is removed (it would be left empty).
    pub after: Option<String>,
}

impl Change {
    pub fn changed(&self) -> bool {
        self.before != self.after
    }

    /// The lines that change, with a little around them.
    pub fn diff(&self) -> String {
        diff(self.before.as_deref().unwrap_or(""), self.after.as_deref().unwrap_or(""))
    }
}

/// The lines between the common beginning and the common end, as `-` and `+`, with 2 lines of context.
pub fn diff(before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let mut start = 0;
    while start < a.len() && start < b.len() && a[start] == b[start] {
        start += 1;
    }
    let mut end = 0;
    while end < a.len() - start && end < b.len() - start && a[a.len() - 1 - end] == b[b.len() - 1 - end] {
        end += 1;
    }
    if start == a.len() && start == b.len() {
        return String::new();
    }
    let ctx = 2;
    let mut out = String::new();
    for l in &a[start.saturating_sub(ctx)..start] {
        out.push_str(&format!("  {l}\n"));
    }
    for l in &a[start..a.len() - end] {
        out.push_str(&format!("- {l}\n"));
    }
    for l in &b[start..b.len() - end] {
        out.push_str(&format!("+ {l}\n"));
    }
    for l in &a[a.len() - end..(a.len() - end + ctx).min(a.len())] {
        out.push_str(&format!("  {l}\n"));
    }
    out
}

fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

/// `(config file, instructions file)` of an agent under `home`.
pub fn files(agent: Agent, home: &Path) -> (PathBuf, PathBuf) {
    match agent {
        Agent::Claude => (home.join(".claude.json"), home.join(".claude").join("CLAUDE.md")),
        Agent::Opencode => {
            let dir = home.join(".config").join("opencode");
            let cfg = ["opencode.jsonc", "opencode.json"].iter().map(|n| dir.join(n)).find(|p| p.exists()).unwrap_or_else(|| dir.join("opencode.json"));
            (cfg, dir.join("AGENTS.md"))
        }
        Agent::Codex => (home.join(".codex").join("config.toml"), home.join(".codex").join("AGENTS.md")),
    }
}

// ------------------------------------------------------------------ JSON

fn entry(agent: Agent, exe: &str) -> Value {
    match agent {
        Agent::Claude => json!({ "type": "stdio", "command": exe, "args": ["mcp"], "env": {} }),
        _ => json!({ "type": "local", "command": [exe, "mcp"], "enabled": true }),
    }
}

fn key(agent: Agent) -> &'static str {
    if agent == Agent::Opencode { "mcp" } else { "mcpServers" }
}

fn parse_json(path: &Path, text: &str) -> Result<Value> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let v: Value = serde_json::from_str(text).map_err(|_| anyhow::anyhow!("{} has comments (or is not plain JSON): oso will not rewrite it and lose them. Add the entry by hand instead.", path.display()))?;
    if !v.is_object() {
        bail!("{} is not a JSON object", path.display());
    }
    Ok(v)
}

fn dump_json(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

fn json_install(agent: Agent, path: &Path, before: Option<&str>, exe: &str) -> Result<String> {
    let mut v = parse_json(path, before.unwrap_or(""))?;
    let obj = v.as_object_mut().expect("an object");
    if agent == Agent::Opencode && before.is_none_or(|t| t.trim().is_empty()) {
        obj.insert("$schema".into(), "https://opencode.ai/config.json".into());
    }
    let servers = obj.entry(key(agent)).or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else { bail!("`{}` in {} is not an object", key(agent), path.display()) };
    let want = entry(agent, exe);
    if servers.get("oso") == Some(&want) {
        // already there: keep the file as it is, byte for byte
        return Ok(before.unwrap_or("").to_string());
    }
    servers.insert("oso".into(), want);
    Ok(dump_json(&v))
}

fn json_uninstall(agent: Agent, path: &Path, before: &str) -> Result<Option<String>> {
    let mut v = parse_json(path, before)?;
    let obj = v.as_object_mut().expect("an object");
    let Some(servers) = obj.get_mut(key(agent)).and_then(Value::as_object_mut) else { return Ok(Some(before.to_string())) };
    // only the entry oso wrote: one that runs `oso mcp`
    let ours = servers.get("oso").is_some_and(|e| e["args"] == json!(["mcp"]) || e["command"].as_array().is_some_and(|c| c.get(1) == Some(&json!("mcp"))));
    if !ours {
        return Ok(Some(before.to_string()));
    }
    servers.shift_remove("oso");
    if servers.is_empty() {
        obj.shift_remove(key(agent));
    }
    let only_schema = obj.len() == 1 && obj.contains_key("$schema");
    if obj.is_empty() || only_schema && agent == Agent::Opencode {
        return Ok(None);
    }
    Ok(Some(dump_json(&v)))
}

// ------------------------------------------------------------------ TOML (Codex)

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn toml_block(exe: &str) -> String {
    format!("{TOML_BEGIN}\n[mcp_servers.oso]\ncommand = {}\nargs = [\"mcp\"]\n{TOML_END}\n", toml_string(exe))
}

/// `(start, end)` byte range of a delimited block, `end` after its closing line.
fn find_block(text: &str, begin: &str, end: &str) -> Option<(usize, usize)> {
    let s = text.find(begin)?;
    let e = text[s..].find(end)? + s + end.len();
    let e = if text[e..].starts_with('\n') { e + 1 } else { e };
    Some((s, e))
}

fn toml_install(path: &Path, before: Option<&str>, exe: &str) -> Result<String> {
    let text = before.unwrap_or("");
    let block = toml_block(exe);
    if let Some((s, e)) = find_block(text, TOML_BEGIN, TOML_END) {
        return Ok(format!("{}{}{}", &text[..s], block, &text[e..]));
    }
    if text.contains("[mcp_servers.oso]") {
        bail!("{} already has an `oso` entry that was not written by oso: remove it or keep it as it is", path.display());
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    Ok(out)
}

fn toml_uninstall(before: &str) -> Option<String> {
    let Some((s, e)) = find_block(before, TOML_BEGIN, TOML_END) else { return Some(before.to_string()) };
    // the blank line put before the block goes with it
    let head = before[..s].strip_suffix("\n\n").map(|h| format!("{h}\n")).unwrap_or_else(|| before[..s].to_string());
    let out = format!("{head}{}", &before[e..]);
    if out.trim().is_empty() { None } else { Some(out) }
}

// ------------------------------------------------------------------ the instructions file

fn md_block() -> String {
    format!("{BEGIN}\n{INSTRUCTIONS}\n{END}\n")
}

fn md_install(before: Option<&str>) -> String {
    let text = before.unwrap_or("");
    let block = md_block();
    if let Some((s, e)) = find_block(text, BEGIN, END) {
        return format!("{}{}{}", &text[..s], block, &text[e..]);
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

fn md_uninstall(before: &str) -> Option<String> {
    let Some((s, e)) = find_block(before, BEGIN, END) else { return Some(before.to_string()) };
    let head = before[..s].strip_suffix("\n\n").map(|h| format!("{h}\n")).unwrap_or_else(|| before[..s].to_string());
    let out = format!("{head}{}", &before[e..]);
    if out.trim().is_empty() { None } else { Some(out) }
}

// ------------------------------------------------------------------ hooks: what the agent is doing

/// The events of Claude Code's hooks that tell oso what it is doing (verified against its documentation: a handler is
/// `{ "type": "command", "command": … }` inside a matcher group, under the name of the event, in `settings.json`).
pub const CLAUDE_EVENTS: [&str; 6] = ["UserPromptSubmit", "PostToolUse", "PermissionRequest", "Notification", "Stop", "SessionEnd"];

/// `~/.claude/settings.json`, where Claude Code keeps its hooks.
pub fn claude_settings(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

/// The OpenCode plugin oso writes (a plugin is a JavaScript file in `~/.config/opencode/plugins/`).
pub fn opencode_plugin(home: &Path) -> PathBuf {
    home.join(".config").join("opencode").join("plugins").join("oso.js")
}

/// The first line of the plugin: it says the file is oso's, so that it is the only kind of file oso removes.
const PLUGIN_MARK: &str = "// oso-hook-plugin";

fn shell_word(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) }
}

fn claude_command(exe: &str, event: &str) -> String {
    format!("{} hook claude {event}", shell_word(exe))
}

/// Is this hook handler one that oso wrote (it runs `<something named oso…> hook claude <Event>`)?
fn is_ours(handler: &Value) -> bool {
    let Some(cmd) = handler.get("command").and_then(Value::as_str) else { return false };
    let Some((head, event)) = cmd.rsplit_once(" hook claude ") else { return false };
    CLAUDE_EVENTS.contains(&event) && head.trim_matches('\'').rsplit('/').next().is_some_and(|n| n.starts_with("oso"))
}

fn claude_hooks_install(path: &Path, before: Option<&str>, exe: &str) -> Result<String> {
    let mut v = parse_json(path, before.unwrap_or(""))?;
    let obj = v.as_object_mut().expect("an object");
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else { bail!("`hooks` in {} is not an object", path.display()) };
    for event in CLAUDE_EVENTS {
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        let Some(groups) = groups.as_array_mut() else { bail!("`hooks.{event}` in {} is not a list", path.display()) };
        let want = json!({ "type": "command", "command": claude_command(exe, event) });
        let has = |g: &Value| g["hooks"].as_array().is_some_and(|hs| hs.iter().any(|h| *h == want));
        if groups.iter().any(has) {
            continue;
        }
        // our handler from an older install (another path): replace it, never add a second one
        let mut replaced = false;
        for g in groups.iter_mut() {
            if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                for h in hs.iter_mut().filter(|h| is_ours(h)) {
                    *h = want.clone();
                    replaced = true;
                }
            }
        }
        if !replaced {
            groups.push(json!({ "hooks": [want] }));
        }
    }
    let after = dump_json(&v);
    Ok(if before.is_some_and(|b| parse_json(path, b).ok().as_ref() == Some(&v)) { before.unwrap_or("").to_string() } else { after })
}

fn claude_hooks_uninstall(path: &Path, before: &str) -> Result<Option<String>> {
    let mut v = parse_json(path, before)?;
    let obj = v.as_object_mut().expect("an object");
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else { return Ok(Some(before.to_string())) };
    let mut touched = false;
    for event in CLAUDE_EVENTS {
        let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else { continue };
        for g in groups.iter_mut() {
            if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                let n = hs.len();
                hs.retain(|h| !is_ours(h));
                touched |= hs.len() != n;
            }
        }
        // a matcher group that only had ours goes with it; one the user made stays
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|hs| !hs.is_empty()));
        if groups.is_empty() {
            hooks.shift_remove(event);
        }
    }
    if !touched {
        return Ok(Some(before.to_string()));
    }
    if hooks.is_empty() {
        obj.shift_remove("hooks");
    }
    Ok(if obj.is_empty() { None } else { Some(dump_json(&v)) })
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// The OpenCode plugin: on the events that mean "it asks you", "it works" and "it is done", it runs `oso hook opencode <event>`
/// (inside an oso terminal only; anywhere else it does nothing).
pub fn plugin_text(exe: &str) -> String {
    format!(
        r#"{PLUGIN_MARK}: written by `oso integration install opencode`. Tells oso when OpenCode needs you, works or is done.
// Remove it with `oso integration uninstall opencode`.
const OSO = {exe};

export const OsoPlugin = async ({{ $ }}) => {{
  const send = async (event) => {{
    if (!process.env.OSO_PANE) return; // not inside a terminal of oso
    try {{
      await $`${{OSO}} hook opencode ${{event}}`.quiet().nothrow();
    }} catch (_) {{}}
  }};
  return {{
    event: async ({{ event }}) => {{
      switch (event.type) {{
        case "permission.asked":
        case "permission.replied":
        case "session.idle":
        case "session.error":
        case "session.created":
          return send(event.type);
        case "session.status": {{
          const status = event.properties?.status;
          if ((status?.type ?? status) === "busy") return send("session.busy");
        }}
      }}
    }},
  }};
}};
"#,
        exe = js_string(exe)
    )
}

fn plugin_ours(text: &str) -> bool {
    text.starts_with(PLUGIN_MARK)
}

// ------------------------------------------------------------------ the plans

/// What `install` would do to the two files of `agent` under `home`. `exe` is the program that answers `mcp`.
pub fn plan_install(agent: Agent, home: &Path, exe: &str) -> Result<Vec<Change>> {
    let (cfg, instr) = files(agent, home);
    let before_cfg = read(&cfg)?;
    let after_cfg = match agent {
        Agent::Codex => toml_install(&cfg, before_cfg.as_deref(), exe)?,
        _ => json_install(agent, &cfg, before_cfg.as_deref(), exe)?,
    };
    let before_md = read(&instr)?;
    let after_md = md_install(before_md.as_deref());
    let mut out = vec![Change { path: cfg, before: before_cfg, after: Some(after_cfg) }, Change { path: instr, before: before_md, after: Some(after_md) }];
    // and the hooks that tell oso what the agent is doing (Codex has none)
    match agent {
        Agent::Claude => {
            let path = claude_settings(home);
            let before = read(&path)?;
            let after = claude_hooks_install(&path, before.as_deref(), exe)?;
            out.push(Change { path, before, after: Some(after) });
        }
        Agent::Opencode => {
            let path = opencode_plugin(home);
            let before = read(&path)?;
            if before.as_deref().is_some_and(|t| !plugin_ours(t)) {
                bail!("{} exists and was not written by oso: remove it or keep it as it is", path.display());
            }
            out.push(Change { path, before, after: Some(plugin_text(exe)) });
        }
        Agent::Codex => {}
    }
    Ok(out)
}

/// What `uninstall` would do: remove what `install` added and nothing else.
pub fn plan_uninstall(agent: Agent, home: &Path) -> Result<Vec<Change>> {
    let (cfg, instr) = files(agent, home);
    let mut out = vec![];
    if let Some(b) = read(&cfg)? {
        let after = match agent {
            Agent::Codex => toml_uninstall(&b),
            _ => json_uninstall(agent, &cfg, &b)?,
        };
        out.push(Change { path: cfg, before: Some(b), after });
    }
    if let Some(b) = read(&instr)? {
        let after = md_uninstall(&b);
        out.push(Change { path: instr, before: Some(b), after });
    }
    match agent {
        Agent::Claude => {
            let path = claude_settings(home);
            if let Some(b) = read(&path)? {
                let after = claude_hooks_uninstall(&path, &b)?;
                out.push(Change { path, before: Some(b), after });
            }
        }
        Agent::Opencode => {
            let path = opencode_plugin(home);
            if let Some(b) = read(&path)?.filter(|t| plugin_ours(t)) {
                out.push(Change { path, before: Some(b), after: None });
            }
        }
        Agent::Codex => {}
    }
    Ok(out)
}

/// Writes the plan. Every file that changes is first copied to `<name>.oso-backup-<time>`.
pub fn apply(changes: &[Change]) -> Result<Vec<PathBuf>> {
    let mut backups = vec![];
    for c in changes.iter().filter(|c| c.changed()) {
        if c.before.is_some() {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let name = format!("{}.oso-backup-{stamp}", c.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let bak = c.path.with_file_name(name);
            std::fs::copy(&c.path, &bak).with_context(|| format!("could not back up {}", c.path.display()))?;
            backups.push(bak);
        }
        match &c.after {
            Some(text) => {
                if let Some(dir) = c.path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let tmp = c.path.with_extension("oso-tmp");
                std::fs::write(&tmp, text)?;
                std::fs::rename(&tmp, &c.path)?;
            }
            None => std::fs::remove_file(&c.path)?,
        }
    }
    Ok(backups)
}

/// Is `oso mcp` registered with this agent? (Only reads the agent's files.)
pub fn installed(agent: Agent, home: &Path) -> bool {
    let (cfg, _) = files(agent, home);
    let Ok(Some(text)) = read(&cfg) else { return false };
    match agent {
        Agent::Codex => text.contains(TOML_BEGIN) && text.contains("[mcp_servers.oso]"),
        _ => parse_json(&cfg, &text).ok().is_some_and(|v| v.get(key(agent)).and_then(|m| m.get("oso")).is_some_and(|e| e["args"] == json!(["mcp"]) || e["command"].as_array().is_some_and(|c| c.get(1) == Some(&json!("mcp"))))),
    }
}

/// `oso integration <install|uninstall> <claude|opencode|codex> [--home DIR] [--dry-run]`: prints what changes in
/// each file (a diff) and, unless `--dry-run`, does it, keeping a copy of every file it changes.
pub fn cli(args: &[String], exe: &str, out: &mut dyn std::io::Write) -> Result<()> {
    const USAGE: &str = "usage: oso integration <install|uninstall> <claude|opencode|codex> [--dry-run] [--home DIR]";
    let mut positional = vec![];
    let mut dry = false;
    let mut home: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" => dry = true,
            "--home" => home = Some(PathBuf::from(it.next().context("--home needs a folder")?)),
            f if f.starts_with("--") => bail!("unknown option {f}\n{USAGE}"),
            _ => positional.push(a.as_str()),
        }
    }
    let [action, agent] = positional.as_slice() else { bail!("{USAGE}") };
    let agent = Agent::parse(agent).with_context(|| format!("unknown agent {agent:?}: claude, opencode or codex"))?;
    let home = home.or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())).context("no home folder")?;
    let plan = match *action {
        "install" => plan_install(agent, &home, exe)?,
        "uninstall" => plan_uninstall(agent, &home)?,
        _ => bail!("{USAGE}"),
    };
    for c in &plan {
        if c.changed() {
            writeln!(out, "{}:\n{}", c.path.display(), c.diff())?;
        } else {
            writeln!(out, "{}: nothing to change", c.path.display())?;
        }
    }
    if dry {
        writeln!(out, "(dry run: nothing was written)")?;
        return Ok(());
    }
    let backups = apply(&plan)?;
    for b in &backups {
        writeln!(out, "kept a copy of the old file: {}", b.display())?;
    }
    writeln!(out, "{}", if plan.iter().any(Change::changed) { "Done." } else { "Already so: nothing changed." })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-integration-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const EXE: &str = "/home/me/.local/bin/oso-app";

    /// A `~/.claude.json` like a real one: lots that is not ours, and a server of the user's own.
    const CLAUDE_JSON: &str = r#"{
  "numStartups": 12,
  "projects": { "/work/x": { "mcpServers": {}, "allowedTools": [] } },
  "mcpServers": {
    "mine": { "type": "stdio", "command": "node", "args": ["server.js"], "env": { "A": "1" } }
  },
  "theme": "dark"
}
"#;

    fn run(agent: Agent, h: &Path) -> Vec<Change> {
        let plan = plan_install(agent, h, EXE).unwrap();
        apply(&plan).unwrap();
        plan
    }

    #[test]
    fn claude_gets_an_mcp_entry_and_the_instructions_and_keeps_everything_else() {
        let h = home("claude");
        put(&h.join(".claude.json"), CLAUDE_JSON);
        put(&h.join(".claude/CLAUDE.md"), "# My rules\n\nBe brief.\n");
        let plan = run(Agent::Claude, &h);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["oso"], json!({ "type": "stdio", "command": EXE, "args": ["mcp"], "env": {} }));
        assert_eq!(v["mcpServers"]["mine"]["args"], json!(["server.js"]), "the user's own server is untouched");
        assert_eq!((v["numStartups"].as_i64(), v["theme"].as_str()), (Some(12), Some("dark")));
        assert!(v["projects"]["/work/x"]["mcpServers"].is_object());
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["numStartups", "projects", "mcpServers", "theme"], "the order of the keys is kept");
        let md = std::fs::read_to_string(h.join(".claude/CLAUDE.md")).unwrap();
        assert!(md.starts_with("# My rules\n\nBe brief.\n\n<!-- oso:begin -->\n") && md.ends_with("<!-- oso:end -->\n") && md.contains("call `oso_context`"), "{md}");
        assert!(plan[0].diff().contains("+     \"oso\": {"), "the plan shows what changes:\n{}", plan[0].diff());
        // a copy of each old file is beside it
        let backups: Vec<String> = std::fs::read_dir(&h).unwrap().chain(std::fs::read_dir(h.join(".claude")).unwrap()).flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains("oso-backup")).collect();
        assert_eq!(backups.len(), 2, "{backups:?}");
        let original = std::fs::read_to_string(h.join(".claude.json").with_file_name(backups.iter().find(|b| b.starts_with(".claude.json")).unwrap())).unwrap();
        assert_eq!(original, CLAUDE_JSON);
    }

    #[test]
    fn installing_twice_changes_nothing_the_second_time() {
        for agent in [Agent::Claude, Agent::Opencode, Agent::Codex] {
            let h = home(&format!("twice-{}", agent.name()));
            run(agent, &h);
            let (cfg, instr) = files(agent, &h);
            let (a, b) = (std::fs::read_to_string(&cfg).unwrap(), std::fs::read_to_string(&instr).unwrap());
            let again = plan_install(agent, &h, EXE).unwrap();
            assert!(again.iter().all(|c| !c.changed()), "{}: {:?}", agent.name(), again.iter().map(|c| c.diff()).collect::<Vec<_>>());
            apply(&again).unwrap();
            assert_eq!((std::fs::read_to_string(&cfg).unwrap(), std::fs::read_to_string(&instr).unwrap()), (a, b));
            assert!(std::fs::read_dir(cfg.parent().unwrap()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("oso-backup-") || e.file_name().to_string_lossy().matches("oso-backup").count() == 1), "no backup of an unchanged file");
        }
    }

    #[test]
    fn uninstall_gives_back_what_there_was() {
        let h = home("undo-claude");
        put(&h.join(".claude.json"), CLAUDE_JSON);
        put(&h.join(".claude/CLAUDE.md"), "# My rules\n\nBe brief.\n");
        run(Agent::Claude, &h);
        apply(&plan_uninstall(Agent::Claude, &h).unwrap()).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(after, serde_json::from_str::<Value>(CLAUDE_JSON).unwrap());
        assert_eq!(std::fs::read_to_string(h.join(".claude/CLAUDE.md")).unwrap(), "# My rules\n\nBe brief.\n", "the instructions file is exactly as it was");
        // and the plan of an uninstall that has nothing to remove is empty of changes
        assert!(plan_uninstall(Agent::Claude, &h).unwrap().iter().all(|c| !c.changed()));
    }

    #[test]
    fn files_that_oso_created_are_removed_again() {
        for agent in [Agent::Claude, Agent::Opencode, Agent::Codex] {
            let h = home(&format!("fresh-{}", agent.name()));
            run(agent, &h);
            let (cfg, instr) = files(agent, &h);
            assert!(cfg.exists() && instr.exists());
            apply(&plan_uninstall(agent, &h).unwrap()).unwrap();
            let left = |p: &Path| std::fs::read_to_string(p).ok().filter(|t| t.trim() != "{}");
            assert!(left(&cfg).is_none(), "{}: {:?}", agent.name(), left(&cfg));
            assert!(!instr.exists());
        }
    }

    #[test]
    fn opencode_uses_the_local_server_shape_in_the_file_it_already_has() {
        let h = home("opencode");
        put(&h.join(".config/opencode/opencode.jsonc"), "{\n  \"$schema\": \"https://opencode.ai/config.json\"\n}\n");
        run(Agent::Opencode, &h);
        assert!(!h.join(".config/opencode/opencode.json").exists(), "the .jsonc that was there is the one edited");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".config/opencode/opencode.jsonc")).unwrap()).unwrap();
        assert_eq!(v["mcp"]["oso"], json!({ "type": "local", "command": [EXE, "mcp"], "enabled": true }));
        assert_eq!(v["$schema"], "https://opencode.ai/config.json");
        assert!(std::fs::read_to_string(h.join(".config/opencode/AGENTS.md")).unwrap().contains(BEGIN));
        apply(&plan_uninstall(Agent::Opencode, &h).unwrap()).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".config/opencode/opencode.jsonc")).map(|t| t).unwrap_or_else(|_| "{}".into())).unwrap_or(json!({}));
        assert!(v.get("mcp").is_none());
    }

    #[test]
    fn a_config_with_comments_is_never_rewritten() {
        let h = home("comments");
        let with = "{\n  // my favourite model\n  \"model\": \"x\"\n}\n";
        put(&h.join(".config/opencode/opencode.jsonc"), with);
        let err = plan_install(Agent::Opencode, &h, EXE).unwrap_err().to_string();
        assert!(err.contains("has comments") && err.contains("by hand"), "{err}");
        assert_eq!(std::fs::read_to_string(h.join(".config/opencode/opencode.jsonc")).unwrap(), with);
    }

    #[test]
    fn codex_gets_a_delimited_toml_table_and_keeps_its_other_settings() {
        let h = home("codex");
        let toml = "model = \"gpt\"\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        put(&h.join(".codex/config.toml"), toml);
        run(Agent::Codex, &h);
        let t = std::fs::read_to_string(h.join(".codex/config.toml")).unwrap();
        assert!(t.starts_with(toml) && t.contains(&format!("{TOML_BEGIN}\n[mcp_servers.oso]\ncommand = \"{EXE}\"\nargs = [\"mcp\"]\n{TOML_END}\n")), "{t}");
        // a changed path is updated in place, not added twice
        let plan = plan_install(Agent::Codex, &h, "/other/oso").unwrap();
        apply(&plan).unwrap();
        let t = std::fs::read_to_string(h.join(".codex/config.toml")).unwrap();
        assert_eq!(t.matches("[mcp_servers.oso]").count(), 1);
        assert!(t.contains("/other/oso"));
        apply(&plan_uninstall(Agent::Codex, &h).unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(h.join(".codex/config.toml")).unwrap(), toml, "exactly as it was");
        // an entry somebody wrote by hand is not touched
        put(&h.join(".codex/config.toml"), "[mcp_servers.oso]\ncommand = \"mine\"\n");
        assert!(plan_install(Agent::Codex, &h, EXE).unwrap_err().to_string().contains("not written by oso"));
        assert!(plan_uninstall(Agent::Codex, &h).unwrap().iter().all(|c| !c.changed()));
    }

    #[test]
    fn the_command_line_shows_the_diff_and_honours_dry_run_and_home() {
        let h = home("cli");
        let run = |args: &[&str]| {
            let mut out = Vec::new();
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let r = cli(&args, EXE, &mut out);
            (r, String::from_utf8(out).unwrap())
        };
        let hs = h.to_string_lossy().to_string();
        let (r, text) = run(&["install", "claude", "--dry-run", "--home", &hs]);
        assert!(r.is_ok() && text.contains("+") && text.contains("dry run") && !h.join(".claude.json").exists(), "{text}");
        let (r, text) = run(&["install", "claude", "--home", &hs]);
        assert!(r.is_ok() && text.contains("Done.") && h.join(".claude.json").exists(), "{text}");
        let (_, text) = run(&["install", "claude", "--home", &hs]);
        assert!(text.contains("Already so") && text.matches("nothing to change").count() == 3, "{text}");
        let (r, _) = run(&["uninstall", "claude", "--home", &hs]);
        assert!(r.is_ok() && !h.join(".claude.json").exists() || std::fs::read_to_string(h.join(".claude.json")).unwrap().trim() == "{}");
        assert!(run(&["install", "emacs", "--home", &hs]).0.is_err());
        assert!(run(&["install"]).0.is_err() && run(&["frobnicate", "claude"]).0.is_err() && run(&["install", "claude", "--bogus"]).0.is_err());
    }

    const SETTINGS: &str = r#"{
  "theme": "dark",
  "hooks": {
    "Stop": [ { "hooks": [ { "type": "command", "command": "~/bin/my-stop-hook.sh" } ] } ],
    "PreToolUse": [ { "matcher": "Bash", "hooks": [ { "type": "command", "command": "~/bin/guard.sh" } ] } ]
  }
}
"#;

    fn commands(v: &Value, event: &str) -> Vec<String> {
        v["hooks"][event].as_array().map(|gs| gs.iter().flat_map(|g| g["hooks"].as_array().cloned().unwrap_or_default()).filter_map(|h| h["command"].as_str().map(str::to_string)).collect()).unwrap_or_default()
    }

    #[test]
    fn claude_gets_hooks_that_tell_oso_what_it_does_and_the_users_own_hooks_stay() {
        let h = home("claude-hooks");
        put(&h.join(".claude/settings.json"), SETTINGS);
        let plan = run(Agent::Claude, &h);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        for event in CLAUDE_EVENTS {
            assert!(commands(&v, event).contains(&format!("{EXE} hook claude {event}")), "{event}");
        }
        assert_eq!(commands(&v, "Stop"), ["~/bin/my-stop-hook.sh", &format!("{EXE} hook claude Stop")], "the user's hook is still there, first");
        assert_eq!(commands(&v, "PreToolUse"), ["~/bin/guard.sh"], "an event of theirs that oso does not use is untouched");
        assert!(plan.iter().any(|c| c.path.ends_with("settings.json") && c.changed()));
        // twice: nothing changes, byte for byte
        let before = std::fs::read_to_string(h.join(".claude/settings.json")).unwrap();
        let again = run(Agent::Claude, &h);
        assert!(again.iter().all(|c| !c.changed()), "{:?}", again.iter().map(|c| c.diff()).collect::<Vec<_>>());
        assert_eq!(std::fs::read_to_string(h.join(".claude/settings.json")).unwrap(), before);
        // another path for the program: our handler is replaced, never doubled
        let moved = plan_install(Agent::Claude, &h, "/opt/oso/oso").unwrap();
        apply(&moved).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(commands(&v, "Stop"), ["~/bin/my-stop-hook.sh", "/opt/oso/oso hook claude Stop"]);
        // uninstall takes back exactly ours
        apply(&plan_uninstall(Agent::Claude, &h).unwrap()).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(commands(&v, "Stop"), ["~/bin/my-stop-hook.sh"]);
        assert_eq!(commands(&v, "PreToolUse"), ["~/bin/guard.sh"]);
        assert!(v["hooks"].get("UserPromptSubmit").is_none() && v["hooks"].get("SessionEnd").is_none(), "events that only had ours are gone");
        assert_eq!(v["theme"], "dark");
    }

    #[test]
    fn hooks_written_from_nothing_are_removed_with_their_file_and_a_path_with_spaces_is_quoted() {
        let h = home("claude-hooks-new");
        apply(&plan_install(Agent::Claude, &h, "/home/me/my tools/oso").unwrap()).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(h.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(commands(&v, "Stop"), ["'/home/me/my tools/oso' hook claude Stop"]);
        apply(&plan_uninstall(Agent::Claude, &h).unwrap()).unwrap();
        assert!(!h.join(".claude/settings.json").exists(), "oso made the file, oso takes it away");
        // a settings.json with comments is never rewritten
        put(&h.join(".claude/settings.json"), "{ // mine\n \"theme\": \"dark\" }");
        assert!(plan_install(Agent::Claude, &h, EXE).unwrap_err().to_string().contains("comments"));
    }

    #[test]
    fn opencode_gets_a_plugin_file_that_is_oso_s_and_only_that_kind_of_file_is_removed() {
        let h = home("opencode-plugin");
        let plan = run(Agent::Opencode, &h);
        let path = opencode_plugin(&h);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(plan.iter().any(|c| c.path == path && c.changed()));
        assert!(text.starts_with("// oso-hook-plugin") && text.contains(&format!("const OSO = {};", js_string(EXE))));
        for event in ["permission.asked", "permission.replied", "session.idle", "session.error", "session.created"] {
            assert!(text.contains(&format!("case \"{event}\"")), "{event}");
        }
        assert!(text.contains("session.busy") && text.contains("process.env.OSO_PANE"));
        // the file is valid JavaScript (checked with node when it is here)
        if let Ok(node) = std::process::Command::new("node").arg("--check").arg(&path).output() {
            assert!(node.status.success(), "{}", String::from_utf8_lossy(&node.stderr));
        }
        assert!(run(Agent::Opencode, &h).iter().all(|c| !c.changed()), "twice changes nothing");
        apply(&plan_uninstall(Agent::Opencode, &h).unwrap()).unwrap();
        assert!(!path.exists());
        // a file with that name that is not oso's is neither overwritten nor removed
        put(&path, "export const Mine = async () => ({});\n");
        assert!(plan_install(Agent::Opencode, &h, EXE).unwrap_err().to_string().contains("not written by oso"));
        assert!(plan_uninstall(Agent::Opencode, &h).unwrap().iter().all(|c| c.path != path));
        // a program path with quotes and backslashes stays a valid string
        let odd = plugin_text("/home/me/\"odd\"\\dir/oso");
        assert!(odd.contains(r#"const OSO = "/home/me/\"odd\"\\dir/oso";"#), "{odd}");
    }

    #[test]
    fn it_can_tell_whether_an_agent_is_connected() {
        for agent in [Agent::Claude, Agent::Opencode, Agent::Codex] {
            let h = home(&format!("status-{}", agent.name()));
            assert!(!installed(agent, &h));
            run(agent, &h);
            assert!(installed(agent, &h), "{}", agent.name());
            apply(&plan_uninstall(agent, &h).unwrap()).unwrap();
            assert!(!installed(agent, &h));
        }
        let h = home("status-other");
        put(&h.join(".claude.json"), "{ \"mcpServers\": { \"oso\": { \"command\": \"x\", \"args\": [\"serve\"] } } }");
        assert!(!installed(Agent::Claude, &h), "an entry that is not ours does not count");
    }

    #[test]
    fn a_path_with_quotes_is_escaped_in_toml() {
        assert_eq!(toml_string("/a \"b\"/c\\d"), "\"/a \\\"b\\\"/c\\\\d\"");
    }

    #[test]
    fn an_entry_that_is_not_ours_is_left_by_uninstall() {
        let h = home("not-ours");
        put(&h.join(".claude.json"), "{ \"mcpServers\": { \"oso\": { \"type\": \"stdio\", \"command\": \"other-tool\", \"args\": [\"serve\"] } } }\n");
        assert!(plan_uninstall(Agent::Claude, &h).unwrap().iter().all(|c| !c.changed()));
    }

    #[test]
    fn the_diff_shows_the_changed_lines_with_a_little_around_them() {
        let d = diff("a\nb\nc\nd\ne\n", "a\nb\nX\nY\nd\ne\n");
        assert_eq!(d, "  a\n  b\n- c\n+ X\n+ Y\n  d\n  e\n");
        assert_eq!(diff("same\n", "same\n"), "");
    }
}
