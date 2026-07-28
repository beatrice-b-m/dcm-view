mod client;
mod protocol;
mod registry;

pub(crate) use client::{run_vscode_bridge_client, run_vscode_bridge_launch};
pub(crate) use registry::{discover_vscode_bridge_endpoints, RegistryMatch};
