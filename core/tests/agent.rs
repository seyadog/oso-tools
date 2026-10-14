//! oso's ssh-agent: signing only while the vault is open, passing on to the agent that was there before, and real
//! logins (through a throwaway `sshd`) with keys that exist only in the vault.

mod common;

use common::*;
use oso_core::agent::{self, KeyProvider};
use oso_core::engine::{Emit, Engine, OpenError};
use oso_core::store::{Auth, Forward, ForwardKind, Server, Store};
use oso_core::vault::{SecretKind, SecretMeta, Vault};
use ssh_encoding::Decode;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oso-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A throwaway key pair made by `ssh-keygen`; returns the private key's text and the path of the private file.
fn keygen(dir: &Path, name: &str, kind: &str) -> Option<(String, PathBuf)> {
    which("ssh-keygen")?;
    let file = dir.join(name);
    let ok = Command::new("ssh-keygen").args(["-q", "-t", kind, "-N", "", "-C", name]).args(if kind == "rsa" { vec!["-b", "2048"] } else { vec![] }).arg("-f").arg(&file).status().ok()?.success();
    ok.then(|| (std::fs::read_to_string(&file).unwrap(), file))
}

fn engine_in(dir: &Path) -> Arc<Mutex<Engine>> {
    let emit: Emit = Arc::new(|_, _| {});
    let store = Store::load_from(dir.join("servers.json")).unwrap();
    let mut e = Engine::new(store, Vault::new(dir.join("vault.json")), emit);
    e.vault_unlock("master").unwrap();
    Arc::new(Mutex::new(e))
}

fn meta(name: &str, kind: SecretKind) -> SecretMeta {
    SecretMeta { id: 0, name: name.into(), kind, env_var: None, agent_visible: false, notes: String::new(), updated: 0 }
}

// ---- a client of the agent protocol, to test it from the outside

fn talk(sock: &Path, body: &[u8]) -> Vec<u8> {
    let mut s = UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(&(body.len() as u32).to_be_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut reply = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut reply).unwrap();
    reply
}

fn string(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

fn identities(sock: &Path) -> Vec<(Vec<u8>, String)> {
    let r = talk(sock, &[11]);
    assert_eq!(r[0], 12, "an identities answer");
    let n = u32::from_be_bytes(r[1..5].try_into().unwrap());
    let mut rest = &r[5..];
    let mut out = vec![];
    let take = |rest: &mut &[u8]| {
        let n = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
        let v = rest[4..4 + n].to_vec();
        *rest = &rest[4 + n..];
        v
    };
    for _ in 0..n {
        let blob = take(&mut rest);
        let comment = String::from_utf8(take(&mut rest)).unwrap();
        out.push((blob, comment));
    }
    out
}

fn sign(sock: &Path, blob: &[u8], data: &[u8], flags: u32) -> Option<Vec<u8>> {
    let mut m = vec![13];
    string(&mut m, blob);
    string(&mut m, data);
    m.extend_from_slice(&flags.to_be_bytes());
    let r = talk(sock, &m);
    if r[0] != 14 {
        return None;
    }
    let n = u32::from_be_bytes(r[1..5].try_into().unwrap()) as usize;
    Some(r[5..5 + n].to_vec())
}

/// Checks a signature blob against the public key (the way `sshd` would).
fn verifies(public: &ssh_key::PublicKey, data: &[u8], blob: &[u8]) -> bool {
    use signature::Verifier;
    let mut slice = blob;
    let Ok(sig) = ssh_key::Signature::decode(&mut slice) else { return false };
    Verifier::verify(public, data, &sig).is_ok()
}

macro_rules! need_keygen {
    ($dir:expr, $name:expr, $kind:expr) => {
        match keygen(&$dir, $name, $kind) {
            Some(k) => k,
            None => {
                eprintln!("skipped: no ssh-keygen here");
                return;
            }
        }
    };
}

#[test]
fn the_agent_signs_with_the_vault_open_and_has_nothing_when_it_is_locked() {
    let dir = temp("sign");
    let (text, _) = need_keygen!(dir, "work", "ed25519");
    let engine = engine_in(&dir);
    let id = engine.lock().unwrap().secret_save(meta("work key", SecretKind::SshKey), Some(&text)).unwrap();
    let sock = dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode_bits(), 0o600);

    let ids = identities(&sock);
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].1, "work key");
    let public = ssh_key::PrivateKey::from_openssh(&text).unwrap().public_key().clone();
    assert_eq!(ids[0].0, public.to_bytes().unwrap());
    let sig = sign(&sock, &ids[0].0, b"data to sign", 0).expect("signs while the vault is open");
    assert!(verifies(&public, b"data to sign", &sig), "a real ed25519 signature");
    assert!(!verifies(&public, b"other data", &sig));
    // a key that is not in the vault is not signed with
    assert!(sign(&sock, b"not a key", b"x", 0).is_none());

    engine.lock().unwrap().vault_lock();
    assert!(identities(&sock).is_empty(), "locked: no identities");
    assert!(sign(&sock, &ids[0].0, b"data to sign", 0).is_none(), "locked: nothing is signed");
    engine.lock().unwrap().vault_unlock("master").unwrap();
    assert_eq!(identities(&sock).len(), 1, "and it is back when the vault is");
    // deleting the secret removes the identity
    engine.lock().unwrap().secret_delete(id).unwrap();
    assert!(identities(&sock).is_empty());
}

#[test]
fn the_agent_of_a_space_offers_the_keys_of_that_space_and_general_and_signs_with_no_other() {
    let dir = temp("space-agent");
    let (a_text, _) = need_keygen!(dir, "example", "ed25519");
    let (b_text, _) = need_keygen!(dir, "beta", "ed25519");
    let (g_text, _) = need_keygen!(dir, "shared", "ed25519");
    let engine = engine_in(&dir);
    let (a_id, b_id, g_id) = {
        let mut e = engine.lock().unwrap();
        (e.secret_save(meta("example key", SecretKind::SshKey), Some(&a_text)).unwrap(), e.secret_save(meta("beta key", SecretKind::SshKey), Some(&b_text)).unwrap(), e.secret_save(meta("shared key", SecretKind::SshKey), Some(&g_text)).unwrap())
    };
    let sock = dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    assert_eq!(identities(&sock).len(), 3, "the ordinary agent (the one logins use) offers every key of the vault");
    let (space, other) = {
        let mut e = engine.lock().unwrap();
        (e.space_save(&oso_core::spaces::Space { name: "Example".into(), secrets: vec![a_id], ..Default::default() }).unwrap(), e.space_save(&oso_core::spaces::Space { name: "Other".into(), secrets: vec![b_id], ..Default::default() }).unwrap())
    };
    let _ = (other, g_id);
    let space_sock = engine.lock().unwrap().space_agent_sock(space).expect("the Space has its own agent");
    assert_ne!(space_sock, sock);
    assert_eq!(std::fs::metadata(&space_sock).unwrap().permissions().mode_bits(), 0o600);
    let names = |s: &Path| {
        let mut n: Vec<String> = identities(s).into_iter().map(|i| i.1).collect();
        n.sort();
        n
    };
    assert_eq!(names(&space_sock), ["example key", "shared key"], "the Space's key and General's, which is shared; not Other's");
    // General has an agent too: only what is in no Space
    let general_sock = engine.lock().unwrap().space_agent_sock(0).expect("General has its own agent");
    assert_eq!(names(&general_sock), ["shared key"]);
    let example = ssh_key::PrivateKey::from_openssh(&a_text).unwrap().public_key().clone();
    let beta = ssh_key::PrivateKey::from_openssh(&b_text).unwrap().public_key().clone();
    assert!(verifies(&example, b"data", &sign(&space_sock, &example.to_bytes().unwrap(), b"data", 0).expect("signs with its own key")));
    assert!(sign(&space_sock, &beta.to_bytes().unwrap(), b"data", 0).is_none(), "and with no key of another Space");
    assert!(sign(&general_sock, &example.to_bytes().unwrap(), b"data", 0).is_none(), "General does not sign with a Space's key");
    // the same socket is used again, and changing the Space changes what it offers without a restart
    assert_eq!(engine.lock().unwrap().space_agent_sock(space), Some(space_sock.clone()));
    let mut changed = engine.lock().unwrap().spaces_list()[0].clone();
    changed.secrets.clear();
    engine.lock().unwrap().space_save(&changed).unwrap();
    assert_eq!(names(&space_sock), ["example key", "shared key"], "a key that left its Space is in General, and General's is everywhere");
    assert_eq!(names(&general_sock), ["example key", "shared key"], "and General offers it too");
    // locked: nothing, as everywhere
    changed.secrets = vec![a_id];
    engine.lock().unwrap().space_save(&changed).unwrap();
    engine.lock().unwrap().vault_lock();
    assert!(identities(&space_sock).is_empty() && identities(&general_sock).is_empty());
    // deleting the Space takes its agent away
    engine.lock().unwrap().space_delete(space).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(!space_sock.exists());
}

trait ModeBits {
    fn mode_bits(&self) -> u32;
}
impl ModeBits for std::fs::Permissions {
    fn mode_bits(&self) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        self.mode() & 0o777
    }
}

#[test]
fn rsa_keys_sign_with_sha2_and_never_with_sha1() {
    let dir = temp("rsa");
    let (text, _) = need_keygen!(dir, "rsa", "rsa");
    let engine = engine_in(&dir);
    engine.lock().unwrap().secret_save(meta("rsa key", SecretKind::SshKey), Some(&text)).unwrap();
    let sock = dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    let blob = identities(&sock).remove(0).0;
    let public = ssh_key::PrivateKey::from_openssh(&text).unwrap().public_key().clone();
    for (flags, alg) in [(2u32, "rsa-sha2-256"), (4, "rsa-sha2-512")] {
        let sig = sign(&sock, &blob, b"payload", flags).unwrap_or_else(|| panic!("flags {flags}"));
        assert!(sig.windows(alg.len()).any(|w| w == alg.as_bytes()), "the signature says {alg}");
        assert!(verifies(&public, b"payload", &sig), "{alg}");
    }
    assert!(sign(&sock, &blob, b"payload", 0).is_none(), "the SHA-1 signature is refused");
}

#[test]
fn a_key_with_a_passphrase_is_refused_with_the_way_out() {
    let dir = temp("pass");
    which("ssh-keygen").map(|_| ()).unwrap_or(());
    if which("ssh-keygen").is_none() {
        return eprintln!("skipped: no ssh-keygen here");
    }
    let file = dir.join("locked");
    assert!(Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "a-passphrase", "-f"]).arg(&file).status().unwrap().success());
    let engine = engine_in(&dir);
    let err = engine.lock().unwrap().secret_save(meta("locked", SecretKind::SshKey), Some(&std::fs::read_to_string(&file).unwrap())).unwrap_err();
    assert!(err.to_string().contains("passphrase"), "{err}");
    let err = engine.lock().unwrap().secret_save(meta("junk", SecretKind::SshKey), Some("not a key")).unwrap_err();
    assert!(err.to_string().contains("OpenSSH"), "{err}");
}

#[test]
fn ssh_add_sees_the_keys_of_the_vault() {
    let dir = temp("sshadd");
    let (text, _) = need_keygen!(dir, "k", "ed25519");
    if which("ssh-add").is_none() {
        return eprintln!("skipped: no ssh-add here");
    }
    let engine = engine_in(&dir);
    engine.lock().unwrap().secret_save(meta("k", SecretKind::SshKey), Some(&text)).unwrap();
    let sock = dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    let out = Command::new("ssh-add").arg("-L").env("SSH_AUTH_SOCK", &sock).output().unwrap();
    let listed = String::from_utf8_lossy(&out.stdout).to_string();
    let public = ssh_key::PrivateKey::from_openssh(&text).unwrap().public_key().to_openssh().unwrap();
    assert!(out.status.success() && listed.contains(public.split_whitespace().nth(1).unwrap()), "{listed} / {}", String::from_utf8_lossy(&out.stderr));
}

/// A key source for the "agent that was there before".
struct Fixed(Vec<(Vec<u8>, String)>, Option<Vec<u8>>);
impl KeyProvider for Fixed {
    fn identities(&self) -> Vec<(Vec<u8>, String)> {
        self.0.clone()
    }
    fn sign(&self, blob: &[u8], _data: &[u8], _flags: u32) -> Option<Vec<u8>> {
        self.0.iter().any(|(b, _)| b == blob).then(|| self.1.clone()).flatten()
    }
}

#[test]
fn what_the_vault_does_not_have_is_asked_of_the_agent_that_was_there_before() {
    let dir = temp("upstream");
    let (text, _) = need_keygen!(dir, "mine", "ed25519");
    let before = agent::start(dir.join("before.sock"), Arc::new(Fixed(vec![(b"their-key".to_vec(), "theirs".into())], Some(b"their-signature".to_vec()))), None).unwrap();
    let engine = engine_in(&dir);
    engine.lock().unwrap().secret_save(meta("mine", SecretKind::SshKey), Some(&text)).unwrap();
    let sock = dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock.clone(), Some(before.path().to_path_buf())).unwrap();
    let ids = identities(&sock);
    assert_eq!(ids.iter().map(|i| i.1.as_str()).collect::<Vec<_>>(), vec!["mine", "theirs"], "the vault's first, then the others");
    assert_eq!(sign(&sock, b"their-key", b"x", 0).as_deref(), Some(&b"their-signature"[..]), "their key signs through us");
    // the vault locked: only theirs is left, and ours is not asked of them
    engine.lock().unwrap().vault_lock();
    assert_eq!(identities(&sock).iter().map(|i| i.1.as_str()).collect::<Vec<_>>(), vec!["theirs"]);
    assert!(sign(&sock, &ids[0].0, b"x", 0).is_none());
}

#[test]
fn a_second_oso_does_not_take_over_a_live_socket_and_a_dead_one_is_replaced() {
    let dir = temp("twice");
    let engine = engine_in(&dir);
    let sock = dir.join("agent.sock");
    let first = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    assert!(Engine::start_agent_at(&engine, sock.clone(), None).is_err(), "someone is answering there");
    drop(first);
    assert!(!sock.exists(), "stopping removes the socket");
    // a socket file nobody listens on (the last oso died)
    drop(std::os::unix::net::UnixListener::bind(&sock).unwrap());
    assert!(sock.exists());
    let _again = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    assert_eq!(identities(&sock).len(), 0);
}

#[test]
fn garbage_does_not_bring_the_agent_down() {
    let dir = temp("garbage");
    let engine = engine_in(&dir);
    let sock = dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock.clone(), None).unwrap();
    {
        let mut s = UnixStream::connect(&sock).unwrap();
        s.write_all(&[0xff, 0xff, 0xff, 0xff, 1, 2, 3]).unwrap(); // a giant message
    }
    assert_eq!(talk(&sock, &[99])[0], 5, "an unknown request fails");
    assert_eq!(talk(&sock, &[13, 0, 0, 0, 9, 1])[0], 5, "a cut-off sign request fails");
    assert_eq!(identities(&sock).len(), 0, "and it still answers");
}

// ---- real logins

fn user() -> String {
    String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string()
}

fn server_for(name: &str, sshd: &Sshd, jump: Option<u64>) -> Server {
    Server {
        id: 0,
        name: name.into(),
        host: "127.0.0.1".into(),
        port: sshd.port,
        user: user(),
        auth: Auth::VaultKey,
        key_path: String::new(),
        parent: None,
        jump,
        has_secret: false,
        has_sudo: false,
        has_totp: false,
        forwards: vec![],
        notes: String::new(),
        env_kind: None,
        color: None,
        proxy: None,
        network: None,
        ai_policy: Default::default(),
    }
}

fn greeter(text: &'static str) -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut c = c;
            let _ = c.write_all(text.as_bytes());
        }
    });
    port
}

fn read_from(port: u16) -> String {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(mut c) = TcpStream::connect(("127.0.0.1", port)) {
            c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut s = String::new();
            let _ = c.read_to_string(&mut s);
            if !s.is_empty() || Instant::now() > end {
                return s;
            }
        } else if Instant::now() > end {
            return String::new();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_state(e: &Arc<Mutex<Engine>>, want: &str) -> (String, String) {
    let end = Instant::now() + Duration::from_secs(25);
    loop {
        let t = e.lock().unwrap().tunnels().into_iter().next().expect("a tunnel");
        if t.state == want || Instant::now() > end {
            return (t.state.to_string(), t.message);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn test_options(e: &mut Engine) {
    e.ssh_extra = ["-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"].iter().map(|s| s.to_string()).collect();
}

#[test]
fn a_server_logs_in_with_a_key_that_exists_only_in_the_vault() {
    let Some(sshd) = start_sshd("vaultkey") else { return eprintln!("skipped: no sshd here") };
    let engine = engine_in(&sshd.dir);
    test_options(&mut engine.lock().unwrap());
    let sock = sshd.dir.join("agent.sock");
    let _agent = Engine::start_agent_at(&engine, sock, None).unwrap();
    let target = greeter("hello-through-the-vault-key");
    let bind = free_port();
    let key_text = std::fs::read_to_string(sshd.dir.join("client")).unwrap();
    let (id, fid) = {
        let mut e = engine.lock().unwrap();
        let secret = e.secret_save(meta("sshd key", SecretKind::SshKey), Some(&key_text)).unwrap();
        // the private key file is removed: from now on the key lives only in the vault
        std::fs::remove_file(sshd.dir.join("client")).unwrap();
        let id = e.save_server(server_for("vk", &sshd, None), None, None).unwrap();
        let f = Forward { id: 0, name: String::new(), auto: false, kind: ForwardKind::Local, bind_port: bind, host: "127.0.0.1".into(), port: target };
        e.set_forwards(id, vec![f]).unwrap();
        e.server_set_key(id, Some(secret)).unwrap();
        (id, e.store.server(id).unwrap().forwards[0].id)
    };
    assert!(engine.lock().unwrap().store.key_files.contains_key(&id), "the public half is written for ssh -i");
    let pub_file = engine.lock().unwrap().store.key_files[&id].clone();
    assert!(!std::fs::read_to_string(&pub_file).unwrap().contains("PRIVATE"), "only the public half is on disk");
    engine.lock().unwrap().tunnel_start(id, fid, true).unwrap();
    let (state, message) = wait_state(&engine, "up");
    assert_eq!(state, "up", "ssh logged in through oso's agent: {message}");
    assert_eq!(read_from(bind), "hello-through-the-vault-key");
    engine.lock().unwrap().tunnel_stop(id, fid);

    // with the vault locked the server asks for it instead of failing at ssh
    engine.lock().unwrap().vault_lock();
    assert_eq!(engine.lock().unwrap().tunnel_start(id, fid, true).err(), Some(OpenError::VaultLocked));
    // and even if someone insisted, the agent has no key to give
    let ids = identities(&sshd.dir.join("agent.sock"));
    assert!(ids.is_empty());
}

#[test]
fn each_hop_of_a_chain_signs_with_its_own_key() {
    let Some(hop) = start_sshd("hop") else { return eprintln!("skipped: no sshd here") };
    let Some(dest) = start_sshd("dest") else { return };
    let engine = engine_in(&hop.dir);
    test_options(&mut engine.lock().unwrap());
    // the hop's own ssh (inside the ProxyCommand) must trust the throwaway sshd too
    engine.lock().unwrap().store.extra_options = ["-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"].iter().map(|s| s.to_string()).collect();
    let _agent = Engine::start_agent_at(&engine, hop.dir.join("agent.sock"), None).unwrap();
    let target = greeter("hello-from-behind-two-keys");
    let bind = free_port();
    let (id, fid) = {
        let mut e = engine.lock().unwrap();
        // each sshd only knows its own key: a hop that signed with the other one would be refused
        let k_hop = e.secret_save(meta("hop key", SecretKind::SshKey), Some(&std::fs::read_to_string(hop.dir.join("client")).unwrap())).unwrap();
        let k_dest = e.secret_save(meta("dest key", SecretKind::SshKey), Some(&std::fs::read_to_string(dest.dir.join("client")).unwrap())).unwrap();
        let a = e.save_server(server_for("hop", &hop, None), None, None).unwrap();
        let b = e.save_server(server_for("dest", &dest, Some(a)), None, None).unwrap();
        e.server_set_key(a, Some(k_hop)).unwrap();
        e.server_set_key(b, Some(k_dest)).unwrap();
        let f = Forward { id: 0, name: String::new(), auto: false, kind: ForwardKind::Local, bind_port: bind, host: "127.0.0.1".into(), port: target };
        e.set_forwards(b, vec![f]).unwrap();
        let fid = e.store.server(b).unwrap().forwards[0].id;
        (b, fid)
    };
    let argv = {
        let e = engine.lock().unwrap();
        oso_core::connect::ssh_argv(&e.store, e.store.server(id).unwrap())
    };
    let pc = argv.iter().find(|a| a.starts_with("ProxyCommand=")).expect("a chain with keys goes through ProxyCommand");
    assert!(pc.contains(".pub"), "the hop names its own public key: {pc}");
    engine.lock().unwrap().tunnel_start(id, fid, true).unwrap();
    let (state, message) = wait_state(&engine, "up");
    assert_eq!(state, "up", "{message}");
    assert_eq!(read_from(bind), "hello-from-behind-two-keys");
}
