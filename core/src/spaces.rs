//! Spaces: one unit of work. A Space groups the folders (repos), servers, secrets and tunnels a piece of work uses, some
//! notes, and the layout of its tabs. It is the replacement of "groups" (saved tabs): `groups.json` is migrated into
//! Spaces the first time (see `migrate_groups`).
//!
//! A Space is also what limits an AI agent: the terminals opened in a Space are bound to it, and the MCP of an agent
//! running there sees only what the Space holds (see `agent_api`). Ids in a Space may point at things that were deleted
//! later; they are skipped when used, never an error.

use crate::store::{Folder, Store};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The file, next to `servers.json`. (`spaces.json` is not used: an unrelated file of the old terminal version of the
/// program can be there.)
pub const FILE: &str = "oso-spaces.json";

const MAX_SPACES: usize = 200;
const MAX_LIST: usize = 500;
const MAX_TABS: usize = 50;
const MAX_PANES: usize = 32;
const MAX_NOTES: usize = 20_000;
const MAX_COMMAND: usize = 2_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TunnelRef {
    pub server: u64,
    pub forward: u64,
}

/// One terminal of a saved tab.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SpacePane {
    /// The server (`0`: this computer).
    #[serde(default)]
    pub server: u64,
    /// Its name when it was saved, to say what is missing if the server is deleted.
    #[serde(default)]
    pub name: String,
    /// Typed (and Enter) once the terminal shows its prompt. Empty: nothing.
    #[serde(default)]
    pub command: String,
    /// Where a local terminal opens (empty: the Space's first folder).
    #[serde(default)]
    pub cwd: String,
}

/// A tab of a Space: the arrangement (the window's tiling tree, kept as it is) and its terminals.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SpaceTab {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub broadcast: bool,
    /// The tiling tree with the terminals numbered 0, 1, 2… as in `panes`.
    pub shape: Value,
    pub panes: Vec<SpacePane>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Space {
    /// 0 for one that is not saved yet.
    #[serde(default)]
    pub id: u64,
    pub name: String,
    /// The profile to work in when it opens (`None`: whichever is active).
    #[serde(default)]
    pub profile: Option<String>,
    /// Work folders (repos), absolute paths (`~` allowed).
    #[serde(default)]
    pub folders: Vec<String>,
    /// Single servers.
    #[serde(default)]
    pub servers: Vec<u64>,
    /// Folders of the server tree: everything below them counts.
    #[serde(default)]
    pub server_folders: Vec<u64>,
    /// Secrets of the vault (API keys, tokens, SSH keys…).
    #[serde(default)]
    pub secrets: Vec<u64>,
    /// Tunnels that are opened with the Space.
    #[serde(default)]
    pub tunnels: Vec<TunnelRef>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tabs: Vec<SpaceTab>,
}

/// Ids of the folder `root` and every folder below it.
fn folder_subtree(folders: &[Folder], root: u64) -> BTreeSet<u64> {
    let mut out = BTreeSet::from([root]);
    let mut grew = true;
    while grew {
        grew = false;
        for f in folders {
            if f.parent.is_some_and(|p| out.contains(&p)) && out.insert(f.id) {
                grew = true;
            }
        }
    }
    out
}

impl Space {
    /// Every server this Space covers: its single servers, and everything under its server folders (any depth).
    /// Ids of servers that no longer exist are not in the answer.
    pub fn server_ids(&self, store: &Store) -> BTreeSet<u64> {
        let mut out: BTreeSet<u64> = self.servers.iter().copied().filter(|id| store.server(*id).is_some()).collect();
        for root in &self.server_folders {
            if !store.folders.iter().any(|f| f.id == *root) {
                continue;
            }
            let tree = folder_subtree(&store.folders, *root);
            out.extend(store.servers.iter().filter(|s| s.parent.is_some_and(|p| tree.contains(&p))).map(|s| s.id));
        }
        out
    }

    pub fn covers_server(&self, store: &Store, id: u64) -> bool {
        self.server_ids(store).contains(&id)
    }

    pub fn has_secret(&self, id: u64) -> bool {
        self.secrets.contains(&id)
    }

    /// The first work folder, with `~` expanded (where a terminal of the Space opens by default).
    pub fn first_folder(&self) -> Option<PathBuf> {
        self.folders.iter().map(|f| expand_home(f)).find(|p| !p.as_os_str().is_empty())
    }

    /// What is wrong with it, in words, or a cleaned copy: trimmed names, no duplicates, sensible sizes.
    pub fn cleaned(&self) -> Result<Space> {
        let mut s = self.clone();
        s.name = s.name.trim().chars().take(80).collect();
        if s.name.is_empty() {
            bail!("a Space needs a name");
        }
        s.profile = s.profile.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        s.folders = s.folders.iter().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
        if s.folders.iter().any(|f| f.contains('\0') || f.contains('\n')) {
            bail!("a folder is a path, on one line");
        }
        dedup(&mut s.folders);
        dedup(&mut s.servers);
        dedup(&mut s.server_folders);
        dedup(&mut s.secrets);
        let mut seen = BTreeSet::new();
        s.tunnels.retain(|t| seen.insert((t.server, t.forward)));
        if [s.folders.len(), s.servers.len(), s.server_folders.len(), s.secrets.len(), s.tunnels.len()].iter().any(|n| *n > MAX_LIST) {
            bail!("a Space holds at most {MAX_LIST} of each thing");
        }
        if s.notes.len() > MAX_NOTES {
            bail!("the notes are too long (at most {MAX_NOTES} characters)");
        }
        if s.tabs.len() > MAX_TABS {
            bail!("a Space holds at most {MAX_TABS} tabs");
        }
        for t in &mut s.tabs {
            if t.panes.is_empty() || t.panes.len() > MAX_PANES || !valid_shape(&t.shape, t.panes.len()) {
                bail!("a saved tab does not match its arrangement");
            }
            t.name = t.name.trim().chars().take(80).collect();
            for p in &mut t.panes {
                p.command = p.command.chars().take(MAX_COMMAND).collect();
                p.name = p.name.chars().take(80).collect();
                if p.cwd.contains('\0') || p.cwd.contains('\n') {
                    bail!("a folder is a path, on one line");
                }
            }
            t.broadcast = t.broadcast && t.panes.len() > 1;
        }
        Ok(s)
    }
}

fn dedup<T: Ord + Clone>(v: &mut Vec<T>) {
    let mut seen = BTreeSet::new();
    v.retain(|x| seen.insert(x.clone()));
}

pub fn expand_home(path: &str) -> PathBuf {
    let p = path.trim();
    if p == "~" || p.starts_with("~/") {
        if let Some(h) = directories::BaseDirs::new() {
            return h.home_dir().join(p.trim_start_matches('~').trim_start_matches('/'));
        }
    }
    PathBuf::from(p)
}

/// A shape is valid when it holds exactly the terminals 0..n-1, each once (the same rule as the window's `validShape`).
pub fn valid_shape(shape: &Value, n: usize) -> bool {
    fn walk(v: &Value, out: &mut Vec<u64>) -> bool {
        match v.get("kind").and_then(Value::as_str) {
            Some("leaf") => match v.get("id").and_then(Value::as_u64) {
                Some(id) => {
                    out.push(id);
                    true
                }
                None => false,
            },
            Some("split") => v.get("side").is_some_and(Value::is_boolean) && v.get("pm").is_some_and(Value::is_number) && v.get("a").is_some_and(|a| walk(a, out)) && v.get("b").is_some_and(|b| walk(b, out)),
            _ => false,
        }
    }
    let mut ids = vec![];
    if !walk(shape, &mut ids) {
        return false;
    }
    ids.sort_unstable();
    ids.len() == n && ids.iter().enumerate().all(|(i, id)| *id == i as u64)
}

// ------------------------------------------------------------------ migration of groups

/// What the migration did, for the window to say once.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Migration {
    pub imported: Vec<String>,
    /// Groups that were not valid, by name (or position).
    pub skipped: Vec<String>,
}

/// Turns the text of `groups.json` into Spaces, one per valid group: the group's name, the servers of its panes, and one
/// tab (its arrangement, commands and broadcast). A group that is not valid is skipped and named in the report.
pub fn migrate_groups(text: &str) -> (Vec<Space>, Migration) {
    let mut report = Migration::default();
    let Ok(Value::Array(groups)) = serde_json::from_str::<Value>(text) else { return (vec![], report) };
    let mut spaces = vec![];
    for (i, g) in groups.iter().enumerate() {
        let name = g.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Group").to_string();
        let label = if g.get("name").is_some() { name.clone() } else { format!("group {}", i + 1) };
        let panes: Option<Vec<SpacePane>> = g.get("panes").and_then(Value::as_array).filter(|p| !p.is_empty()).map(|ps| {
            ps.iter()
                .map(|p| SpacePane {
                    server: p.get("server").and_then(Value::as_u64).unwrap_or(0),
                    name: p.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    command: p.get("command").and_then(Value::as_str).unwrap_or("").to_string(),
                    cwd: String::new(),
                })
                .collect()
        });
        let shape = g.get("shape").cloned().unwrap_or(Value::Null);
        let Some(panes) = panes.filter(|p| valid_shape(&shape, p.len())) else {
            report.skipped.push(label);
            continue;
        };
        let mut servers: Vec<u64> = panes.iter().map(|p| p.server).filter(|s| *s != 0).collect();
        dedup(&mut servers);
        let tab = SpaceTab { name: name.clone(), broadcast: g.get("broadcast").and_then(Value::as_bool).unwrap_or(false) && panes.len() > 1, shape, panes };
        let space = Space { id: spaces.len() as u64 + 1, name, servers, tabs: vec![tab], ..Default::default() };
        match space.cleaned() {
            Ok(s) => {
                report.imported.push(s.name.clone());
                spaces.push(s);
            }
            Err(_) => report.skipped.push(label),
        }
    }
    (spaces, report)
}

// ------------------------------------------------------------------ the saved list

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    spaces: Vec<Space>,
}

/// The Spaces on disk (`oso-spaces.json`).
pub struct Spaces {
    path: PathBuf,
    next_id: u64,
    spaces: Vec<Space>,
    /// Set when `groups.json` was migrated at load, until the window has told the person.
    migration: Option<Migration>,
}

impl Spaces {
    /// Reads the Spaces of a config folder. If there is no `oso-spaces.json` but there is a `groups.json`, the groups
    /// become Spaces and `groups.json` is renamed to `groups.json.migrated` (never deleted). Both present: the groups are
    /// left alone, they are never imported twice.
    pub fn load_from(dir: &Path) -> Spaces {
        let path = dir.join(FILE);
        let mut me = Spaces { path: path.clone(), next_id: 1, spaces: vec![], migration: None };
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(file) = serde_json::from_str::<File>(&text) {
                me.spaces = file.spaces;
                me.next_id = file.next_id.max(me.spaces.iter().map(|s| s.id).max().unwrap_or(0) + 1).max(1);
            }
            return me;
        }
        let groups = dir.join("groups.json");
        if let Ok(text) = std::fs::read_to_string(&groups) {
            let (spaces, report) = migrate_groups(&text);
            me.next_id = spaces.len() as u64 + 1;
            me.spaces = spaces;
            // the Spaces are written first; only then is the old file put aside
            if me.save().is_ok() {
                let _ = std::fs::rename(&groups, dir.join("groups.json.migrated"));
                me.migration = Some(report);
            }
        }
        me
    }

    pub fn list(&self) -> &[Space] {
        &self.spaces
    }

    pub fn get(&self, id: u64) -> Option<&Space> {
        self.spaces.iter().find(|s| s.id == id)
    }

    /// The report of the migration, once.
    pub fn take_migration(&mut self) -> Option<Migration> {
        self.migration.take()
    }

    /// Saves a new Space (`id` 0) or replaces the one with that id. Returns its id.
    pub fn save_space(&mut self, space: &Space) -> Result<u64> {
        let mut clean = space.cleaned()?;
        if clean.id == 0 {
            if self.spaces.len() >= MAX_SPACES {
                bail!("there are already {MAX_SPACES} Spaces");
            }
            clean.id = self.next_id;
            self.next_id += 1;
            self.spaces.push(clean.clone());
        } else {
            let slot = self.spaces.iter_mut().find(|s| s.id == clean.id).context("this Space no longer exists")?;
            *slot = clean.clone();
        }
        self.save()?;
        Ok(clean.id)
    }

    pub fn delete(&mut self, id: u64) -> Result<()> {
        self.spaces.retain(|s| s.id != id);
        self.save()
    }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&File { next_id: self.next_id, spaces: self.spaces.clone() })?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Folder, Server};
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-spaces-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn leaf(i: u64) -> Value {
        json!({ "kind": "leaf", "id": i })
    }

    fn split(a: Value, b: Value) -> Value {
        json!({ "kind": "split", "side": true, "pm": 500, "a": a, "b": b })
    }

    #[test]
    fn a_shape_holds_exactly_its_terminals() {
        assert!(valid_shape(&leaf(0), 1));
        assert!(valid_shape(&split(leaf(1), split(leaf(0), leaf(2))), 3));
        assert!(!valid_shape(&split(leaf(0), leaf(0)), 2), "a terminal twice");
        assert!(!valid_shape(&split(leaf(0), leaf(2)), 2), "a hole");
        assert!(!valid_shape(&leaf(0), 2), "a terminal missing");
        assert!(!valid_shape(&json!({ "kind": "split", "a": leaf(0), "b": leaf(1) }), 2), "a split needs its side and size");
        assert!(!valid_shape(&Value::Null, 1));
    }

    #[test]
    fn groups_become_spaces_one_each_and_the_invalid_ones_are_named() {
        let text = json!([
            { "id": 1, "name": "ops", "broadcast": true, "shape": split(leaf(0), leaf(1)), "panes": [ { "server": 3, "name": "web-1", "command": "tail -f x" }, { "server": 0, "name": "Local", "command": "" } ] },
            { "id": 2, "name": "broken", "broadcast": false, "shape": split(leaf(0), leaf(5)), "panes": [ { "server": 3, "name": "web-1", "command": "" }, { "server": 4, "name": "db", "command": "" } ] },
            { "id": 3, "name": "solo", "broadcast": true, "shape": leaf(0), "panes": [ { "server": 4, "name": "db-1", "command": "" } ] },
            { "id": 4, "name": "twice", "shape": split(leaf(0), leaf(1)), "panes": [ { "server": 4 }, { "server": 4 } ] },
            { "id": 5, "name": "no panes", "shape": leaf(0), "panes": [] },
            "nonsense"
        ])
        .to_string();
        let (spaces, report) = migrate_groups(&text);
        assert_eq!(spaces.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ops", "solo", "twice"]);
        assert_eq!(report.imported, ["ops", "solo", "twice"]);
        assert_eq!(report.skipped, ["broken", "no panes", "group 6"]);
        let ops = &spaces[0];
        assert_eq!((ops.id, ops.servers.clone(), ops.tabs.len()), (1, vec![3], 1), "this computer is not a server of the Space");
        assert_eq!((ops.tabs[0].name.as_str(), ops.tabs[0].broadcast, ops.tabs[0].panes[0].command.as_str()), ("ops", true, "tail -f x"));
        assert!(!spaces[1].tabs[0].broadcast, "a lone terminal has nobody to broadcast to");
        assert_eq!(spaces[2].servers, vec![4], "the same server twice is one");
        assert!(ops.folders.is_empty() && ops.secrets.is_empty() && ops.tunnels.is_empty() && ops.notes.is_empty() && ops.profile.is_none());
        assert_eq!(migrate_groups("not json").0.len() + migrate_groups("{}").0.len(), 0);
    }

    #[test]
    fn the_migration_runs_once_keeps_the_old_file_and_never_imports_twice() {
        let dir = temp("migrate");
        let groups = json!([{ "id": 1, "name": "ops", "broadcast": false, "shape": leaf(0), "panes": [{ "server": 2, "name": "web", "command": "" }] }]).to_string();
        std::fs::write(dir.join("groups.json"), &groups).unwrap();
        let mut s = Spaces::load_from(&dir);
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.take_migration().unwrap().imported, ["ops"]);
        assert!(s.take_migration().is_none(), "the report is given once");
        assert!(dir.join(FILE).exists() && !dir.join("groups.json").exists(), "the groups were put aside");
        assert_eq!(std::fs::read_to_string(dir.join("groups.json.migrated")).unwrap(), groups, "and are kept as they were");
        // a second start reads the Spaces and does not migrate
        let again = Spaces::load_from(&dir);
        assert_eq!(again.list().len(), 1);
        // a groups.json that comes back (a restored backup) is not imported over existing Spaces
        std::fs::write(dir.join("groups.json"), &groups).unwrap();
        let third = Spaces::load_from(&dir);
        assert_eq!(third.list().len(), 1);
        assert!(dir.join("groups.json").exists(), "left alone");
        // no groups.json and no Spaces: nothing, and no file is made
        let empty = temp("migrate-none");
        assert!(Spaces::load_from(&empty).list().is_empty() && !empty.join(FILE).exists());
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(empty).ok();
    }

    #[test]
    fn spaces_are_saved_changed_deleted_and_get_ids_that_are_not_reused() {
        let dir = temp("save");
        let mut s = Spaces::load_from(&dir);
        let a = s.save_space(&Space { name: "  Acme  ".into(), folders: vec!["/srv/a".into(), "/srv/a".into(), " ".into()], servers: vec![3, 3], ..Default::default() }).unwrap();
        let b = s.save_space(&Space { name: "Beta".into(), ..Default::default() }).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(s.get(a).unwrap().name, "Acme");
        assert_eq!((s.get(a).unwrap().folders.clone(), s.get(a).unwrap().servers.clone()), (vec!["/srv/a".to_string()], vec![3]));
        s.delete(b).unwrap();
        let c = s.save_space(&Space { name: "Gamma".into(), ..Default::default() }).unwrap();
        assert_eq!(c, 3, "a deleted id is not given again");
        let mut changed = s.get(a).unwrap().clone();
        changed.notes = "use the staging db".into();
        s.save_space(&changed).unwrap();
        assert!(s.save_space(&Space { id: 99, name: "ghost".into(), ..Default::default() }).is_err());
        assert!(s.save_space(&Space { name: "   ".into(), ..Default::default() }).is_err());
        let again = Spaces::load_from(&dir);
        assert_eq!(again.list().iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Acme", "Gamma"]);
        assert_eq!(again.get(a).unwrap().notes, "use the staging db");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_tab_that_does_not_match_its_arrangement_is_refused() {
        let bad = Space { name: "x".into(), tabs: vec![SpaceTab { name: "t".into(), broadcast: false, shape: split(leaf(0), leaf(1)), panes: vec![SpacePane { server: 1, name: String::new(), command: String::new(), cwd: String::new() }] }], ..Default::default() };
        assert!(bad.cleaned().is_err());
    }

    fn server(id: u64, parent: Option<u64>) -> Server {
        Server { id, name: format!("s{id}"), host: "h".into(), port: 22, user: "u".into(), parent, ..Default::default() }
    }

    #[test]
    fn a_space_covers_its_servers_and_everything_under_its_folders_and_skips_what_was_deleted() {
        let dir = temp("cover");
        let mut store = Store::load_from(dir.join("servers.json")).unwrap();
        store.folders = vec![Folder { id: 10, name: "prod".into(), parent: None, expanded: false, profile: None }, Folder { id: 11, name: "eu".into(), parent: Some(10), expanded: false, profile: None }, Folder { id: 12, name: "other".into(), parent: None, expanded: false, profile: None }];
        store.servers = vec![server(1, None), server(2, Some(10)), server(3, Some(11)), server(4, Some(12)), server(5, None)];
        let sp = Space { name: "x".into(), servers: vec![1, 99], server_folders: vec![10, 77], ..Default::default() };
        assert_eq!(sp.server_ids(&store).into_iter().collect::<Vec<_>>(), [1, 2, 3], "a folder counts at any depth; ids that do not exist are skipped");
        assert!(sp.covers_server(&store, 3) && !sp.covers_server(&store, 4) && !sp.covers_server(&store, 5));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_first_folder_is_where_a_terminal_opens() {
        let sp = Space { name: "x".into(), folders: vec!["/srv/a".into(), "/srv/b".into()], ..Default::default() };
        assert_eq!(sp.first_folder(), Some(PathBuf::from("/srv/a")));
        assert_eq!(Space::default().first_folder(), None);
        let home = expand_home("~/code");
        assert!(home.is_absolute() && home.ends_with("code"));
    }
}
