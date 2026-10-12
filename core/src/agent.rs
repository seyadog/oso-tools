//! oso's own ssh-agent. The keys kept in the vault are offered to `ssh` through a private socket, and only while
//! the vault is unlocked: with it locked there are no identities to list and nothing to sign. The private keys never
//! leave this process; `ssh` receives signatures only.
//!
//! It speaks the small part of the agent protocol that `ssh` and `git` use (list the identities, sign). Anything
//! that is not in the vault is passed on to the agent that was running when oso started (`SSH_AUTH_SOCK`), so the
//! keys a person already has keep working in oso's local terminals. The protocol is written by hand: it is a page
//! long, and the crates for it bring an async runtime this program does not otherwise need.

use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
/// Messages larger than this are not agent messages.
const MAX_MESSAGE: usize = 256 * 1024;

/// Where the keys come from. Both calls answer "nothing" while the vault is locked.
pub trait KeyProvider: Send + Sync {
    /// `(public key blob, comment)` of every key.
    fn identities(&self) -> Vec<(Vec<u8>, String)>;
    /// The signature (as the wire blob `string algorithm, string signature`) of `data` by the key with this public blob.
    /// `flags` are the agent's: 2 asks for rsa-sha2-256, 4 for rsa-sha2-512.
    fn sign(&self, blob: &[u8], data: &[u8], flags: u32) -> Option<Vec<u8>>;
}

/// Where the agent listens: inside the config dir if `OSO_CONFIG_DIR` is set (separate setups and tests do not
/// share one), otherwise in the user's private runtime directory.
pub fn socket_path() -> PathBuf {
    if let Some(d) = std::env::var_os("OSO_CONFIG_DIR") {
        return PathBuf::from(d).join("agent.sock");
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    if std::env::var_os("XDG_RUNTIME_DIR").is_some() { base.join("oso").join("agent.sock") } else { base.join(format!("oso-{uid}")).join("agent.sock") }
}

/// A running agent. Dropping it stops it and removes the socket.
pub struct Agent {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Agent {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Starts the agent on `path` (mode 0600, in a 0700 directory). Fails if another one is already answering there.
pub fn start(path: PathBuf, keys: Arc<dyn KeyProvider>, upstream: Option<PathBuf>) -> Result<Agent> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!("another oso is already serving {}", path.display());
        }
        let _ = std::fs::remove_file(&path); // the socket of a dead one
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("could not listen on {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    // do not send anything to ourselves
    let upstream = upstream.filter(|u| *u != path);
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((conn, _)) => {
                    let keys = Arc::clone(&keys);
                    let upstream = upstream.clone();
                    std::thread::spawn(move || {
                        let _ = conn.set_nonblocking(false);
                        let _ = serve(conn, keys.as_ref(), upstream.as_deref());
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(40)),
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    });
    Ok(Agent { path, stop })
}

// ------------------------------------------------------------------ wire format

fn read_message(r: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_MESSAGE {
        return Err(std::io::Error::other("not an agent message"));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body)?;
    Ok(body)
}

fn write_message(w: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    w.write_all(&(body.len() as u32).to_be_bytes())?;
    w.write_all(body)?;
    w.flush()
}

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Reads fields out of a message, refusing to go past its end.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn u32(&mut self) -> Option<u32> {
        let (head, rest) = self.0.split_at_checked(4)?;
        self.0 = rest;
        Some(u32::from_be_bytes(head.try_into().ok()?))
    }
    fn string(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
}

/// The identities of a message type 12 answer.
fn parse_identities(answer: &[u8]) -> Option<Vec<(Vec<u8>, String)>> {
    let (&kind, rest) = answer.split_first()?;
    if kind != IDENTITIES_ANSWER {
        return None;
    }
    let mut c = Cursor(rest);
    let n = c.u32()?;
    let mut out = vec![];
    for _ in 0..n.min(1024) {
        let blob = c.string()?.to_vec();
        let comment = String::from_utf8_lossy(c.string()?).into_owned();
        out.push((blob, comment));
    }
    Some(out)
}

/// Asks the agent that was running before oso (a short conversation: one request, one answer).
fn ask_upstream(path: &Path, request: &[u8]) -> Option<Vec<u8>> {
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(10))).ok()?;
    write_message(&mut s, request).ok()?;
    read_message(&mut s).ok()
}

fn serve(mut conn: UnixStream, keys: &dyn KeyProvider, upstream: Option<&Path>) -> std::io::Result<()> {
    loop {
        let msg = match read_message(&mut conn) {
            Ok(m) => m,
            Err(_) => return Ok(()), // the client went away
        };
        let reply = answer(&msg, keys, upstream);
        write_message(&mut conn, &reply)?;
    }
}

fn answer(msg: &[u8], keys: &dyn KeyProvider, upstream: Option<&Path>) -> Vec<u8> {
    let Some((&kind, rest)) = msg.split_first() else { return vec![FAILURE] };
    match kind {
        REQUEST_IDENTITIES => {
            let mut all = keys.identities();
            if let Some(more) = upstream.and_then(|u| ask_upstream(u, msg)).and_then(|a| parse_identities(&a)) {
                for (blob, comment) in more {
                    if !all.iter().any(|(b, _)| *b == blob) {
                        all.push((blob, comment));
                    }
                }
            }
            let mut out = vec![IDENTITIES_ANSWER];
            out.extend_from_slice(&(all.len() as u32).to_be_bytes());
            for (blob, comment) in &all {
                put_string(&mut out, blob);
                put_string(&mut out, comment.as_bytes());
            }
            out
        }
        SIGN_REQUEST => {
            let mut c = Cursor(rest);
            let (Some(blob), Some(data), Some(flags)) = (c.string(), c.string(), c.u32()) else { return vec![FAILURE] };
            if let Some(sig) = keys.sign(blob, data, flags) {
                let mut out = vec![SIGN_RESPONSE];
                put_string(&mut out, &sig);
                return out;
            }
            // not a key of the vault: the agent that was there before may know it
            if keys.identities().iter().any(|(b, _)| b == blob) {
                return vec![FAILURE]; // it is ours, and we could not sign: never ask anyone else
            }
            upstream.and_then(|u| ask_upstream(u, msg)).unwrap_or_else(|| vec![FAILURE])
        }
        _ => vec![FAILURE],
    }
}

// ------------------------------------------------------------------ signing

/// The RSA key from its OpenSSH parts. (`ssh-key` 0.6 has a conversion for this, but it builds the key from `p` twice
/// instead of `p` and `q`, so it never works: the factors are passed in by hand here.)
fn rsa_private_key(pair: &ssh_key::private::RsaKeypair) -> Option<rsa::RsaPrivateKey> {
    let big = |m: &ssh_key::Mpint| rsa::BigUint::try_from(m).ok();
    let key = rsa::RsaPrivateKey::from_components(big(&pair.public.n)?, big(&pair.public.e)?, big(&pair.private.d)?, vec![big(&pair.private.p)?, big(&pair.private.q)?]).ok()?;
    key.validate().ok()?;
    Some(key)
}

/// The public blob and comment of a private key.
pub fn identity_of(key: &ssh_key::PrivateKey, comment: &str) -> Option<(Vec<u8>, String)> {
    Some((key.public_key().to_bytes().ok()?, comment.to_string()))
}

/// Signs `data` with `key` the way `ssh` asked. For RSA the flags choose SHA-256 or SHA-512; the old SHA-1
/// signature (flags 0) is refused.
pub fn sign_with(key: &ssh_key::PrivateKey, data: &[u8], flags: u32) -> Option<Vec<u8>> {
    use ssh_key::{Algorithm, HashAlg, Signature, private::KeypairData};
    use ssh_encoding::Encode;
    let sig: Signature = match key.key_data() {
        KeypairData::Rsa(pair) => {
            let want_256 = flags & 2 != 0 && flags & 4 == 0;
            let want_512 = flags & 4 != 0;
            if !want_256 && !want_512 {
                return None;
            }
            let rsa_key = rsa_private_key(pair)?;
            let (alg, bytes) = if want_256 {
                use rsa::signature::{SignatureEncoding, Signer};
                let s = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(rsa_key).try_sign(data).ok()?;
                (HashAlg::Sha256, s.to_vec())
            } else {
                use rsa::signature::{SignatureEncoding, Signer};
                let s = rsa::pkcs1v15::SigningKey::<sha2::Sha512>::new(rsa_key).try_sign(data).ok()?;
                (HashAlg::Sha512, s.to_vec())
            };
            Signature::new(Algorithm::Rsa { hash: Some(alg) }, bytes).ok()?
        }
        _ => {
            use signature::Signer;
            key.try_sign(data).ok()?
        }
    };
    let mut blob = Vec::new();
    sig.encode(&mut blob).ok()?;
    Some(blob)
}
