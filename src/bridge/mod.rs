mod protocol;
mod registry;

pub(crate) use protocol::{BridgeLaunchRequest, BridgeLaunchResponse, BridgeWaitResponse};
pub(crate) use registry::{
    bridge_debug, discover_vscode_bridge_endpoints, discover_vscode_bridge_registry_endpoints,
    remove_vscode_bridge_registry_endpoint, BridgeEndpoint, RegistryMatch,
    VSCODE_BRIDGE_BYPASS_ENV,
};
