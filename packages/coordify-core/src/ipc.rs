use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub id: String,
    pub token: String,
    pub action: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub meta: Value,
    #[serde(default)]
    pub event: Value,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok_for(id: &str) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: None, error: None }
    }
    pub fn ok_with_agent(id: &str, agent_id: &str) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: Some(agent_id.to_string()), error: None }
    }
    pub fn err(id: &str, msg: &str) -> Self {
        Self { id: id.to_string(), ok: false, agent_id: None, error: Some(msg.to_string()) }
    }
}

pub fn decode_request(line: &str) -> serde_json::Result<Request> {
    serde_json::from_str(line)
}

pub fn encode_response(r: &Response) -> String {
    // Response contains only owned strings/bools/options — serialization cannot fail.
    serde_json::to_string(r).expect("Response serialization is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_register_request_with_defaults() {
        let line = r#"{"id":"r1","token":"abc","action":"register","meta":{"task":"auth"}}"#;
        let req = decode_request(line).unwrap();
        assert_eq!(req.action, "register");
        assert_eq!(req.token, "abc");
        assert_eq!(req.agent_id, None);
        assert_eq!(req.meta["task"], "auth");
    }

    #[test]
    fn encodes_response_omits_none_fields() {
        let r = Response::ok_with_agent("r1", "agent-1");
        let line = encode_response(&r);
        assert_eq!(line, r#"{"id":"r1","ok":true,"agent_id":"agent-1"}"#);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn encodes_error_response() {
        let r = Response::err("r2", "bad token");
        let line = encode_response(&r);
        assert!(line.contains(r#""ok":false"#));
        assert!(line.contains(r#""error":"bad token""#));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(decode_request("{not json").is_err());
    }
}
