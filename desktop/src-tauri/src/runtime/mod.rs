pub(crate) mod capability_catalog;
pub(crate) mod diagnostics;
pub(crate) mod external_skill_route;
pub(crate) mod legacy_proxy;
pub(crate) mod model_discovery;
pub(crate) mod operation;
pub(crate) mod profile;
pub(crate) mod profile_switch;
pub(crate) mod provider;
pub(crate) mod proxy;
pub(crate) mod proxy_lifecycle;
#[cfg(unix)]
pub(crate) mod sandbox_session;
#[cfg(windows)]
#[path = "sandbox_session_win.rs"]
pub(crate) mod sandbox_session;
pub(crate) mod science;
pub(crate) mod settings;
pub(crate) mod skill_install_bridge;
#[cfg(unix)]
pub(crate) mod ssh_bridge;
#[cfg(windows)]
#[path = "ssh_bridge_unsupported.rs"]
pub(crate) mod ssh_bridge;
pub(crate) mod system;
pub(crate) mod transaction;
