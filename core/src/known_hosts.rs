//! The list of servers whose keys ssh has seen (`~/.ssh/known_hosts`). When a server is reinstalled its key
//! changes and ssh refuses to connect until the old key is forgotten: this does that, and only that.

use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

/// A host name as ssh writes it in known_hosts: `name`, `1.2.3.4`, `[name]:2222`. Nothing that could be taken for
/// an option or carry a shell/glob character.
pub fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 255
        && !h.starts_with('-')
        && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:[]%".contains(c))
}

/// Removes the saved key(s) of `host` (`file`: another known_hosts than the user's, for tests).
/// Says what ssh-keygen said ("Host … found: line 3", or that it was not there).
pub fn forget(host: &str, file: Option<&Path>) -> Result<String> {
    if !valid_host(host) {
        bail!("that is not a host name");
    }
    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-R").arg(host);
    if let Some(f) = file {
        cmd.arg("-f").arg(f);
    }
    let out = cmd.output().map_err(|e| anyhow::anyhow!("could not run ssh-keygen: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        bail!("{}", text.trim());
    }
    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oso-kh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn host_names_are_checked() {
        for ok in ["web1.example.com", "10.0.0.5", "[web1]:2222", "fe80::1%eth0", "a_b-c"] {
            assert!(valid_host(ok), "{ok}");
        }
        for bad in ["", "-oProxyCommand=x", "a b", "a;b", "a*", "$(x)", "a\nb"] {
            assert!(!valid_host(bad), "{bad:?}");
        }
    }

    #[test]
    fn forgets_only_the_named_host() {
        if Command::new("ssh-keygen").arg("-V").output().is_err() {
            return;
        }
        let dir = temp("forget");
        let key = dir.join("k");
        let ok = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", ""]).arg("-f").arg(&key).status().unwrap();
        assert!(ok.success());
        let pubkey = std::fs::read_to_string(dir.join("k.pub")).unwrap();
        let keytext = pubkey.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        let kh = dir.join("known_hosts");
        std::fs::write(&kh, format!("web1.example.com {keytext}\ndb1.example.com {keytext}\n[web1.example.com]:2222 {keytext}\n")).unwrap();
        forget("web1.example.com", Some(&kh)).unwrap();
        let left = std::fs::read_to_string(&kh).unwrap();
        assert!(!left.contains("\nweb1.example.com ") && !left.starts_with("web1.example.com "), "{left}");
        assert!(left.contains("db1.example.com "), "{left}");
        assert!(left.contains("[web1.example.com]:2222 "), "{left}");
        forget("[web1.example.com]:2222", Some(&kh)).unwrap();
        assert!(!std::fs::read_to_string(&kh).unwrap().contains("2222"));
        // a host that is not there is not an error
        assert!(forget("nobody.example.com", Some(&kh)).is_ok());
        assert!(forget("-oProxyCommand=x", Some(&kh)).is_err());
    }
}
