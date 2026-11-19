//! oso's engine, with no interface in it: the servers and folders (`store`), the encrypted vault, SSH
//! sessions (`session`) and the background server that keeps them alive when the window closes (`daemon`).

pub mod accounts;
pub mod agent;
pub mod agents;
pub mod procscan;
pub mod agent_api;
pub mod approvals;
pub mod audit;
pub mod cli;
pub mod cloud;
pub mod connect;
pub mod control;
pub mod daemon;
pub mod engine;
pub mod envscan;
pub mod jobs;
pub mod k8s;
pub mod integration;
pub mod known_hosts;
pub mod lockwatch;
pub mod mcp;
pub mod policy;
pub mod session;
pub mod ssh_import;
pub mod settings;
pub mod spaces;
pub mod sounds;
pub mod store;
pub mod totp;
pub mod transfer;
pub mod vault;

/// Makes it harder for another program of the same user to read oso's memory (where the unlocked vault is): no core dumps and
/// no `ptrace`/`/proc/<pid>/mem` from a process that is not its parent when the system allows it. It is not a wall against a
/// determined program of the same user (see docs/STATUS.md), only a step.
pub fn harden_process() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: prctl with PR_SET_DUMPABLE takes plain integers and has no memory safety requirements.
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        }
    }
}
