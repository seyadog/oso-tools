//! oso's engine, with no interface in it: the servers and folders (`store`), the encrypted vault, SSH
//! sessions (`session`) and the background server that keeps them alive when the window closes (`daemon`).

pub mod accounts;
pub mod agent;
pub mod agent_api;
pub mod approvals;
pub mod audit;
pub mod connect;
pub mod daemon;
pub mod engine;
pub mod jobs;
pub mod integration;
pub mod known_hosts;
pub mod mcp;
pub mod policy;
pub mod session;
pub mod ssh_import;
pub mod settings;
pub mod store;
pub mod vault;
