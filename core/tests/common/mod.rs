//! A real `sshd` on a high port with throwaway keys, shared by the tests that need one.
#![allow(dead_code)]

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Sshd {
    pub child: Child,
    pub port: u16,
    pub dir: PathBuf,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

pub fn which(name: &str) -> Option<PathBuf> {
    ["/usr/sbin", "/usr/bin", "/sbin", "/bin", "/usr/local/sbin", "/usr/local/bin"].iter().map(|d| Path::new(d).join(name)).find(|p| p.exists())
}

pub fn start_sshd(tag: &str) -> Option<Sshd> {
    let sshd = which("sshd")?;
    which("ssh-keygen")?;
    which("scp")?;
    let dir = std::env::temp_dir().join(format!("oso-sshd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    for k in ["host", "client"] {
        let ok = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", ""]).arg("-f").arg(dir.join(k)).status().ok()?.success();
        if !ok {
            return None;
        }
    }
    std::fs::copy(dir.join("client.pub"), dir.join("authorized_keys")).ok()?;
    let port = free_port();
    let cfg = format!(
        "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host\nAuthorizedKeysFile {d}/authorized_keys\nPidFile {d}/sshd.pid\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nStrictModes no\nAllowTcpForwarding yes\nGatewayPorts no\nSubsystem sftp internal-sftp\n",
        d = dir.display()
    );
    std::fs::write(dir.join("sshd_config"), cfg).ok()?;
    let child = Command::new(sshd).arg("-D").arg("-e").arg("-f").arg(dir.join("sshd_config")).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    let me = Sshd { child, port, dir };
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some(me);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

