use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeLaunchRequest {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: String,
    pub(crate) wait: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) binary_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeLaunchResponse {
    pub(crate) session_id: String,
    pub(crate) url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeWaitResponse {
    pub(crate) exit_code: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_launch_request_serializes_camel_case_contract() {
        let request = BridgeLaunchRequest {
            program: "dcmview".to_string(),
            args: vec!["scan.dcm".to_string()],
            cwd: "/workspace".to_string(),
            wait: false,
            binary_path: None,
        };

        let value = serde_json::to_value(request).expect("bridge launch request serializes");

        assert_eq!(value["program"], "dcmview");
        assert_eq!(value["args"], serde_json::json!(["scan.dcm"]));
        assert_eq!(value["cwd"], "/workspace");
        assert_eq!(value["wait"], false);
        assert_eq!(value.get("binaryPath"), None);
    }

    #[test]
    fn bridge_launch_request_matches_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../docs/contracts/bridge-protocol.json"))
                .expect("bridge fixture parses");
        let request: BridgeLaunchRequest =
            serde_json::from_value(fixture["launch"]["request"].clone())
                .expect("launch request fixture parses");

        let value = serde_json::to_value(request).expect("bridge launch request serializes");

        assert_eq!(fixture["launch"]["method"], "POST");
        assert_eq!(fixture["launch"]["path"], "/launch");
        assert_eq!(value, fixture["launch"]["request"]);
    }

    #[test]
    fn bridge_responses_parse_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../docs/contracts/bridge-protocol.json"))
                .expect("bridge fixture parses");

        let launch: BridgeLaunchResponse =
            serde_json::from_value(fixture["launch"]["response"].clone())
                .expect("launch response fixture parses");
        let wait: BridgeWaitResponse = serde_json::from_value(fixture["wait"]["response"].clone())
            .expect("wait response fixture parses");

        assert_eq!(launch.session_id, "session-1");
        assert_eq!(launch.url, "http://127.0.0.1:51234");
        assert_eq!(wait.exit_code, Some(0));
    }

    #[test]
    fn bridge_wait_response_deserializes_camel_case_contract() {
        let response: BridgeWaitResponse =
            serde_json::from_str(r#"{"exitCode":7}"#).expect("bridge wait response parses");

        assert_eq!(response.exit_code, Some(7));
    }
}
