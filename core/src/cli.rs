//! oso's command line. Every command asks the running window (see `control`); there is no command that prints a
//! secret. `oso run` hands the secrets of a Space (or of General) to a program you name, as environment variables.

use crate::control;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;

/// What the command line needs from a terminal (the tests play it).
pub trait Term {
    /// A password, not shown while typed.
    fn password(&mut self, prompt: &str) -> String;
    fn confirm(&mut self, question: &str) -> bool;
    fn out(&mut self, text: &str);
    fn err(&mut self, text: &str);
    /// Runs the program in place of this one. Returns only if it could not start (or, for a test, when it ends).
    fn exec(&mut self, program: &str, args: &[String], env: &[(String, String)]) -> i32;
}

pub struct RealTerm;

impl Term for RealTerm {
    fn password(&mut self, prompt: &str) -> String {
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        // SAFETY: plain termios calls on stdin; the saved state is put back before returning.
        let tty = unsafe { libc::isatty(0) } == 1;
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        let hidden = tty && unsafe { libc::tcgetattr(0, &mut saved) } == 0 && {
            let mut quiet = saved;
            quiet.c_lflag &= !libc::ECHO;
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &quiet) == 0 }
        };
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        if hidden {
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &saved) };
            eprintln!();
        }
        line.trim_end_matches(['\n', '\r']).to_string()
    }

    fn confirm(&mut self, question: &str) -> bool {
        eprint!("{question} [y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        matches!(line.trim().to_lowercase().as_str(), "y" | "yes" | "s" | "si" | "sí")
    }

    fn out(&mut self, text: &str) {
        println!("{text}");
    }

    fn err(&mut self, text: &str) {
        eprintln!("{text}");
    }

    fn exec(&mut self, program: &str, args: &[String], env: &[(String, String)]) -> i32 {
        use std::os::unix::process::CommandExt;
        let e = std::process::Command::new(program).args(args).env_clear().envs(env.iter().cloned()).exec();
        eprintln!("oso run: could not start {program}: {e}");
        127
    }
}

/// The commands of the command line, or `None` when `args` is not one of them (the window starts instead).
pub const COMMANDS: [&str; 11] = ["status", "unlock", "lock", "space", "run", "ssh", "open", "host", "scan", "show", "quit"];

pub fn is_command(args: &[String]) -> bool {
    args.first().is_some_and(|a| COMMANDS.contains(&a.as_str()))
}

const USAGE: &str = "oso commands:
  oso status                         is oso open, the vault locked, which Space
  oso unlock | lock                  the vault (the master password is asked here, not shown)
  oso space [use <name>]             list the Spaces, or work in one (General: the things that are in no Space)
  oso run [-s space] -- cmd...       run a program with the secrets of a Space (and General's) in its environment
  oso ssh <host> [ssh args…]         connect to a host from this terminal (user, key, jump hosts and saved secrets from oso)
  oso open <host>                    open a host in oso's window
  oso host add <name> <address> [--user U] [--port N] [--auth agent|key|password] [--key FILE] [--space S] [--notes TEXT]
  oso scan <dir> [--import]          find .env files with secrets in a folder (and bring them into the vault)";

fn ask(socket: &Path, req: Value, t: &mut dyn Term) -> Option<Value> {
    match control::request(socket, &req) {
        Ok(v) if v["ok"] == true => Some(v),
        Ok(v) => {
            t.err(v["error"].as_str().unwrap_or("oso could not do that"));
            None
        }
        Err(e) => {
            t.err(&format!("{e:#}"));
            None
        }
    }
}

/// Runs a command; the number is the exit code.
pub fn run(args: &[String], socket: &Path, t: &mut dyn Term) -> i32 {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["status"] => {
            let Some(v) = ask(socket, json!({ "cmd": "status" }), t) else { return 1 };
            let vault = match (v["vault_exists"].as_bool(), v["vault_unlocked"].as_bool()) {
                (Some(false), _) => "no vault yet",
                (_, Some(true)) => "unlocked",
                _ => "locked",
            };
            t.out(&format!("oso is open · vault: {vault} · Space: {}", v["space"].as_str().unwrap_or("?")));
            0
        }
        ["unlock"] => {
            let pw = t.password("Master password: ");
            if pw.is_empty() {
                t.err("no password given");
                return 1;
            }
            if ask(socket, json!({ "cmd": "unlock", "master": pw }), t).is_none() {
                return 1;
            }
            t.out("Vault unlocked.");
            0
        }
        ["lock"] => {
            if ask(socket, json!({ "cmd": "lock" }), t).is_none() {
                return 1;
            }
            t.out("Vault locked.");
            0
        }
        ["show"] => {
            if ask(socket, json!({ "cmd": "show" }), t).is_none() {
                return 1;
            }
            0
        }
        ["quit"] => {
            if ask(socket, json!({ "cmd": "quit" }), t).is_none() {
                return 1;
            }
            t.out("oso stopped.");
            0
        }
        ["space"] => {
            let Some(v) = ask(socket, json!({ "cmd": "spaces" }), t) else { return 1 };
            let active = v["active"].as_str().unwrap_or("General");
            for p in v["spaces"].as_array().into_iter().flatten() {
                let name = p["name"].as_str().unwrap_or("");
                t.out(&format!("{} {name}", if name == active { "*" } else { " " }));
            }
            0
        }
        ["space", "use", name] => {
            let Some(v) = ask(socket, json!({ "cmd": "space_use", "space": name }), t) else { return 1 };
            t.out(&format!("Space: {}", v["space"].as_str().unwrap_or(name)));
            // oso does not switch an account by itself: the window asks first
            for c in v["changes"].as_array().into_iter().flatten() {
                t.err(&format!("  this Space asks for the {} account {}, and {} is in use: switch it in oso's window", c["tool"].as_str().unwrap_or(""), c["wanted"].as_str().unwrap_or(""), c["current"].as_str().unwrap_or("none")));
            }
            0
        }
        ["open", host] => {
            let Some(v) = ask(socket, json!({ "cmd": "open", "host": host }), t) else { return 1 };
            t.out(&format!("Opened {} in oso.", v["host"].as_str().unwrap_or(host)));
            0
        }
        ["ssh", host, extra @ ..] => {
            let Some(v) = ask(socket, json!({ "cmd": "connect", "host": host }), t) else { return 1 };
            let mut argv: Vec<String> = v["argv"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)).collect();
            if argv.is_empty() {
                t.err("oso gave no command to run");
                return 1;
            }
            argv.extend(extra.iter().map(|a| a.to_string()));
            let mut env: Vec<(String, String)> = std::env::vars().collect();
            let mut set = |k: &str, val: &str| {
                env.retain(|(name, _)| name != k);
                env.push((k.to_string(), val.to_string()));
            };
            // ssh asks oso (through this same program) for the saved password, passphrase or code of each prompt
            if let (Some(token), Ok(me)) = (v["token"].as_str(), std::env::current_exe()) {
                set("SSH_ASKPASS", &me.to_string_lossy());
                set("SSH_ASKPASS_REQUIRE", "force");
                set(ASKPASS_TOKEN, token);
                set("OSO_ASKPASS_SOCKET", &socket.to_string_lossy());
            }
            // a key kept in the vault is offered by oso's own agent
            if let Some(sock) = v["agent"].as_str() {
                set("SSH_AUTH_SOCK", sock);
            }
            let program = argv.remove(0);
            t.exec(&program, &argv, &env)
        }
        ["run", rest @ ..] => {
            let (profile, command) = match rest {
                ["-s", p, "--", cmd @ ..] | ["--space", p, "--", cmd @ ..] => (Some(*p), cmd),
                ["--", cmd @ ..] => (None, cmd),
                _ => {
                    t.err("usage: oso run [-s space] -- command [arguments…]");
                    return 2;
                }
            };
            if command.is_empty() {
                t.err("usage: oso run [-s space] -- command [arguments…]");
                return 2;
            }
            let Some(v) = ask(socket, json!({ "cmd": "run_env", "space": profile, "command": command }), t) else { return 1 };
            let mut env: Vec<(String, String)> = std::env::vars().collect();
            for pair in v["env"].as_array().into_iter().flatten() {
                if let (Some(k), Some(val)) = (pair[0].as_str(), pair[1].as_str()) {
                    env.retain(|(name, _)| name != k);
                    env.push((k.to_string(), val.to_string()));
                }
            }
            t.exec(command[0], &command[1..].iter().map(|s| s.to_string()).collect::<Vec<_>>(), &env)
        }
        ["host", "add", name, host, flags @ ..] => {
            let mut req = json!({ "cmd": "host_add", "name": name, "host": host });
            let mut it = flags.iter();
            while let Some(f) = it.next() {
                let key = match *f {
                    "--user" => "user",
                    "--port" => "port",
                    "--auth" => "auth",
                    "--key" => "key",
                    "--space" => "space",
                    "--notes" => "notes",
                    other => {
                        t.err(&format!("unknown option {other}\n{USAGE}"));
                        return 2;
                    }
                };
                let Some(value) = it.next() else {
                    t.err(&format!("{f} needs a value"));
                    return 2;
                };
                req[key] = if key == "port" { value.parse::<u64>().map(Value::from).unwrap_or(Value::Null) } else { Value::from(*value) };
            }
            let Some(_) = ask(socket, req, t) else { return 1 };
            t.out(&format!("Added {name}."));
            0
        }
        ["scan", ..] => crate::envscan::cli(&a[1..], socket, t),
        _ => {
            t.err(USAGE);
            2
        }
    }
}

/// The environment variable that tells this program it was started by `ssh` as its `SSH_ASKPASS` helper.
pub const ASKPASS_TOKEN: &str = "OSO_ASKPASS_TOKEN";

/// `ssh` runs this with its question as the only argument and reads the answer from standard output. The saved secret
/// comes from the running oso; a question without one (a host key to confirm, a wrong password) is asked on the
/// terminal itself.
pub fn askpass(question: &str, token: &str, socket: &Path) -> i32 {
    let answer = control::request(socket, &json!({ "cmd": "askpass", "token": token, "prompt": question }))
        .ok()
        .and_then(|v| v["answer"].as_str().map(str::to_string));
    let answer = match answer {
        Some(a) => a,
        None => match ask_on_terminal(question) {
            Some(a) => a,
            None => return 1,
        },
    };
    println!("{answer}");
    0
}

fn ask_on_terminal(question: &str) -> Option<String> {
    use std::os::fd::AsRawFd;
    let tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
    let fd = tty.as_raw_fd();
    let mut out = &tty;
    let _ = write!(out, "{question}");
    let _ = out.flush();
    // what is typed is hidden unless it is a plain yes/no question (a host key)
    let hide = !question.to_lowercase().contains("(yes/no");
    // SAFETY: plain termios calls on the terminal opened above; the saved state is put back before returning.
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    let hidden = hide && unsafe { libc::tcgetattr(fd, &mut saved) } == 0 && {
        let mut quiet = saved;
        quiet.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &quiet) == 0 }
    };
    let mut line = String::new();
    let read = std::io::BufReader::new(&tty).read_line(&mut line);
    if hidden {
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
        let _ = writeln!(out);
    }
    read.ok().filter(|n| *n > 0)?;
    Some(line.trim_end_matches(['\n', '\r']).to_string())
}
