//! oso's engine, with no interface in it: the servers and folders (`store`), the encrypted vault, SSH
//! sessions (`session`) and the background server that keeps them alive when the window closes (`daemon`).

pub mod connect;
pub mod daemon;
pub mod engine;
pub mod known_hosts;
pub mod session;
pub mod settings;
pub mod store;
pub mod vault;
