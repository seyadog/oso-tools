//! Spaces: one unit of work. A Space groups the folders (repos), servers, secrets and tunnels a piece of work uses, some
//! notes, and the layout of its tabs. It is the replacement of "groups" (saved tabs): `groups.json` is migrated into
//! Spaces the first time (see `migrate_groups`).
//!
//! A Space is also what limits an AI agent: the terminals opened in a Space are bound to it, and the MCP of an agent
//! running there sees only what the Space holds (see `agent_api`). Ids in a Space may point at things that were deleted
//! later; they are skipped when used, never an error.

use crate::store::{Folder, Store};
use std::collections::HashMap;
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

/// Something that can be put in a Space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Server(u64),
    Folder(u64),
    Secret(u64),
}

impl Item {
    fn id(self) -> u64 {
        match self {
            Item::Server(i) | Item::Folder(i) | Item::Secret(i) => i,
        }
    }
}

/// What a Space (or General) may fix for the machine while it is in use. Every field is optional: what is not set is
/// left as it is.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SpaceSettings {
    /// The Tailscale account (id, tailnet or account name) to be on. oso asks before switching to it.
    #[serde(default)]
    pub tailscale_account: Option<String>,
    /// The GitHub account (`host/login`) `gh` should use. oso asks before switching to it.
    #[serde(default)]
    pub gh_user: Option<String>,
    /// The proxy (a saved one) that the local terminals go out through (`HTTP_PROXY` and friends).
    #[serde(default)]
    pub proxy: Option<u64>,
    /// `AWS_PROFILE` for its local terminals: a name from `~/.aws/config`, never a key.
    #[serde(default)]
    pub aws_profile: Option<String>,
    /// `KUBECONFIG` for them (a path).
    #[serde(default)]
    pub kubeconfig: Option<String>,
    /// Real machines are on the other end: the window shows it in red.
    #[serde(default)]
    pub production: bool,
}

impl SpaceSettings {
    /// Trimmed, with what is empty taken out, and refused when a value would end up in an environment variable or a path
    /// it must not.
    pub fn cleaned(&self) -> Result<SpaceSettings> {
        let mut s = self.clone();
        let clean = |v: &mut Option<String>| *v = v.take().map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
        clean(&mut s.tailscale_account);
        clean(&mut s.gh_user);
        clean(&mut s.aws_profile);
        clean(&mut s.kubeconfig);
        if s.aws_profile.as_deref().is_some_and(|a| a.starts_with('-') || a.chars().any(|c| c.is_whitespace() || c.is_control())) {
            bail!("an AWS account name has no spaces");
        }
        if s.kubeconfig.as_deref().is_some_and(|k| k.contains('\0') || k.contains('\n')) {
            bail!("a kubeconfig is a path, on one line");
        }
        Ok(s)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Space {
    /// 0 for one that is not saved yet.
    #[serde(default)]
    pub id: u64,
    pub name: String,
    /// What it fixes for the machine while you work in it (accounts, a proxy, cloud settings): all optional.
    #[serde(flatten)]
    pub settings: SpaceSettings,
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

/// Which Spaces each server and folder are in; one in none is in General.
#[derive(Default, Debug)]
pub struct Membership {
    servers: HashMap<u64, Vec<u64>>,
    folders: HashMap<u64, Vec<u64>>,
}

impl Membership {
    pub fn spaces_of_server(&self, id: u64) -> &[u64] {
        self.servers.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Is it shown when `active` (None: General) is the Space in use? A server in no Space (General) shows in all of them.
    pub fn server_shows(&self, id: u64, active: Option<u64>) -> bool {
        let m = self.spaces_of_server(id);
        m.is_empty() || active.is_some_and(|a| m.contains(&a))
    }

    /// The folders worth showing: those in the active Space, those that hold a server that shows, empty ones that are in
    /// General, and every folder above one of those.
    pub fn folders_shown(&self, store: &Store, active: Option<u64>) -> Vec<u64> {
        let inside = |p: Option<u64>, root: u64| {
            let mut cur = p;
            let mut guard = 0;
            while let Some(id) = cur {
                if id == root {
                    return true;
                }
                cur = store.folder(id).and_then(|f| f.parent);
                guard += 1;
                if guard > 64 {
                    break;
                }
            }
            false
        };
        let holds_anything = |f: u64| store.servers.iter().any(|s| s.parent.is_some_and(|p| inside(Some(p), f))) || store.folders.iter().any(|g| g.id != f && g.parent.is_some_and(|p| inside(Some(p), f)));
        let holds_visible = |f: u64| store.servers.iter().any(|s| s.parent.is_some_and(|p| inside(Some(p), f)) && self.server_shows(s.id, active));
        let mut shown: Vec<u64> = store
            .folders
            .iter()
            .filter(|f| {
                let own = self.folders.get(&f.id).map(Vec::as_slice).unwrap_or(&[]);
                active.is_some_and(|a| own.contains(&a)) || holds_visible(f.id) || (own.is_empty() && !holds_anything(f.id))
            })
            .map(|f| f.id)
            .collect();
        let mut i = 0;
        while i < shown.len() {
            if let Some(p) = store.folder(shown[i]).and_then(|f| f.parent) {
                if !shown.contains(&p) {
                    shown.push(p);
                }
            }
            i += 1;
        }
        shown
    }
}

/// A profile as the old `profiles.json` had it (read only, to migrate it).
#[derive(Deserialize, Default)]
struct LegacyProfile {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    proxy: Option<u64>,
    #[serde(default)]
    gh_user: Option<String>,
    #[serde(default)]
    tailscale_account: Option<String>,
    #[serde(default)]
    aws_profile: Option<String>,
    #[serde(default)]
    kubeconfig: Option<String>,
    #[serde(default)]
    production: bool,
}

impl LegacyProfile {
    fn settings(&self) -> SpaceSettings {
        SpaceSettings { tailscale_account: self.tailscale_account.clone(), gh_user: self.gh_user.clone(), proxy: self.proxy, aws_profile: self.aws_profile.clone(), kubeconfig: self.kubeconfig.clone(), production: self.production }
    }
}

/// What turning the profiles into Spaces did, for the window to say once.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct ProfileMigration {
    /// The Spaces that were made (one per old profile that was not the default one).
    pub spaces: Vec<String>,
    /// The Space that was in use, when it was one of them.
    pub active: Option<String>,
    /// Where the copy of the old files is.
    pub backup: String,
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
        s.settings = s.settings.cleaned()?;
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
    /// What General fixes (the same things a Space may fix).
    #[serde(default)]
    general: SpaceSettings,
    /// The old profiles have become Spaces (done once).
    #[serde(default)]
    profiles_migrated: bool,
    /// While secrets still have to be put in the Space of their old profile (they are in the vault, which has to be unlocked):
    /// old profile id → the Space it became.
    #[serde(default)]
    pending_secrets: std::collections::BTreeMap<String, u64>,
}

/// The Spaces on disk (`oso-spaces.json`).
pub struct Spaces {
    path: PathBuf,
    next_id: u64,
    spaces: Vec<Space>,
    /// Set when `groups.json` was migrated at load, until the window has told the person.
    migration: Option<Migration>,
    /// What General fixes.
    pub general: SpaceSettings,
    profiles_migrated: bool,
    pending_secrets: std::collections::BTreeMap<String, u64>,
    /// Set when the old profiles became Spaces at load, until the window has told the person.
    profile_migration: Option<ProfileMigration>,
}

impl Spaces {
    /// Reads the Spaces of a config folder. If there is no `oso-spaces.json` but there is a `groups.json`, the groups
    /// become Spaces and `groups.json` is renamed to `groups.json.migrated` (never deleted). Both present: the groups are
    /// left alone, they are never imported twice.
    pub fn load_from(dir: &Path) -> Spaces {
        let path = dir.join(FILE);
        let mut me = Spaces { path: path.clone(), next_id: 1, spaces: vec![], migration: None, general: SpaceSettings::default(), profiles_migrated: false, pending_secrets: Default::default(), profile_migration: None };
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(file) = serde_json::from_str::<File>(&text) {
                me.spaces = file.spaces;
                me.next_id = file.next_id.max(me.spaces.iter().map(|s| s.id).max().unwrap_or(0) + 1).max(1);
                me.general = file.general;
                me.profiles_migrated = file.profiles_migrated;
                me.pending_secrets = file.pending_secrets;
            }
        } else {
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
        }
        me.migrate_profiles(dir);
        me
    }

    /// The old profiles become Spaces (once). The default one is General: its things stay where they are and its accounts,
    /// proxy and cloud settings are General's; every other one becomes a Space with what was in it. A copy of every file
    /// that is read goes to `backup-before-spaces-<time>/` first. Secrets are moved when the vault is unlocked (they are
    /// in it): see `take_pending_secrets`.
    fn migrate_profiles(&mut self, dir: &Path) {
        if self.profiles_migrated {
            return;
        }
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        let legacy: Vec<LegacyProfile> = read("profiles.json").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let servers: Value = read("servers.json").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let tagged = |key: &str| servers.get(key).and_then(Value::as_array).map(|a| a.iter().any(|x| x.get("profile").and_then(Value::as_str).is_some_and(|p| !p.is_empty()))).unwrap_or(false);
        let settings: Value = read("settings.json").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let has_legacy = !legacy.is_empty() || tagged("servers") || tagged("folders") || settings.get("active_profile").is_some_and(|v| v.is_string());
        if !has_legacy {
            return;
        }
        // a copy first
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let backup = dir.join(format!("backup-before-spaces-{stamp}"));
        let copied = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&backup)?;
            for f in ["servers.json", "profiles.json", "proxies.json", "settings.json", "vault.json"] {
                if dir.join(f).exists() {
                    std::fs::copy(dir.join(f), backup.join(f))?;
                }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        })();
        if copied.is_err() {
            return; // without a copy nothing is changed: it is tried again next time
        }
        let mut report = ProfileMigration { backup: backup.to_string_lossy().into_owned(), ..Default::default() };
        if let Some(d) = legacy.iter().find(|p| p.id == "default") {
            self.general = d.settings();
        }
        let ids = |key: &str, profile: &str| -> Vec<u64> {
            servers.get(key).and_then(Value::as_array).map(|a| a.iter().filter(|x| x.get("profile").and_then(Value::as_str) == Some(profile)).filter_map(|x| x.get("id").and_then(Value::as_u64)).collect()).unwrap_or_default()
        };
        for p in legacy.iter().filter(|p| p.id != "default") {
            let mut name: String = p.name.trim().chars().take(80).collect();
            if name.is_empty() {
                name = p.id.clone();
            }
            let base = name.clone();
            let mut n = 2;
            while self.spaces.iter().any(|s| s.name == name) {
                name = format!("{base} ({n})");
                n += 1;
            }
            let id = self.next_id;
            self.next_id += 1;
            self.spaces.push(Space { id, name: name.clone(), settings: p.settings(), servers: ids("servers", &p.id), server_folders: ids("folders", &p.id), ..Default::default() });
            self.pending_secrets.insert(p.id.clone(), id);
            report.spaces.push(name);
        }
        // the vault is looked at when it is unlocked, even if there is nothing to move: the old profile marks go away then
        self.profiles_migrated = true;
        if self.save().is_err() {
            return;
        }
        // settings: the Space in use, and whether every Space's servers are shown
        let dir_s = dir;
        if let Some(active) = settings.get("active_profile").and_then(Value::as_str) {
            if let Some(space) = self.pending_secrets.get(active) {
                crate::settings::save_key_in(dir_s, "active_space", (*space).into());
                report.active = self.get(*space).map(|s| s.name.clone());
            }
        }
        if let Some(all) = settings.get("view_all_profiles").and_then(Value::as_bool) {
            crate::settings::save_key_in(dir_s, "view_all", all.into());
        }
        for old in ["active_profile", "view_all_profiles"] {
            crate::settings::save_key_in(dir_s, old, Value::Null);
        }
        let _ = std::fs::rename(dir.join("profiles.json"), dir.join("profiles.json.migrated"));
        self.profile_migration = Some(report);
    }

    /// The report of the migration of the profiles, once.
    pub fn take_profile_migration(&mut self) -> Option<ProfileMigration> {
        self.profile_migration.take()
    }

    /// The old profiles whose secrets have not been put in their Space yet: old profile id → Space id.
    pub fn pending_secrets(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.pending_secrets
    }

    /// The secrets have been moved (or there was nothing to move): the old marks are not needed any more.
    pub fn secrets_done(&mut self) -> Result<()> {
        self.pending_secrets.clear();
        self.save()
    }

    /// Puts a secret in a Space (without saving: call `save_all` after a batch).
    pub fn add_secret_to(&mut self, space: u64, secret: u64) {
        if let Some(s) = self.spaces.iter_mut().find(|s| s.id == space) {
            if !s.secrets.contains(&secret) {
                s.secrets.push(secret);
            }
        }
    }

    pub fn save_all(&self) -> Result<()> {
        self.save()
    }

    /// Changes what General fixes.
    pub fn set_general(&mut self, g: SpaceSettings) -> Result<()> {
        self.general = g;
        self.save()
    }

    /// The settings of a Space, or General's.
    pub fn settings_of(&self, space: Option<u64>) -> SpaceSettings {
        space.and_then(|id| self.get(id)).map(|s| s.settings.clone()).unwrap_or_else(|| self.general.clone())
    }

    /// For every server and folder, the Spaces it is in (a folder counts for everything below it). Empty: General.
    pub fn membership(&self, store: &Store) -> Membership {
        let mut m = Membership::default();
        for sp in &self.spaces {
            for id in sp.server_ids(store) {
                m.servers.entry(id).or_default().push(sp.id);
            }
            for root in &sp.server_folders {
                if store.folders.iter().any(|f| f.id == *root) {
                    for f in folder_subtree(&store.folders, *root) {
                        m.folders.entry(f).or_default().push(sp.id);
                    }
                }
            }
        }
        m
    }

    /// For every secret that is in a Space, which ones.
    pub fn secret_membership(&self) -> HashMap<u64, Vec<u64>> {
        let mut out: HashMap<u64, Vec<u64>> = HashMap::new();
        for sp in &self.spaces {
            for id in &sp.secrets {
                out.entry(*id).or_default().push(sp.id);
            }
        }
        out
    }

    /// The secrets that are in `space` or in General, out of `all` (General: the ones in no Space; `Some(space)`: its own as
    /// well). This is what a terminal of that Space, and an agent in it, may use.
    pub fn secrets_in_scope(&self, space: Option<u64>, all: impl IntoIterator<Item = u64>) -> Vec<u64> {
        let member = self.secret_membership();
        all.into_iter().filter(|id| member.get(id).is_none_or(Vec::is_empty) || space.is_some_and(|sp| member.get(id).is_some_and(|v| v.contains(&sp)))).collect()
    }

    /// The servers that are in `space` or in General (General: the ones in no Space).
    pub fn servers_in_scope(&self, store: &Store, space: Option<u64>) -> Vec<u64> {
        let m = self.membership(store);
        store.servers.iter().map(|s| s.id).filter(|id| m.server_shows(*id, space)).collect()
    }

    /// Puts something in one Space only, or (`None`) in none, which is General. A server in a folder that is in a Space is
    /// still in that Space too: only what the Space lists itself is moved.
    pub fn move_item(&mut self, item: Item, to: Option<u64>) -> Result<()> {
        if to.is_some_and(|id| self.get(id).is_none()) {
            bail!("this Space no longer exists");
        }
        for sp in &mut self.spaces {
            let list = match item {
                Item::Server(_) => &mut sp.servers,
                Item::Folder(_) => &mut sp.server_folders,
                Item::Secret(_) => &mut sp.secrets,
            };
            list.retain(|x| *x != item.id());
            if Some(sp.id) == to {
                list.push(item.id());
            }
        }
        self.save()
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
        std::fs::write(&tmp, serde_json::to_string_pretty(&File { next_id: self.next_id, spaces: self.spaces.clone(), general: self.general.clone(), profiles_migrated: self.profiles_migrated, pending_secrets: self.pending_secrets.clone() })?)?;
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
        assert!(ops.folders.is_empty() && ops.secrets.is_empty() && ops.tunnels.is_empty() && ops.notes.is_empty() && ops.settings == SpaceSettings::default());
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
        let a = s.save_space(&Space { name: "  Example  ".into(), folders: vec!["/srv/a".into(), "/srv/a".into(), " ".into()], servers: vec![3, 3], ..Default::default() }).unwrap();
        let b = s.save_space(&Space { name: "Beta".into(), ..Default::default() }).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(s.get(a).unwrap().name, "Example");
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
        assert_eq!(again.list().iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Example", "Gamma"]);
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
        store.folders = vec![Folder { id: 10, name: "prod".into(), parent: None, expanded: false }, Folder { id: 11, name: "eu".into(), parent: Some(10), expanded: false }, Folder { id: 12, name: "other".into(), parent: None, expanded: false }];
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

    // ------------------------------------------------------------ profiles became Spaces

    fn old_files(dir: &Path) {
        std::fs::write(
            dir.join("servers.json"),
            json!({
                "next_id": 20,
                "folders": [ { "id": 10, "name": "clients", "parent": null, "expanded": true, "profile": "work" }, { "id": 11, "name": "mixed", "parent": null, "expanded": false } ],
                "servers": [
                    { "id": 1, "name": "shop-api", "host": "h1", "port": 22, "user": "u", "auth": "agent", "parent": 10, "profile": null },
                    { "id": 2, "name": "nas", "host": "h2", "port": 22, "user": "u", "auth": "agent", "parent": null, "profile": "home" },
                    { "id": 3, "name": "everywhere", "host": "h3", "port": 22, "user": "u", "auth": "agent", "parent": null },
                    { "id": 4, "name": "old-default", "host": "h4", "port": 22, "user": "u", "auth": "agent", "parent": 11, "profile": "default" }
                ]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join("profiles.json"),
            json!([
                { "id": "default", "name": "default", "proxy": 7, "gh_user": "me", "aws_profile": "base" },
                { "id": "work", "name": "Work", "tailscale_account": "corp.example.com", "gh_user": "work-user", "production": true },
                { "id": "home", "name": "Home", "kubeconfig": "~/.kube/home.yaml" }
            ])
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("proxies.json"), "[]").unwrap();
        std::fs::write(dir.join("settings.json"), json!({ "theme": "terminal", "active_profile": "work", "view_all_profiles": true }).to_string()).unwrap();
        std::fs::write(dir.join("vault.json"), "SEALED").unwrap();
    }

    #[test]
    fn the_profiles_become_general_and_spaces_with_a_copy_of_the_old_files() {
        let dir = temp("profiles");
        old_files(&dir);
        let mut s = Spaces::load_from(&dir);
        // the default profile is General: its settings are General's, its servers are in no Space
        assert_eq!((s.general.proxy, s.general.gh_user.as_deref(), s.general.aws_profile.as_deref()), (Some(7), Some("me"), Some("base")));
        // every other profile is a Space with what was in it
        let names: Vec<&str> = s.list().iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Work", "Home"]);
        let work = s.list()[0].clone();
        assert_eq!((work.server_folders.clone(), work.servers.clone()), (vec![10], vec![]), "a folder of the profile: everything inside it");
        assert_eq!((work.settings.tailscale_account.as_deref(), work.settings.gh_user.as_deref(), work.settings.production), (Some("corp.example.com"), Some("work-user"), true));
        let home = s.list()[1].clone();
        assert_eq!((home.servers.clone(), home.settings.kubeconfig.as_deref()), (vec![2], Some("~/.kube/home.yaml")));
        // the report, once
        let r = s.take_profile_migration().expect("told once");
        assert_eq!((r.spaces.clone(), r.active.as_deref()), (vec!["Work".to_string(), "Home".to_string()], Some("Work")));
        assert!(s.take_profile_migration().is_none());
        // a copy of what was read, before anything was changed
        let backup = PathBuf::from(&r.backup);
        for f in ["servers.json", "profiles.json", "proxies.json", "settings.json", "vault.json"] {
            assert!(backup.join(f).exists(), "{f}");
        }
        assert_eq!(std::fs::read_to_string(backup.join("vault.json")).unwrap(), "SEALED");
        assert!(std::fs::read_to_string(backup.join("profiles.json")).unwrap().contains("corp.example.com"), "as it was");
        // the space in use and the view are in settings, the old keys are gone
        let st: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(st["active_space"], work.id);
        assert_eq!(st["view_all"], true);
        assert!(st["active_profile"].is_null() && st["view_all_profiles"].is_null() && st["theme"] == "terminal", "the rest is kept: {st}");
        assert!(dir.join("profiles.json.migrated").exists() && !dir.join("profiles.json").exists());
        // the secrets of each old profile wait for the vault to be unlocked
        assert_eq!(s.pending_secrets().get("work"), Some(&work.id));
        assert!(s.pending_secrets().get("default").is_none(), "General's stay where they are");
        // starting again does not migrate again
        let mut again = Spaces::load_from(&dir);
        assert_eq!(again.list().len(), 2);
        assert!(again.take_profile_migration().is_none());
        assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("backup-before-spaces")).count(), 1, "one copy, not two");
        // the servers: who is where
        let mut store = Store::load_from(dir.join("servers.json")).unwrap();
        store.proxies.clear();
        let m = again.membership(&store);
        assert_eq!(m.spaces_of_server(1), &[again.list()[0].id], "shop-api is in a folder of Work");
        assert_eq!(m.spaces_of_server(2), &[again.list()[1].id], "nas is Home's");
        assert!(m.spaces_of_server(3).is_empty() && m.spaces_of_server(4).is_empty(), "the rest is General");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_single_default_profile_becomes_general_and_makes_no_space_and_nothing_else_is_touched() {
        let dir = temp("one-profile");
        std::fs::write(dir.join("profiles.json"), json!([{ "id": "default", "name": "default", "tailscale_account": "me.ts.net" }]).to_string()).unwrap();
        std::fs::write(dir.join("servers.json"), json!({ "next_id": 2, "folders": [], "servers": [ { "id": 1, "name": "a", "host": "h", "port": 22, "user": "u", "auth": "agent", "profile": "default" } ] }).to_string()).unwrap();
        let mut s = Spaces::load_from(&dir);
        assert!(s.list().is_empty(), "no Space is made from the only profile");
        assert_eq!(s.general.tailscale_account.as_deref(), Some("me.ts.net"));
        assert!(s.take_profile_migration().unwrap().spaces.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn nothing_that_comes_from_profiles_means_nothing_is_done_and_no_file_is_made() {
        let dir = temp("no-profiles");
        let mut s = Spaces::load_from(&dir);
        assert!(s.take_profile_migration().is_none() && !dir.join(FILE).exists());
        std::fs::write(dir.join("servers.json"), json!({ "next_id": 1, "folders": [], "servers": [] }).to_string()).unwrap();
        assert!(Spaces::load_from(&dir).list().is_empty() && !dir.join(FILE).exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn two_profiles_that_would_have_the_same_name_get_different_ones() {
        let dir = temp("same-name");
        std::fs::write(dir.join("profiles.json"), json!([{ "id": "default", "name": "default" }, { "id": "a", "name": "Work" }, { "id": "b", "name": "Work" }]).to_string()).unwrap();
        let s = Spaces::load_from(&dir);
        assert_eq!(s.list().iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Work", "Work (2)"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn what_is_shown_depends_on_the_space_in_use_and_general_is_everywhere() {
        let dir = temp("membership");
        let mut store = Store::load_from(dir.join("servers.json")).unwrap();
        store.folders = vec![Folder { id: 10, name: "prod".into(), parent: None, expanded: false }, Folder { id: 11, name: "eu".into(), parent: Some(10), expanded: false }, Folder { id: 12, name: "empty".into(), parent: None, expanded: false }];
        store.servers = vec![server(1, Some(11)), server(2, None), server(3, None)];
        let mut s = Spaces::load_from(&dir);
        let a = s.save_space(&Space { name: "A".into(), server_folders: vec![10], ..Default::default() }).unwrap();
        let b = s.save_space(&Space { name: "B".into(), servers: vec![2], ..Default::default() }).unwrap();
        let m = s.membership(&store);
        assert!(m.server_shows(1, Some(a)) && !m.server_shows(1, Some(b)) && !m.server_shows(1, None), "in A only");
        assert!(m.server_shows(3, None) && m.server_shows(3, Some(a)) && m.server_shows(3, Some(b)), "General is everywhere");
        assert_eq!(s.servers_in_scope(&store, None), vec![3]);
        assert_eq!(s.servers_in_scope(&store, Some(b)), vec![2, 3]);
        let mut shown = m.folders_shown(&store, Some(a));
        shown.sort();
        assert_eq!(shown, vec![10, 11, 12], "A's folders, and an empty one that is General's");
        assert_eq!(m.folders_shown(&store, Some(b)), vec![12]);
        // secrets
        s.add_secret_to(a, 100);
        s.save_all().unwrap();
        assert_eq!(s.secrets_in_scope(None, [100, 101]), vec![101]);
        assert_eq!(s.secrets_in_scope(Some(a), [100, 101]), vec![100, 101]);
        assert_eq!(s.secrets_in_scope(Some(b), [100, 101]), vec![101]);
        std::fs::remove_dir_all(dir).ok();
    }
}
