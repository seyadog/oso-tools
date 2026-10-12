//! Encrypted secret vault.
//!
//! One file (`vault.json`) holds every saved secret: passwords, sudo passwords, SSH keys, API keys, tokens and
//! one-time-password seeds, each with a name and a type. The key is derived from the master password with
//! Argon2id and the data is sealed with XChaCha20-Poly1305. Names and notes are inside the sealed part too.
//! The master password and the derived key live only in memory, and only while the vault is unlocked.
//!
//! Format versions: 1 kept one login and one sudo password per server; 2 (this) keeps named, typed secrets and
//! links servers to them. A version 1 file is migrated when it is opened, after a copy `vault.json.v1.bak`.

use anyhow::{Context, Result, anyhow, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

/// Authenticated but not encrypted: binds the ciphertext to this file format.
// Part of the encryption: changing this value would make existing vaults impossible to open.
const AAD: &[u8] = b"ship-vault-v1";

/// Which of a server's two classic secrets (what the connection code asks for).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Login password, or the passphrase of a private key.
    Login,
    /// Password for `sudo` / `su` on the remote machine.
    Sudo,
    /// The seed of the server's one-time codes (the link is made with `set_totp_link`).
    Totp,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Password,
    Sudo,
    SshKey,
    ApiKey,
    Token,
    Totp,
}

/// A secret as it is sealed in the file. Only this type carries the value.
#[derive(Serialize, Deserialize, Clone)]
pub struct Secret {
    pub id: u64,
    pub name: String,
    pub kind: SecretKind,
    /// The only really secret part (for `SshKey`: the OpenSSH private key).
    pub value: String,
    /// What older versions called the profile of a secret. It is moved into a Space the first time the vault is unlocked
    /// after the change, and then dropped (see `legacy_marks` and `clear_legacy_marks`).
    #[serde(default, rename = "profile", skip_serializing_if = "Option::is_none")]
    pub legacy_profile: Option<String>,
    /// Put in the environment of the terminals where it is available (its Space's, or all of them in General), e.g. `ANTHROPIC_API_KEY`.
    #[serde(default)]
    pub env_var: Option<String>,
    /// May be put in terminals where an AI agent can run.
    #[serde(default)]
    pub agent_visible: bool,
    #[serde(default)]
    pub notes: String,
    /// Unix seconds of the last change, to warn about old secrets.
    #[serde(default)]
    pub updated: u64,
}

/// Everything about a secret except its value: this is what lists, events and tool answers may carry.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SecretMeta {
    /// 0 for one that is not saved yet.
    pub id: u64,
    pub name: String,
    pub kind: SecretKind,
    #[serde(default)]
    pub env_var: Option<String>,
    #[serde(default)]
    pub agent_visible: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub updated: u64,
}

impl Secret {
    fn meta(&self) -> SecretMeta {
        SecretMeta {
            id: self.id,
            name: self.name.clone(),
            kind: self.kind,
            env_var: self.env_var.clone(),
            agent_visible: self.agent_visible,
            notes: self.notes.clone(),
            updated: self.updated,
        }
    }
}

/// The secrets a server uses, by id.
#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HostSecrets {
    #[serde(default)]
    pub login: Option<u64>,
    #[serde(default)]
    pub sudo: Option<u64>,
    /// A private key kept in the vault (`Auth::VaultKey`).
    #[serde(default)]
    pub key: Option<u64>,
    /// The seed of the one-time codes the server asks for after the password (`TOTP`).
    #[serde(default)]
    pub totp: Option<u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    secrets: BTreeMap<u64, Secret>,
    /// Keyed by server id; 0 is this computer (its sudo password).
    #[serde(default)]
    host_links: BTreeMap<u64, HostSecrets>,
}

/// What version 1 kept inside the sealed part.
#[derive(Default, Deserialize)]
struct EntryV1 {
    login: Option<String>,
    sudo: Option<String>,
}

#[derive(Default, Deserialize)]
struct DataV1 {
    entries: BTreeMap<u64, EntryV1>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Names the migration gives (the engine replaces them with the server's name once it knows it).
fn auto_name(server: u64, kind: Kind) -> String {
    format!("server {server} {}", if kind == Kind::Login { "login" } else { "sudo" })
}

impl Data {
    fn from_v1(v1: DataV1) -> Data {
        let mut d = Data::default();
        for (server, e) in v1.entries {
            let mut links = HostSecrets::default();
            for (kind, value) in [(Kind::Login, e.login), (Kind::Sudo, e.sudo)] {
                let Some(value) = value else { continue };
                let id = d.new_id();
                let sk = if kind == Kind::Login { SecretKind::Password } else { SecretKind::Sudo };
                d.secrets.insert(id, Secret { id, name: auto_name(server, kind), kind: sk, value, legacy_profile: None, env_var: None, agent_visible: false, notes: String::new(), updated: now() });
                if kind == Kind::Login { links.login = Some(id) } else { links.sudo = Some(id) }
            }
            d.host_links.insert(server, links);
        }
        d
    }

    fn new_id(&mut self) -> u64 {
        let max = self.secrets.keys().next_back().copied().unwrap_or(0);
        self.next_id = self.next_id.max(max) + 1;
        self.next_id
    }

    fn referenced(&self, secret: u64, except_server: Option<u64>) -> bool {
        self.host_links.iter().any(|(s, l)| Some(*s) != except_server && (l.login == Some(secret) || l.sudo == Some(secret) || l.key == Some(secret) || l.totp == Some(secret)))
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Kdf {
    m_kib: u32,
    t: u32,
    p: u32,
}

impl Kdf {
    const DEFAULT: Kdf = Kdf { m_kib: 64 * 1024, t: 3, p: 1 };
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    kdf: Kdf,
    salt: String,
    nonce: String,
    ciphertext: String,
}

struct Unlocked {
    key: Zeroizing<[u8; 32]>,
    kdf: Kdf,
    salt: [u8; 16],
    data: Data,
    last_used: Instant,
}

impl Drop for Unlocked {
    fn drop(&mut self) {
        for s in self.data.secrets.values_mut() {
            s.value.zeroize();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum UnlockError {
    WrongPassword,
    Other(String),
}

pub struct Vault {
    path: PathBuf,
    state: Option<Unlocked>,
}

fn derive(master: &str, salt: &[u8], kdf: Kdf) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|e| anyhow!("bad KDF parameters: {e}"))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(master.as_bytes(), salt, key.as_mut_slice())
        .map_err(|e| anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| anyhow!("no secure randomness available: {e}"))?;
    Ok(b)
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Vault { path, state: None }
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn is_unlocked(&self) -> bool {
        self.state.is_some()
    }

    /// Creates an empty vault protected by `master` and leaves it unlocked.
    pub fn create(&mut self, master: &str) -> Result<()> {
        self.create_with(master, Kdf::DEFAULT)
    }

    fn create_with(&mut self, master: &str, kdf: Kdf) -> Result<()> {
        if self.exists() {
            bail!("a vault already exists");
        }
        let salt = random::<16>()?;
        let key = derive(master, &salt, kdf)?;
        self.state = Some(Unlocked { key, kdf, salt, data: Data::default(), last_used: Instant::now() });
        self.save()
    }

    pub fn unlock(&mut self, master: &str) -> Result<(), UnlockError> {
        let other = |e: anyhow::Error| UnlockError::Other(format!("{e:#}"));
        let text = std::fs::read_to_string(&self.path).map_err(|e| UnlockError::Other(e.to_string()))?;
        let file: File = serde_json::from_str(&text).map_err(|e| UnlockError::Other(format!("corrupt vault: {e}")))?;
        if file.version != 1 && file.version != 2 {
            return Err(UnlockError::Other(format!("unsupported vault version {}", file.version)));
        }
        let decode = |s: &str| B64.decode(s).map_err(|e| UnlockError::Other(format!("corrupt vault: {e}")));
        let salt: [u8; 16] = decode(&file.salt)?.try_into().map_err(|_| UnlockError::Other("corrupt vault".into()))?;
        let nonce: [u8; 24] =
            decode(&file.nonce)?.try_into().map_err(|_| UnlockError::Other("corrupt vault".into()))?;
        let ciphertext = decode(&file.ciphertext)?;

        let key = derive(master, &salt, file.kdf).map_err(other)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| UnlockError::Other("bad key".into()))?;
        let plain = cipher
            .decrypt(&XNonce::from(nonce), Payload { msg: &ciphertext, aad: AAD })
            .map_err(|_| UnlockError::WrongPassword)?;
        let plain = Zeroizing::new(plain);
        let corrupt = |e: serde_json::Error| UnlockError::Other(format!("corrupt vault: {e}"));
        let data: Data = if file.version == 1 { Data::from_v1(serde_json::from_slice(&plain).map_err(corrupt)?) } else { serde_json::from_slice(&plain).map_err(corrupt)? };
        self.state = Some(Unlocked { key, kdf: file.kdf, salt, data, last_used: Instant::now() });
        if file.version == 1 {
            // Keep the old file next to the new one, once, before it is overwritten.
            let bak = self.path.with_extension("json.v1.bak");
            if !bak.exists() {
                std::fs::copy(&self.path, &bak).map_err(|e| UnlockError::Other(format!("could not back up the old vault: {e}")))?;
                restrict_permissions(&bak);
            }
            self.save().map_err(other)?;
        }
        Ok(())
    }

    pub fn lock(&mut self) {
        self.state = None;
    }

    /// Counts as activity, postponing the idle lock.
    pub fn touch(&mut self) {
        if let Some(st) = self.state.as_mut() {
            st.last_used = Instant::now();
        }
    }

    /// Locks the vault after `after` without use.
    pub fn lock_if_idle(&mut self, after: Duration) {
        if self.state.as_ref().is_some_and(|s| s.last_used.elapsed() > after) {
            self.lock();
        }
    }

    /// The login or sudo secret linked to a server.
    pub fn get(&mut self, id: u64, kind: Kind) -> Option<String> {
        let st = self.state.as_mut()?;
        st.last_used = Instant::now();
        let link = st.data.host_links.get(&id)?;
        let sid = match kind {
            Kind::Login => link.login,
            Kind::Sudo => link.sudo,
            Kind::Totp => link.totp,
        }?;
        st.data.secrets.get(&sid).map(|s| s.value.clone())
    }

    /// Stores (or, with `None`, removes) the login or sudo secret of a server and saves the vault. `label` names
    /// a secret that has to be created for it.
    pub fn set(&mut self, id: u64, kind: Kind, secret: Option<&str>, label: &str) -> Result<()> {
        if kind == Kind::Totp {
            bail!("a server's one-time codes are linked with set_totp_link");
        }
        let st = self.state.as_mut().context("the vault is locked")?;
        st.last_used = Instant::now();
        let current = st.data.host_links.get(&id).and_then(|l| if kind == Kind::Login { l.login } else { l.sudo });
        match (secret, current) {
            (Some(v), Some(sid)) => {
                if let Some(sec) = st.data.secrets.get_mut(&sid) {
                    sec.value.zeroize();
                    sec.value = v.to_string();
                    sec.updated = now();
                }
            }
            (Some(v), None) => {
                let sid = st.data.new_id();
                let (sk, suffix) = if kind == Kind::Login { (SecretKind::Password, "login") } else { (SecretKind::Sudo, "sudo") };
                st.data.secrets.insert(sid, Secret { id: sid, name: format!("{label} ({suffix})"), kind: sk, value: v.to_string(), legacy_profile: None, env_var: None, agent_visible: false, notes: String::new(), updated: now() });
                let link = st.data.host_links.entry(id).or_default();
                if kind == Kind::Login { link.login = Some(sid) } else { link.sudo = Some(sid) }
            }
            (None, Some(sid)) => {
                let link = st.data.host_links.entry(id).or_default();
                if kind == Kind::Login { link.login = None } else { link.sudo = None }
                if !st.data.referenced(sid, None) {
                    st.data.secrets.remove(&sid);
                }
                if st.data.host_links.get(&id) == Some(&HostSecrets::default()) {
                    st.data.host_links.remove(&id);
                }
            }
            (None, None) => return Ok(()),
        }
        self.save()
    }

    /// The server forgets its secrets (the server was deleted): the ones only it used are deleted too.
    pub fn remove_server(&mut self, id: u64) -> Result<()> {
        let st = self.state.as_mut().context("the vault is locked")?;
        let Some(link) = st.data.host_links.remove(&id) else { return Ok(()) };
        for sid in [link.login, link.sudo, link.key, link.totp].into_iter().flatten() {
            if !st.data.referenced(sid, None) {
                st.data.secrets.remove(&sid);
            }
        }
        self.save()
    }

    /// The secrets a server is linked to.
    pub fn links(&self, id: u64) -> HostSecrets {
        self.state.as_ref().and_then(|st| st.data.host_links.get(&id).cloned()).unwrap_or_default()
    }

    /// Links a server to an existing secret (its key, say), or unlinks with `None`.
    pub fn set_key_link(&mut self, server: u64, secret: Option<u64>) -> Result<()> {
        let st = self.state.as_mut().context("the vault is locked")?;
        if let Some(sid) = secret {
            match st.data.secrets.get(&sid) {
                Some(s) if s.kind == SecretKind::SshKey => {}
                Some(_) => bail!("that secret is not an SSH key"),
                None => bail!("that secret no longer exists"),
            }
        }
        st.data.host_links.entry(server).or_default().key = secret;
        if st.data.host_links.get(&server) == Some(&HostSecrets::default()) {
            st.data.host_links.remove(&server);
        }
        self.save()
    }

    /// Links a server to the seed of its one-time codes (a `Totp` secret), or unlinks with `None`.
    pub fn set_totp_link(&mut self, server: u64, secret: Option<u64>) -> Result<()> {
        let st = self.state.as_mut().context("the vault is locked")?;
        if let Some(sid) = secret {
            match st.data.secrets.get(&sid) {
                Some(s) if s.kind == SecretKind::Totp => {}
                Some(_) => bail!("that secret is not a one-time-code seed"),
                None => bail!("that secret no longer exists"),
            }
        }
        st.data.host_links.entry(server).or_default().totp = secret;
        if st.data.host_links.get(&server) == Some(&HostSecrets::default()) {
            st.data.host_links.remove(&server);
        }
        self.save()
    }

    /// Gives the secrets a migration named by number the name of their server (`name_of(server id)`).
    pub fn relabel_hosts(&mut self, name_of: impl Fn(u64) -> Option<String>) -> Result<()> {
        let Some(st) = self.state.as_mut() else { return Ok(()) };
        let mut changed = false;
        for (server, link) in &st.data.host_links {
            let Some(name) = name_of(*server) else { continue };
            for (kind, sid) in [(Kind::Login, link.login), (Kind::Sudo, link.sudo)] {
                let Some(sec) = sid.and_then(|i| st.data.secrets.get_mut(&i)) else { continue };
                if sec.name == auto_name(*server, kind) {
                    sec.name = format!("{name} ({})", if kind == Kind::Login { "login" } else { "sudo" });
                    changed = true;
                }
            }
        }
        if changed { self.save() } else { Ok(()) }
    }

    // ------------------------------------------------------------ named secrets

    /// Everything but the values.
    pub fn list(&self) -> Vec<SecretMeta> {
        self.state.as_ref().map(|st| st.data.secrets.values().map(Secret::meta).collect()).unwrap_or_default()
    }

    /// The SSH keys, as `(name, private key text)`, without counting as activity (the agent signs with them: a
    /// stream of logins must not keep the vault from locking itself when it is idle).
    pub fn ssh_keys(&self) -> Vec<(u64, String, Zeroizing<String>)> {
        let Some(st) = self.state.as_ref() else { return vec![] };
        st.data.secrets.values().filter(|s| s.kind == SecretKind::SshKey).map(|s| (s.id, s.name.clone(), Zeroizing::new(s.value.clone()))).collect()
    }

    /// Every secret value that is long enough to be worth hiding (to blank them out of what a program printed).
    pub fn values_to_hide(&self) -> Vec<Zeroizing<String>> {
        let Some(st) = self.state.as_ref() else { return vec![] };
        let mut out: Vec<Zeroizing<String>> = vec![];
        for s in st.data.secrets.values() {
            // a key is several lines: hide each long line of it as well as the whole
            if s.kind == SecretKind::SshKey {
                out.extend(s.value.lines().map(str::trim).filter(|l| l.len() >= 12).map(|l| Zeroizing::new(l.to_string())));
            }
            if s.value.trim().len() >= 6 {
                out.push(Zeroizing::new(s.value.trim().to_string()));
            }
        }
        out
    }

    /// One secret's description (no value).
    pub fn meta(&self, id: u64) -> Option<SecretMeta> {
        self.state.as_ref()?.data.secrets.get(&id).map(Secret::meta)
    }

    /// The value of a secret. Counts as activity.
    pub fn reveal(&mut self, id: u64) -> Option<String> {
        let st = self.state.as_mut()?;
        st.last_used = Instant::now();
        st.data.secrets.get(&id).map(|s| s.value.clone())
    }

    /// Creates (`meta.id == 0`) or changes a secret. `value` is required for a new one; on a change, `None` keeps it.
    pub fn save_secret(&mut self, meta: SecretMeta, value: Option<&str>) -> Result<u64> {
        let st = self.state.as_mut().context("the vault is locked")?;
        st.last_used = Instant::now();
        let name = meta.name.trim().to_string();
        if name.is_empty() {
            bail!("a secret needs a name");
        }
        let env_var = meta.env_var.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(v) = &env_var {
            check_env_name(v)?;
        }
        let existing = st.data.secrets.contains_key(&meta.id);
        if meta.id != 0 && !existing {
            bail!("this secret no longer exists");
        }
        let value = value.filter(|v| !v.is_empty());
        if !existing && value.is_none() {
            bail!("a secret needs a value");
        }
        let id = if existing { meta.id } else { st.data.new_id() };
        let prev = st.data.secrets.remove(&id);
        let changed_value = value.is_some();
        let mut sec = Secret {
            id,
            name,
            kind: meta.kind,
            value: value.map(str::to_string).or_else(|| prev.as_ref().map(|p| p.value.clone())).unwrap_or_default(),
            legacy_profile: None,
            env_var,
            agent_visible: meta.agent_visible,
            notes: meta.notes,
            updated: prev.as_ref().map_or_else(now, |p| p.updated),
        };
        if changed_value || prev.is_none() {
            sec.updated = now();
        }
        st.data.secrets.insert(id, sec);
        // the old copy of the value must not linger in memory
        if let Some(mut p) = prev {
            p.value.zeroize();
        }
        self.save()?;
        Ok(id)
    }

    /// Deletes a secret and every link to it. Returns the servers that lost a link.
    pub fn delete_secret(&mut self, id: u64) -> Result<Vec<u64>> {
        let st = self.state.as_mut().context("the vault is locked")?;
        if let Some(mut s) = st.data.secrets.remove(&id) {
            s.value.zeroize();
        }
        let mut touched = vec![];
        for (server, link) in st.data.host_links.iter_mut() {
            let before = link.clone();
            for slot in [&mut link.login, &mut link.sudo, &mut link.key, &mut link.totp] {
                if *slot == Some(id) {
                    *slot = None;
                }
            }
            if *link != before {
                touched.push(*server);
            }
        }
        st.data.host_links.retain(|_, l| *l != HostSecrets::default());
        self.save()?;
        Ok(touched)
    }

    /// For `oso run`: `(secret name, variable, value, visible to agents)` of the secrets in `ids` that fill a variable.
    pub fn env_with_flags(&mut self, ids: &[u64]) -> Vec<(String, String, Zeroizing<String>, bool)> {
        let Some(st) = self.state.as_mut() else { return vec![] };
        st.last_used = Instant::now();
        let mut rows: Vec<&Secret> = st.data.secrets.values().filter(|s| ids.contains(&s.id)).collect();
        rows.sort_by_key(|s| s.id);
        rows.into_iter().filter_map(|s| s.env_var.clone().map(|v| (s.name.clone(), v, Zeroizing::new(s.value.clone()), s.agent_visible))).collect()
    }

    /// Writes an old profile mark (`#[doc(hidden)]`: only for the tests of the migration, which need a vault as an older
    /// version left it).
    #[doc(hidden)]
    pub fn set_legacy_mark(&mut self, id: u64, profile: &str) -> Result<()> {
        let st = self.state.as_mut().context("the vault is locked")?;
        st.data.secrets.get_mut(&id).context("no such secret")?.legacy_profile = Some(profile.to_string());
        self.save()
    }

    /// The old profile marks that are still in the vault: `(secret, old profile id)`.
    pub fn legacy_marks(&self) -> Vec<(u64, String)> {
        let Some(st) = self.state.as_ref() else { return vec![] };
        st.data.secrets.values().filter_map(|s| s.legacy_profile.clone().filter(|p| !p.is_empty()).map(|p| (s.id, p))).collect()
    }

    /// Drops the old profile marks (their secrets are in a Space now, or in General) and saves.
    pub fn clear_legacy_marks(&mut self) -> Result<()> {
        let st = self.state.as_mut().context("the vault is locked")?;
        let any = st.data.secrets.values().any(|s| s.legacy_profile.is_some());
        for s in st.data.secrets.values_mut() {
            s.legacy_profile = None;
        }
        if any {
            self.save()?;
        }
        Ok(())
    }

    /// `(variable, value)` of the secrets in `ids` that fill a variable (and, when `agent_visible_only`, are marked visible
    /// to agents). In the order of `ids`, so that a later secret wins over an earlier one that fills the same variable.
    pub fn env_for_ids(&mut self, ids: &[u64], agent_visible_only: bool) -> Vec<(String, Zeroizing<String>)> {
        let Some(st) = self.state.as_mut() else { return vec![] };
        st.last_used = Instant::now();
        ids.iter()
            .filter_map(|id| st.data.secrets.get(id))
            .filter(|s| !agent_visible_only || s.agent_visible)
            .filter_map(|s| s.env_var.clone().map(|v| (v, Zeroizing::new(s.value.clone()))))
            .collect()
    }

    /// Re-encrypts everything under a new master password (the vault must be unlocked).
    pub fn change_master(&mut self, new_master: &str) -> Result<()> {
        let st = self.state.as_mut().context("the vault is locked")?;
        let salt = random::<16>()?;
        st.key = derive(new_master, &salt, st.kdf)?;
        st.salt = salt;
        st.last_used = Instant::now();
        self.save()
    }

    fn save(&self) -> Result<()> {
        let st = self.state.as_ref().context("the vault is locked")?;
        let plain = Zeroizing::new(serde_json::to_vec(&st.data)?);
        let nonce = random::<24>()?;
        let cipher = XChaCha20Poly1305::new_from_slice(st.key.as_slice()).map_err(|_| anyhow!("bad key"))?;
        let ciphertext = cipher
            .encrypt(&XNonce::from(nonce), Payload { msg: &plain, aad: AAD })
            .map_err(|_| anyhow!("encryption failed"))?;
        let file = File {
            version: 2,
            kdf: st.kdf,
            salt: B64.encode(st.salt),
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(ciphertext),
        };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&file)?)?;
        restrict_permissions(&tmp);
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Names a variable may have in a terminal's environment. A few are never taken from the vault.
pub fn check_env_name(name: &str) -> Result<()> {
    let ok = !name.is_empty() && name.chars().enumerate().all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()));
    if !ok {
        bail!("{name:?} is not a valid variable name (letters, digits and _, not starting with a digit)");
    }
    let up = name.to_ascii_uppercase();
    if up.starts_with("OSO_") || up.starts_with("LD_") || ["PATH", "HOME", "SHELL", "USER", "TERM", "SSH_AUTH_SOCK", "IFS", "PS1"].contains(&up.as_str()) {
        bail!("{name} is reserved: oso or the system sets it");
    }
    Ok(())
}

/// Owner-only access (0600) on Unix; Windows relies on the user profile ACLs.
fn restrict_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap parameters so tests stay fast; production uses `Kdf::DEFAULT`.
    const FAST: Kdf = Kdf { m_kib: 8, t: 1, p: 1 };

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-vault-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("vault.json")
    }

    #[test]
    fn roundtrip_and_wrong_password() {
        let path = temp("rt");
        let mut v = Vault::new(path.clone());
        v.create_with("master-1", FAST).unwrap();
        v.set(7, Kind::Login, Some("hunter2"), "t").unwrap();
        v.set(7, Kind::Sudo, Some("rootpw"), "t").unwrap();
        v.lock();
        assert!(v.get(7, Kind::Login).is_none(), "locked vault must not answer");

        let mut v = Vault::new(path.clone());
        assert_eq!(v.unlock("nope"), Err(UnlockError::WrongPassword));
        v.unlock("master-1").unwrap();
        assert_eq!(v.get(7, Kind::Login).as_deref(), Some("hunter2"));
        assert_eq!(v.get(7, Kind::Sudo).as_deref(), Some("rootpw"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn file_has_no_plaintext_and_tampering_is_detected() {
        let path = temp("tamper");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        v.set(1, Kind::Login, Some("super-secret-value"), "t").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("super-secret-value"));

        let mut file: File = serde_json::from_str(&raw).unwrap();
        let mut ct = B64.decode(&file.ciphertext).unwrap();
        ct[0] ^= 1;
        file.ciphertext = B64.encode(ct);
        std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
        let mut v = Vault::new(path.clone());
        assert_eq!(v.unlock("m"), Err(UnlockError::WrongPassword));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn change_master_keeps_data() {
        let path = temp("chg");
        let mut v = Vault::new(path.clone());
        v.create_with("old", FAST).unwrap();
        v.set(3, Kind::Login, Some("keep-me"), "t").unwrap();
        v.change_master("new").unwrap();
        v.lock();
        let mut v = Vault::new(path.clone());
        assert_eq!(v.unlock("old"), Err(UnlockError::WrongPassword));
        v.unlock("new").unwrap();
        assert_eq!(v.get(3, Kind::Login).as_deref(), Some("keep-me"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn removing_last_secret_drops_the_entry() {
        let path = temp("rm");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        v.set(5, Kind::Sudo, Some("x"), "t").unwrap();
        v.set(5, Kind::Sudo, None, "t").unwrap();
        assert!(v.get(5, Kind::Sudo).is_none());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn idle_lock() {
        let path = temp("idle");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        v.lock_if_idle(Duration::from_secs(60));
        assert!(v.is_unlocked());
        v.lock_if_idle(Duration::ZERO);
        std::thread::sleep(Duration::from_millis(5));
        v.lock_if_idle(Duration::ZERO);
        assert!(!v.is_unlocked());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vault-v1.json");

    /// A copy of the fixture (a vault written by version 1 of the program, master `fixture-master`).
    fn v1_copy(name: &str) -> PathBuf {
        let path = temp(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(FIXTURE, &path).unwrap();
        path
    }

    #[test]
    fn a_version_1_vault_is_migrated_with_a_backup() {
        let path = v1_copy("mig");
        let original = std::fs::read(&path).unwrap();
        let mut v = Vault::new(path.clone());
        assert_eq!(v.unlock("wrong"), Err(UnlockError::WrongPassword));
        assert!(!path.with_extension("json.v1.bak").exists(), "nothing is touched by a failed unlock");
        v.unlock("fixture-master").unwrap();
        assert_eq!(std::fs::read(path.with_extension("json.v1.bak")).unwrap(), original, "the old file is kept as it was");
        assert_eq!(v.get(1, Kind::Login).as_deref(), Some("login-one"));
        assert_eq!(v.get(1, Kind::Sudo).as_deref(), Some("sudo-one"));
        assert_eq!(v.get(2, Kind::Login).as_deref(), Some("login-two"));
        assert_eq!(v.get(2, Kind::Sudo), None);
        assert_eq!(v.get(3, Kind::Sudo).as_deref(), Some("sudo-three"));
        let kinds: Vec<SecretKind> = v.list().iter().map(|m| m.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == SecretKind::Password).count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == SecretKind::Sudo).count(), 2);
        // the file on disk is version 2 now and opens again
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["version"], 2);
        let mut again = Vault::new(path.clone());
        again.unlock("fixture-master").unwrap();
        assert_eq!(again.get(3, Kind::Sudo).as_deref(), Some("sudo-three"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_migration_names_secrets_after_their_servers() {
        let path = v1_copy("names");
        let mut v = Vault::new(path.clone());
        v.unlock("fixture-master").unwrap();
        v.relabel_hosts(|id| (id == 1).then(|| "web".to_string())).unwrap();
        let names: Vec<String> = v.list().into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"web (login)".to_string()) && names.contains(&"web (sudo)".to_string()), "{names:?}");
        assert!(names.contains(&"server 2 login".to_string()), "a server that is unknown keeps its number");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    fn meta(name: &str, kind: SecretKind) -> SecretMeta {
        SecretMeta { id: 0, name: name.into(), kind, env_var: None, agent_visible: false, notes: String::new(), updated: 0 }
    }

    #[test]
    fn every_kind_survives_a_roundtrip() {
        let path = temp("kinds");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        let kinds = [SecretKind::Password, SecretKind::Sudo, SecretKind::SshKey, SecretKind::ApiKey, SecretKind::Token, SecretKind::Totp];
        let mut ids = vec![];
        for (i, k) in kinds.iter().enumerate() {
            let mut m = meta(&format!("secret-{i}"), *k);
            m.notes = format!("note {i}");
            ids.push(v.save_secret(m, Some(&format!("value-{i}"))).unwrap());
        }
        v.lock();
        let mut v = Vault::new(path.clone());
        v.unlock("m").unwrap();
        for (i, k) in kinds.iter().enumerate() {
            let m = v.meta(ids[i]).unwrap();
            assert_eq!((m.kind, m.name.as_str(), m.notes.as_str()), (*k, format!("secret-{i}").as_str(), format!("note {i}").as_str()));
            assert_eq!(v.reveal(ids[i]).as_deref(), Some(format!("value-{i}").as_str()));
        }
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn names_and_notes_are_sealed_too() {
        let path = temp("sealed");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        let mut m = meta("aws-example-prod", SecretKind::Token);
        m.notes = "rotates on fridays".into();
        v.save_secret(m, Some("v")).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("aws-example-prod") && !raw.contains("fridays"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_list_never_carries_a_value() {
        let path = temp("nolist");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        v.save_secret(meta("k", SecretKind::ApiKey), Some("TRAP-VALUE-123")).unwrap();
        v.set(4, Kind::Login, Some("TRAP-LOGIN-456"), "web").unwrap();
        let json = serde_json::to_string(&v.list()).unwrap();
        assert!(!json.contains("TRAP-") && !json.contains("value"), "{json}");
        assert_eq!(v.list().len(), 2);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn editing_keeps_the_value_unless_a_new_one_is_given() {
        let path = temp("edit");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        let id = v.save_secret(meta("k", SecretKind::Token), Some("first")).unwrap();
        let before = v.meta(id).unwrap().updated;
        let mut m = v.meta(id).unwrap();
        m.name = "renamed".into();
        v.save_secret(m.clone(), None).unwrap();
        assert_eq!(v.reveal(id).as_deref(), Some("first"));
        assert_eq!(v.meta(id).unwrap().updated, before, "renaming is not a rotation");
        v.save_secret(m, Some("second")).unwrap();
        assert_eq!(v.reveal(id).as_deref(), Some("second"));
        assert!(v.save_secret(meta("no value", SecretKind::Token), None).is_err());
        assert!(v.save_secret(meta("  ", SecretKind::Token), Some("x")).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn variables_are_checked_and_come_in_the_order_asked() {
        let path = temp("env");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        let mut a = meta("a", SecretKind::ApiKey);
        a.env_var = Some("ANTHROPIC_API_KEY".into());
        a.agent_visible = true;
        let ia = v.save_secret(a.clone(), Some("va")).unwrap();
        // the same variable in two Spaces is fine: what a terminal gets depends on which secrets are in its reach
        let mut dup = a.clone();
        dup.name = "dup".into();
        let ib = v.save_secret(dup, Some("vb")).unwrap();
        for bad in ["1X", "A B", "PATH", "LD_PRELOAD", "OSO_TOKEN", "a-b"] {
            let mut m = meta("bad", SecretKind::Token);
            m.env_var = Some(bad.into());
            assert!(v.save_secret(m, Some("x")).is_err(), "{bad}");
        }
        assert_eq!(v.env_for_ids(&[ia], false).iter().map(|(k, x)| (k.as_str(), x.as_str())).collect::<Vec<_>>(), [("ANTHROPIC_API_KEY", "va")]);
        assert_eq!(v.env_for_ids(&[ib], false)[0].1.as_str(), "vb");
        let both = v.env_for_ids(&[ia, ib], false);
        assert_eq!(both.last().unwrap().1.as_str(), "vb", "the later one is the one that stays");
        assert_eq!(v.env_with_flags(&[ib]).len(), 1);
        assert!(v.env_for_ids(&[], false).is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn deleting_a_secret_unlinks_it_and_removing_a_server_deletes_what_only_it_used() {
        let path = temp("del");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        v.set(1, Kind::Login, Some("l"), "web").unwrap();
        v.set(1, Kind::Sudo, Some("s"), "web").unwrap();
        let sudo_id = v.links(1).sudo.unwrap();
        assert_eq!(v.delete_secret(sudo_id).unwrap(), vec![1]);
        assert_eq!(v.get(1, Kind::Sudo), None);
        assert_eq!(v.get(1, Kind::Login).as_deref(), Some("l"));
        v.remove_server(1).unwrap();
        assert!(v.list().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_key_link_only_takes_ssh_keys() {
        let path = temp("keylink");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        let tok = v.save_secret(meta("t", SecretKind::Token), Some("x")).unwrap();
        let key = v.save_secret(meta("k", SecretKind::SshKey), Some("x")).unwrap();
        assert!(v.set_key_link(1, Some(tok)).is_err());
        v.set_key_link(1, Some(key)).unwrap();
        assert_eq!(v.links(1).key, Some(key));
        v.set_key_link(1, None).unwrap();
        assert_eq!(v.links(1), HostSecrets::default());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_locked_vault_lists_and_reveals_nothing() {
        let path = temp("locked");
        let mut v = Vault::new(path.clone());
        v.create_with("m", FAST).unwrap();
        let id = v.save_secret(meta("k", SecretKind::Token), Some("x")).unwrap();
        v.lock();
        assert!(v.list().is_empty() && v.reveal(id).is_none() && v.save_secret(meta("z", SecretKind::Token), Some("x")).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
