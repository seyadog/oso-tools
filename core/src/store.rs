//! Data model (folders and servers) and its JSON persistence.
//! Passwords never go through here: they live in the encrypted vault (see `vault`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    #[default]
    Agent,
    Key,
    Password,
}

impl Auth {
    pub fn label(self) -> &'static str {
        match self {
            Auth::Agent => "SSH agent",
            Auth::Key => "Private key",
            Auth::Password => "Password",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Auth::Agent => Auth::Key,
            Auth::Key => Auth::Password,
            Auth::Password => Auth::Agent,
        }
    }

    pub fn prev(self) -> Self {
        self.next().next()
    }
}

/// Which way a port forward goes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ForwardKind {
    /// A port on this computer that leads to `host:port` as the server sees it (`ssh -L`).
    Local,
    /// A port on the server that leads to `host:port` as this computer sees it (`ssh -R`).
    Remote,
    /// A SOCKS proxy on this computer that goes out through the server (`ssh -D`).
    Dynamic,
}

/// A saved port forward of a server. Whether it is switched on is not saved: it is on while oso keeps it open.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Forward {
    pub id: u64,
    pub kind: ForwardKind,
    /// The port that is opened (on this computer for local and dynamic, on the server for remote).
    pub bind_port: u16,
    /// Where it leads (not used by a dynamic one).
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Server {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub parent: Option<u64>,
    /// Server this one is reached through (bastion / jump host).
    #[serde(default)]
    pub jump: Option<u64>,
    /// A login password / key passphrase is saved in the vault (never the secret itself).
    #[serde(default)]
    pub has_secret: bool,
    /// A sudo/su password is saved in the vault.
    #[serde(default)]
    pub has_sudo: bool,
    /// Port forwards (tunnels) kept for this server.
    #[serde(default)]
    pub forwards: Vec<Forward>,
    /// The profile it belongs to (`None`: its folder's, and failing that every profile).
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Folder {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub parent: Option<u64>,
    /// Folders start closed, and stay as the user leaves them.
    #[serde(default)]
    pub expanded: bool,
    /// The profile of what is inside, unless a server says its own.
    #[serde(default)]
    pub profile: Option<String>,
}

/// The profile every installation has.
pub const DEFAULT_PROFILE: &str = "default";

/// A profile groups servers, secrets and accounts ("work", "personal"): switching it switches them together.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    /// Short name used as the key everywhere (`work`); never changes.
    pub id: String,
    /// What the window shows.
    pub name: String,
}

impl Profile {
    pub fn default_one() -> Profile {
        Profile { id: DEFAULT_PROFILE.into(), name: "default".into() }
    }
}

/// `work-laptop` from `Work laptop!`: lowercase letters and digits joined by dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeId {
    Folder(u64),
    Server(u64),
    /// The “bastions” section title of the SSH view. Not part of this store either.
    BastionsHeader,
}

/// Sibling order is the order of the vectors (folders first, then servers).
#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    next_id: u64,
    pub folders: Vec<Folder>,
    pub servers: Vec<Server>,
    /// Saved in `profiles.json`, next to this file; always has at least the default one.
    #[serde(skip)]
    pub profiles: Vec<Profile>,
    #[serde(skip)]
    path: PathBuf,
}

impl Store {
    /// Config directory: `$OSO_CONFIG_DIR` or the system default.
    pub fn config_dir() -> PathBuf {
        if let Some(d) = std::env::var_os("OSO_CONFIG_DIR") {
            return PathBuf::from(d);
        }
        directories::ProjectDirs::from("", "", "oso").map(|d| d.config_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from(".oso"))
    }

    pub fn load() -> Result<Self> {
        Self::load_from(Self::config_dir().join("servers.json"))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn load_from(path: PathBuf) -> Result<Self> {
        let mut store = if path.exists() {
            let text = std::fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))?
        } else {
            Store::default()
        };
        store.profiles = std::fs::read_to_string(path.with_file_name("profiles.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Profile>>(&t).ok())
            .unwrap_or_default();
        if !store.profiles.iter().any(|p| p.id == DEFAULT_PROFILE) {
            store.profiles.insert(0, Profile::default_one());
        }
        store.path = path;
        Ok(store)
    }

    /// Atomic write: temp file + rename.
    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &self.path)?;
        // the file of profiles appears with the first profile that is not the implicit one
        let pfile = self.path.with_file_name("profiles.json");
        if self.profiles != [Profile::default_one()] || pfile.exists() {
            let tmp = pfile.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(&self.profiles)?)?;
            std::fs::rename(&tmp, &pfile)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ profiles

    /// The profile a folder is in: its own, or the nearest one above.
    pub fn folder_profile(&self, folder: Option<u64>) -> Option<&str> {
        let mut cur = folder;
        let mut guard = 0;
        while let Some(id) = cur {
            let f = self.folder(id)?;
            if let Some(p) = f.profile.as_deref() {
                return Some(p);
            }
            cur = f.parent;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        None
    }

    /// The profile of a server: its own, else its folder's (`None`: every profile).
    pub fn server_profile<'a>(&'a self, s: &'a Server) -> Option<&'a str> {
        s.profile.as_deref().or_else(|| self.folder_profile(s.parent))
    }

    /// Does this server show in `profile`? One that belongs to no profile shows in all of them.
    pub fn server_in(&self, s: &Server, profile: &str) -> bool {
        self.server_profile(s).is_none_or(|p| p == profile)
    }

    /// Folders worth showing in `profile`: those that belong to it, empty ones that belong to every profile, and
    /// every folder above one of those or above a server that shows.
    pub fn folders_in(&self, profile: &str) -> Vec<u64> {
        let mut shown: Vec<u64> = self
            .folders
            .iter()
            .filter(|f| {
                let own = self.folder_profile(Some(f.id));
                own == Some(profile) || self.holds_visible(f.id, profile) || (own.is_none() && !self.holds_anything(f.id))
            })
            .map(|f| f.id)
            .collect();
        // a shown folder needs the folders it sits in
        let mut i = 0;
        while i < shown.len() {
            if let Some(p) = self.folder(shown[i]).and_then(|f| f.parent) {
                if !shown.contains(&p) {
                    shown.push(p);
                }
            }
            i += 1;
        }
        shown
    }

    fn holds_anything(&self, folder: u64) -> bool {
        self.servers.iter().any(|s| s.parent.is_some_and(|p| self.is_inside(Some(p), folder))) || self.folders.iter().any(|f| f.id != folder && f.parent.is_some_and(|p| self.is_inside(Some(p), folder)))
    }

    /// Does the folder (or one below it) hold a server that shows in `profile`?
    fn holds_visible(&self, folder: u64, profile: &str) -> bool {
        self.servers.iter().any(|s| s.parent.is_some_and(|p| self.is_inside(Some(p), folder)) && self.server_in(s, profile))
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn server(&self, id: u64) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn folder(&self, id: u64) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// Hops needed to reach `id`, outermost first (excluding `id` itself).
    pub fn jump_chain(&self, id: u64) -> Vec<&Server> {
        let mut chain = vec![];
        let mut cur = self.server(id).and_then(|s| s.jump);
        while let Some(j) = cur {
            if chain.len() > 16 || j == id {
                break;
            }
            let Some(s) = self.server(j) else { break };
            chain.push(s);
            cur = s.jump;
        }
        chain.reverse();
        chain
    }

    /// Would routing `id` through `via` create a loop?
    pub fn would_cycle(&self, id: u64, via: u64) -> bool {
        id == via || self.jump_chain(via).iter().any(|s| s.id == id)
    }

    /// Route `id` through `via` (or directly, with `None`).
    pub fn move_via(&mut self, id: u64, via: Option<u64>) -> bool {
        if via.is_some_and(|v| self.would_cycle(id, v)) {
            return false;
        }
        match self.servers.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.jump = via;
                true
            }
            None => false,
        }
    }

    pub fn add_folder(&mut self, name: String, parent: Option<u64>) -> u64 {
        let id = self.new_id();
        self.folders.push(Folder { id, name, parent, expanded: false, profile: None });
        id
    }

    pub fn add_server(&mut self, mut s: Server) -> u64 {
        s.id = self.new_id();
        let id = s.id;
        self.servers.push(s);
        id
    }

    pub fn update_server(&mut self, s: Server) {
        if let Some(slot) = self.servers.iter_mut().find(|x| x.id == s.id) {
            *slot = s;
        }
    }

    pub fn parent_of(&self, node: NodeId) -> Option<u64> {
        match node {
            NodeId::Folder(id) => self.folder(id).and_then(|f| f.parent),
            NodeId::Server(id) => self.server(id).and_then(|s| s.parent),
            NodeId::BastionsHeader => None,
        }
    }

    /// Is `folder` equal to `ancestor` or inside it?
    pub fn is_inside(&self, folder: Option<u64>, ancestor: u64) -> bool {
        let mut cur = folder;
        while let Some(id) = cur {
            if id == ancestor {
                return true;
            }
            cur = self.folder(id).and_then(|f| f.parent);
        }
        false
    }

    /// Deletes the node (and everything inside, for a folder).
    /// Returns the ids of the removed servers so their secrets can be cleaned up.
    pub fn delete(&mut self, node: NodeId) -> Vec<u64> {
        let gone = self.delete_nodes(node);
        for s in &mut self.servers {
            if s.jump.is_some_and(|j| gone.contains(&j)) {
                s.jump = None;
            }
        }
        gone
    }

    fn delete_nodes(&mut self, node: NodeId) -> Vec<u64> {
        match node {
            NodeId::BastionsHeader => vec![],
            NodeId::Server(id) => {
                self.servers.retain(|s| s.id != id);
                vec![id]
            }
            NodeId::Folder(id) => {
                let doomed: Vec<u64> =
                    self.folders.iter().filter(|f| self.is_inside(Some(f.id), id)).map(|f| f.id).collect();
                let gone: Vec<u64> = self
                    .servers
                    .iter()
                    .filter(|s| s.parent.is_some_and(|p| doomed.contains(&p)))
                    .map(|s| s.id)
                    .collect();
                self.folders.retain(|f| !doomed.contains(&f.id));
                self.servers.retain(|s| !gone.contains(&s.id));
                gone
            }
        }
    }

    /// Moves `node` into `dest` (at the end). Returns false if the move is invalid.
    pub fn move_into(&mut self, node: NodeId, dest: Option<u64>) -> bool {
        if let NodeId::Folder(id) = node {
            if self.is_inside(dest, id) {
                return false;
            }
        }
        match node {
            NodeId::BastionsHeader => return false,
            NodeId::Folder(id) => {
                let Some(pos) = self.folders.iter().position(|f| f.id == id) else { return false };
                let mut f = self.folders.remove(pos);
                f.parent = dest;
                self.folders.push(f);
            }
            NodeId::Server(id) => {
                let Some(pos) = self.servers.iter().position(|s| s.id == id) else { return false };
                let mut s = self.servers.remove(pos);
                s.parent = dest;
                self.servers.push(s);
            }
        }
        if let Some(d) = dest {
            if let Some(f) = self.folders.iter_mut().find(|f| f.id == d) {
                f.expanded = true;
            }
        }
        true
    }

    /// Moves a server right before another one (adopting its folder).
    pub fn move_server_before(&mut self, id: u64, target: u64) -> bool {
        if id == target {
            return false;
        }
        let Some(pos) = self.servers.iter().position(|s| s.id == id) else { return false };
        let mut s = self.servers.remove(pos);
        let Some(tpos) = self.servers.iter().position(|x| x.id == target) else {
            self.servers.insert(pos, s);
            return false;
        };
        s.parent = self.servers[tpos].parent;
        self.servers.insert(tpos, s);
        true
    }

    /// Moves the node up (-1) or down (+1) among siblings of the same kind.
    pub fn shift(&mut self, node: NodeId, delta: i32, by_jump: bool) -> bool {
        fn go<T>(v: &mut [T], i: usize, delta: i32, same: impl Fn(&T, &T) -> bool) -> bool {
            let step = delta.signum();
            let mut j = i as i32 + step;
            while j >= 0 && (j as usize) < v.len() {
                if same(&v[i], &v[j as usize]) {
                    v.swap(i, j as usize);
                    return true;
                }
                j += step;
            }
            false
        }
        match node {
            NodeId::BastionsHeader => false,
            NodeId::Folder(id) => {
                let Some(i) = self.folders.iter().position(|f| f.id == id) else { return false };
                go(&mut self.folders, i, delta, |a, b| a.parent == b.parent)
            }
            NodeId::Server(id) => {
                let Some(i) = self.servers.iter().position(|s| s.id == id) else { return false };
                if by_jump {
                    go(&mut self.servers, i, delta, |a, b| a.jump == b.jump)
                } else {
                    go(&mut self.servers, i, delta, |a, b| a.parent == b.parent)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv(name: &str, parent: Option<u64>) -> Server {
        Server {
            id: 0,
            name: name.into(),
            host: format!("{name}.example"),
            port: 22,
            user: "u".into(),
            auth: Auth::Agent,
            key_path: String::new(),
            parent,
            jump: None,
            has_secret: false,
            has_sudo: false,
            forwards: vec![],
            profile: None,
        }
    }

    #[test]
    fn folder_cannot_move_into_itself_or_descendant() {
        let mut s = Store::default();
        let a = s.add_folder("a".into(), None);
        let b = s.add_folder("b".into(), Some(a));
        assert!(!s.move_into(NodeId::Folder(a), Some(b)));
        assert!(!s.move_into(NodeId::Folder(a), Some(a)));
        assert!(s.move_into(NodeId::Folder(b), None));
    }

    #[test]
    fn delete_folder_removes_contents() {
        let mut s = Store::default();
        let a = s.add_folder("a".into(), None);
        let b = s.add_folder("b".into(), Some(a));
        let x = s.add_server(srv("x", Some(b)));
        let y = s.add_server(srv("y", None));
        let gone = s.delete(NodeId::Folder(a));
        assert_eq!(gone, vec![x]);
        assert!(s.folders.is_empty());
        assert_eq!(s.servers.len(), 1);
        assert_eq!(s.servers[0].id, y);
    }

    #[test]
    fn manual_order_before_and_shift() {
        let mut s = Store::default();
        let a = s.add_server(srv("a", None));
        let b = s.add_server(srv("b", None));
        let c = s.add_server(srv("c", None));
        assert!(s.move_server_before(c, a));
        let order: Vec<u64> = s.servers.iter().map(|x| x.id).collect();
        assert_eq!(order, vec![c, a, b]);
        assert!(s.shift(NodeId::Server(c), 1, false));
        let order: Vec<u64> = s.servers.iter().map(|x| x.id).collect();
        assert_eq!(order, vec![a, c, b]);
    }

    #[test]
    fn jump_chain_cycles_and_delete() {
        let mut s = Store::default();
        let a = s.add_server(srv("a", None));
        let b = s.add_server(srv("b", None));
        let c = s.add_server(srv("c", None));
        assert!(s.move_via(b, Some(a)));
        assert!(s.move_via(c, Some(b)));
        let chain: Vec<u64> = s.jump_chain(c).iter().map(|x| x.id).collect();
        assert_eq!(chain, vec![a, b]);
        assert!(!s.move_via(a, Some(c)), "a -> c -> b -> a is a loop");
        assert!(!s.move_via(a, Some(a)));
        s.delete(NodeId::Server(b));
        assert_eq!(s.server(c).unwrap().jump, None);
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = std::env::temp_dir().join(format!("oso-test-{}", std::process::id()));
        let path = dir.join("servers.json");
        let mut s = Store::load_from(path.clone()).unwrap();
        s.add_server(srv("a", None));
        s.save().unwrap();
        let s2 = Store::load_from(path).unwrap();
        assert_eq!(s2.servers.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    fn named(store: &mut Store, name: &str, parent: Option<u64>, profile: Option<&str>) -> u64 {
        let mut s = srv(name, parent);
        s.profile = profile.map(str::to_string);
        store.add_server(s)
    }

    #[test]
    fn a_server_takes_its_folders_profile_unless_it_has_its_own() {
        let mut st = Store::default();
        let work = st.add_folder("work".into(), None);
        let deep = st.add_folder("deep".into(), Some(work));
        st.folders.iter_mut().find(|f| f.id == work).unwrap().profile = Some("work".into());
        let a = named(&mut st, "a", Some(deep), None);
        let b = named(&mut st, "b", Some(deep), Some("home"));
        let c = named(&mut st, "c", None, None);
        assert_eq!(st.server_profile(st.server(a).unwrap()), Some("work"), "inherited through two folders");
        assert_eq!(st.server_profile(st.server(b).unwrap()), Some("home"), "its own wins");
        assert_eq!(st.server_profile(st.server(c).unwrap()), None);
        assert!(st.server_in(st.server(a).unwrap(), "work") && !st.server_in(st.server(a).unwrap(), "home"));
        assert!(st.server_in(st.server(b).unwrap(), "home") && !st.server_in(st.server(b).unwrap(), "work"));
        assert!(st.server_in(st.server(c).unwrap(), "work") && st.server_in(st.server(c).unwrap(), "home"), "no profile: shown in all");
    }

    #[test]
    fn folders_show_when_they_belong_or_hold_something_that_does() {
        let mut st = Store::default();
        let shared = st.add_folder("shared".into(), None);
        let work = st.add_folder("work".into(), None);
        let clients = st.add_folder("clients".into(), Some(shared));
        let empty = st.add_folder("empty".into(), None);
        st.folders.iter_mut().find(|f| f.id == work).unwrap().profile = Some("work".into());
        named(&mut st, "w", Some(clients), Some("work"));
        named(&mut st, "h", Some(work), None);
        let home: Vec<u64> = st.folders_in("home");
        assert!(!home.contains(&work) && !home.contains(&clients) && !home.contains(&shared), "nothing for home inside them: {home:?}");
        assert!(home.contains(&empty), "an empty folder of no profile shows everywhere");
        let w = st.folders_in("work");
        assert!(w.contains(&work) && w.contains(&clients) && w.contains(&shared), "the folder above one that shows also shows: {w:?}");
    }

    #[test]
    fn profiles_live_in_their_own_file_and_default_always_exists() {
        let dir = std::env::temp_dir().join(format!("oso-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("servers.json");
        let mut s = Store::load_from(path.clone()).unwrap();
        assert_eq!(s.profiles, vec![Profile::default_one()]);
        s.save().unwrap();
        assert!(!dir.join("profiles.json").exists(), "nothing is written for the implicit profile");
        s.profiles.push(Profile { id: "work".into(), name: "Work".into() });
        s.save().unwrap();
        let again = Store::load_from(path).unwrap();
        assert_eq!(again.profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["default", "work"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn profile_ids_are_slugs() {
        assert_eq!(slug("Work laptop!"), "work-laptop");
        assert_eq!(slug("  Casa  "), "casa");
        assert_eq!(slug("!!!"), "");
    }
}
