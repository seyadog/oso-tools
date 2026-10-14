//! Tunnels and file copies against a real `sshd` started on a high port with throwaway keys. Skipped (and says so)
//! when `sshd` or `ssh-keygen` is not installed or cannot start here.

use oso_core::engine::{Emit, Engine, TransferStatus, TunnelStatus};
use oso_core::store::{Auth, Forward, ForwardKind, Server, Store};
use oso_core::vault::Vault;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod common;
use common::*;

fn engine_for(sshd: &Sshd, forwards: Vec<Forward>) -> (Engine, u64) {
    let emit: Emit = Arc::new(|_, _| {});
    let store = Store::load_from(sshd.dir.join("servers.json")).unwrap();
    let mut e = Engine::new(store, Vault::new(sshd.dir.join("vault.json")), emit);
    e.ssh_extra = ["-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"].iter().map(|s| s.to_string()).collect();
    let user = String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string();
    let id = e.save_server(
        Server {
            id: 0,
            name: "local-sshd".into(),
            host: "127.0.0.1".into(),
            port: sshd.port,
            user,
            auth: Auth::Key,
            key_path: sshd.dir.join("client").to_string_lossy().to_string(),
            parent: None,
            jump: None,
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
        },
        None,
        None,
    )
    .unwrap();
    e.set_forwards(id, forwards).unwrap();
    (e, id)
}

fn fwd(kind: ForwardKind, bind: u16, host: &str, port: u16) -> Forward {
    Forward { id: 0, name: String::new(), auto: false, kind, bind_port: bind, host: host.into(), port }
}

/// A server that says one thing to whoever connects.
fn greeter(text: &'static str) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut c = c;
            let _ = c.write_all(text.as_bytes());
        }
    });
    port
}

fn wait_tunnel(e: &mut Engine, want: &str) -> TunnelStatus {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let t = e.tunnels().into_iter().next().expect("a tunnel");
        if t.state == want || Instant::now() > end {
            return t;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn read_from(port: u16) -> String {
    let end = Instant::now() + Duration::from_secs(5);
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

macro_rules! need_sshd {
    ($tag:expr) => {
        match start_sshd($tag) {
            Some(s) => s,
            None => {
                eprintln!("skipped: no sshd available here");
                return;
            }
        }
    };
}

#[test]
fn a_local_tunnel_reaches_the_target_and_closes_when_switched_off() {
    let sshd = need_sshd!("local");
    let target = greeter("hello-from-the-target");
    let bind = free_port();
    let (mut e, id) = engine_for(&sshd, vec![fwd(ForwardKind::Local, bind, "127.0.0.1", target)]);
    let fid = e.store.server(id).unwrap().forwards[0].id;
    e.tunnel_start(id, fid, true).unwrap();
    let t = wait_tunnel(&mut e, "up");
    assert_eq!(t.state, "up", "{t:?}");
    assert_eq!(read_from(bind), "hello-from-the-target");
    e.tunnel_stop(id, fid);
    assert!(e.tunnels().is_empty());
    std::thread::sleep(Duration::from_millis(600));
    assert!(TcpStream::connect(("127.0.0.1", bind)).is_err(), "the port should be closed again");
}

#[test]
fn the_tunnel_of_a_cluster_leads_to_its_api_and_closes_when_switched_off() {
    use oso_core::k8s::Cluster;
    let sshd = need_sshd!("cluster-tunnel");
    let api = greeter("hello-from-the-api");
    let bind = free_port();
    let (mut e, id) = engine_for(&sshd, vec![]);
    let cid = e.clusters.save(Cluster { name: "k3s".into(), via_server: Some(id), api_port: api, local_port: bind, ..Default::default() }).unwrap();
    // a cluster of this computer has nothing to tunnel
    let own = e.clusters.save(Cluster { name: "own".into(), ..Default::default() }).unwrap();
    assert!(e.cluster_tunnel_start(own, true).is_err());
    e.cluster_tunnel_start(cid, true).unwrap();
    let t = wait_tunnel(&mut e, "up");
    assert_eq!(t.state, "up", "{t:?}");
    assert_eq!(t.forward_id, oso_core::k8s::TUNNEL_BASE + cid);
    assert_eq!(read_from(bind), "hello-from-the-api");
    e.cluster_tunnel_stop(cid);
    assert!(e.tunnels().is_empty());
    std::thread::sleep(Duration::from_millis(600));
    assert!(TcpStream::connect(("127.0.0.1", bind)).is_err(), "the port should be closed again");
}

#[test]
fn a_port_in_use_is_reported_in_plain_words() {
    let sshd = need_sshd!("busy");
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = taken.local_addr().unwrap().port();
    let (mut e, id) = engine_for(&sshd, vec![fwd(ForwardKind::Local, bind, "127.0.0.1", 9)]);
    let fid = e.store.server(id).unwrap().forwards[0].id;
    e.tunnel_start(id, fid, true).unwrap();
    let t = wait_tunnel(&mut e, "failed");
    assert_eq!(t.state, "failed", "{t:?}");
    assert!(t.message.contains("already in use"), "{}", t.message);
}

#[test]
fn a_remote_tunnel_opens_a_port_on_the_server() {
    let sshd = need_sshd!("remote");
    let target = greeter("hello-from-here");
    let bind = free_port();
    let (mut e, id) = engine_for(&sshd, vec![fwd(ForwardKind::Remote, bind, "127.0.0.1", target)]);
    let fid = e.store.server(id).unwrap().forwards[0].id;
    e.tunnel_start(id, fid, true).unwrap();
    let t = wait_tunnel(&mut e, "up");
    assert_eq!(t.state, "up", "{t:?}");
    assert_eq!(read_from(bind), "hello-from-here");
}

#[test]
fn a_dynamic_tunnel_is_a_socks_proxy() {
    let sshd = need_sshd!("socks");
    let bind = free_port();
    let (mut e, id) = engine_for(&sshd, vec![fwd(ForwardKind::Dynamic, bind, "", 0)]);
    let fid = e.store.server(id).unwrap().forwards[0].id;
    e.tunnel_start(id, fid, true).unwrap();
    let t = wait_tunnel(&mut e, "up");
    assert_eq!(t.state, "up", "{t:?}");
    let mut c = TcpStream::connect(("127.0.0.1", bind)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    c.write_all(&[5, 1, 0]).unwrap();
    let mut reply = [0u8; 2];
    c.read_exact(&mut reply).unwrap();
    assert_eq!(reply, [5, 0], "a SOCKS5 server answers the greeting");
}

#[test]
fn removing_a_tunnel_switches_it_off() {
    let sshd = need_sshd!("remove");
    let bind = free_port();
    let (mut e, id) = engine_for(&sshd, vec![fwd(ForwardKind::Local, bind, "127.0.0.1", 9)]);
    let fid = e.store.server(id).unwrap().forwards[0].id;
    e.tunnel_start(id, fid, true).unwrap();
    assert_eq!(wait_tunnel(&mut e, "up").state, "up");
    e.set_forwards(id, vec![]).unwrap();
    assert!(e.tunnels().is_empty());
    assert!(e.store.server(id).unwrap().forwards.is_empty());
}

#[test]
fn bad_tunnels_are_refused() {
    let sshd = need_sshd!("bad");
    let (mut e, id) = engine_for(&sshd, vec![]);
    assert!(e.set_forwards(id, vec![fwd(ForwardKind::Local, 0, "h", 1)]).is_err());
    assert!(e.set_forwards(id, vec![fwd(ForwardKind::Local, 5000, "", 1)]).is_err());
    assert!(e.set_forwards(id, vec![fwd(ForwardKind::Local, 5000, "-oProxyCommand=x", 1)]).is_err());
    assert!(e.set_forwards(id, vec![fwd(ForwardKind::Local, 5000, "a b", 1)]).is_err());
    assert!(e.set_forwards(id, vec![fwd(ForwardKind::Remote, 5000, "db", 0)]).is_err());
    assert!(e.set_forwards(id, vec![fwd(ForwardKind::Dynamic, 5000, "", 0)]).is_ok());
}

fn wait_transfer(e: &mut Engine, id: u64) -> TransferStatus {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let t = e.transfers().into_iter().find(|t| t.id == id).expect("the transfer");
        if t.state != "running" || Instant::now() > end {
            return t;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn files_go_up_and_come_down() {
    let sshd = need_sshd!("files");
    let (mut e, id) = engine_for(&sshd, vec![]);
    let here = sshd.dir.join("here");
    let there = sshd.dir.join("there");
    std::fs::create_dir_all(&here).unwrap();
    std::fs::create_dir_all(&there).unwrap();
    std::fs::write(here.join("a b.txt"), "contents of a\n").unwrap();
    std::fs::create_dir_all(here.join("folder")).unwrap();
    std::fs::write(here.join("folder/inner.txt"), "inner\n").unwrap();

    // up: a file with a space in its name, and a whole folder
    let up = e.transfer_start(id, true, vec![here.join("a b.txt").to_string_lossy().into(), here.join("folder").to_string_lossy().into()], &there.to_string_lossy(), true).unwrap();
    let t = wait_transfer(&mut e, up);
    assert_eq!(t.state, "done", "{t:?}");
    assert_eq!(t.what, "2 files");
    assert_eq!(std::fs::read_to_string(there.join("a b.txt")).unwrap(), "contents of a\n");
    assert_eq!(std::fs::read_to_string(there.join("folder/inner.txt")).unwrap(), "inner\n");

    // down
    let back = sshd.dir.join("back");
    std::fs::create_dir_all(&back).unwrap();
    let down = e.transfer_start(id, false, vec![back.to_string_lossy().into()], &there.join("a b.txt").to_string_lossy(), true).unwrap();
    let t = wait_transfer(&mut e, down);
    assert_eq!(t.state, "done", "{t:?}");
    assert_eq!(std::fs::read_to_string(back.join("a b.txt")).unwrap(), "contents of a\n");

    // a path that is not there fails with a sentence
    let bad = e.transfer_start(id, false, vec![back.to_string_lossy().into()], &there.join("nothing-here").to_string_lossy(), true).unwrap();
    let t = wait_transfer(&mut e, bad);
    assert_eq!(t.state, "failed", "{t:?}");
    assert!(!t.message.is_empty());
    e.transfer_clear(bad);
    assert!(e.transfers().iter().all(|t| t.id != bad));
}

#[test]
fn a_copy_to_nowhere_fails_with_the_reason() {
    let sshd = need_sshd!("nowhere");
    let (mut e, id) = engine_for(&sshd, vec![]);
    let mut s = e.store.server(id).unwrap().clone();
    s.port = free_port();
    e.store.update_server(s);
    let f = sshd.dir.join("x.txt");
    std::fs::write(&f, "x").unwrap();
    let t = e.transfer_start(id, true, vec![f.to_string_lossy().into()], "", true).unwrap();
    let t = wait_transfer(&mut e, t);
    assert_eq!(t.state, "failed");
    assert!(t.message.to_lowercase().contains("refused") || t.message.to_lowercase().contains("connect"), "{}", t.message);
}

/// A real HTTP proxy (`ncat` in listening mode), so the connection really leaves through it.
fn start_http_proxy() -> Option<(Child, u16)> {
    let ncat = which("ncat")?;
    let port = free_port();
    let child = Command::new(ncat).args(["-l", "-k", "127.0.0.1", &port.to_string(), "--proxy-type", "http"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some((child, port));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn a_connection_leaves_through_the_proxy_of_its_server() {
    let sshd = need_sshd!("proxy");
    let Some((mut proxy, proxy_port)) = start_http_proxy() else { return eprintln!("skipped: no ncat here") };
    let target = greeter("hello-through-the-proxy");
    let bind = free_port();
    let (mut e, id) = engine_for(&sshd, vec![fwd(ForwardKind::Local, bind, "127.0.0.1", target)]);
    let px = e.proxy_save(oso_core::store::Proxy { id: 0, name: "test".into(), kind: oso_core::store::ProxyKind::Http, host: "127.0.0.1".into(), port: proxy_port }).unwrap();
    let mut s = e.store.server(id).unwrap().clone();
    s.proxy = Some(px);
    e.save_server(s, None, None).unwrap();
    let fid = e.store.server(id).unwrap().forwards[0].id;
    e.tunnel_start(id, fid, true).unwrap();
    let t = wait_tunnel(&mut e, "up");
    assert_eq!(t.state, "up", "ssh got to sshd through the proxy: {t:?}");
    assert_eq!(read_from(bind), "hello-through-the-proxy");
    e.tunnel_stop(id, fid);
    // with the proxy gone the same server cannot be reached: it really was going through it
    let _ = proxy.kill();
    let _ = proxy.wait();
    std::thread::sleep(Duration::from_millis(300));
    e.tunnel_start(id, fid, true).unwrap();
    let t = wait_tunnel(&mut e, "failed");
    assert_eq!(t.state, "failed", "{t:?}");
}

#[test]
fn an_automatic_tunnel_is_open_in_every_terminal_of_the_server() {
    let sshd = need_sshd!("auto");
    let target = greeter("hello-from-an-auto-tunnel");
    let bind = free_port();
    let mut f = fwd(ForwardKind::Local, bind, "127.0.0.1", target);
    f.auto = true;
    let (e, id) = engine_for(&sshd, vec![f]);
    let argv = oso_core::connect::ssh_argv(&e.store, e.store.server(id).unwrap());
    // run the terminal's own ssh command, with the test's options, without a shell on the other side
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..argv.len() - 1]).args(&e.ssh_extra).args(["-o", "BatchMode=yes", "-N"]).arg(argv.last().unwrap()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    assert_eq!(read_from(bind), "hello-from-an-auto-tunnel");
    let _ = child.kill();
    let _ = child.wait();
}
