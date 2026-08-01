use std::collections::{HashMap, HashSet};
use std::io::Write;

use anyhow::{Context as _, ensure};
use serde_json::{Value, json};

use super::process::write_frame;

#[derive(Default)]
pub(super) struct ProtocolState {
	initialize: Option<Vec<u8>>,
	initialize_id: Option<String>,
	initialize_response: Option<Value>,
	initialized: Option<Vec<u8>>,
	pending_client_requests: HashMap<String, Value>,
	pending_worker_requests: HashSet<String>,
}

impl ProtocolState {
	pub(super) fn observe_client_frame(&mut self, frame: &[u8]) {
		let Ok(message) = serde_json::from_slice::<Value>(frame) else {
			return;
		};
		if message.get("method").and_then(Value::as_str) == Some("initialize") {
			self.initialize = Some(frame.to_vec());
			self.initialize_id = request_id_value(&message).map(id_key);
		}
		if message.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
			self.initialized = Some(frame.to_vec());
		}
		if let Some(id) = request_id_value(&message) {
			self.pending_client_requests.insert(id_key(id), id.clone());
		} else if let Some(id) = response_id(&message) {
			self.pending_worker_requests.remove(&id);
		}
	}

	pub(super) fn observe_worker_frame(&mut self, frame: &[u8]) {
		let Ok(message) = serde_json::from_slice::<Value>(frame) else {
			return;
		};
		if let Some(id) = request_id_value(&message) {
			self.pending_worker_requests.insert(id_key(id));
		} else if let Some(id) = response_id(&message) {
			if self.initialize_response.is_none() && self.initialize_id.as_deref() == Some(&id) {
				self.initialize_response = Some(message.clone());
			}
			self.pending_client_requests.remove(&id);
		}
	}

	pub(super) fn can_cut_over(&self) -> bool {
		self.handshake_complete()
			&& self.pending_client_requests.is_empty()
			&& self.pending_worker_requests.is_empty()
	}

	pub(super) fn handshake_complete(&self) -> bool {
		self.initialize.is_some()
			&& self.initialize_response.is_some()
			&& self.initialized.is_some()
	}

	pub(super) fn initialize_frame(&self) -> Option<&[u8]> {
		self.initialize.as_deref()
	}

	pub(super) fn initialize_response(&self) -> Option<&Value> {
		self.initialize_response.as_ref()
	}

	pub(super) fn initialized_frame(&self) -> Option<&[u8]> {
		self.initialized.as_deref()
	}
}

pub(super) fn fail_pending_requests<W: Write>(
	state: &mut ProtocolState,
	client_stdout: &mut W,
) -> anyhow::Result<()> {
	for (_, id) in state.pending_client_requests.drain() {
		let response = serde_json::to_vec(&json!({
			"jsonrpc": "2.0",
			"id": id,
			"error": {
				"code": -32603,
				"message": "Code Moniker MCP worker exited during the request"
			}
		}))?;
		write_frame(client_stdout, &response)?;
	}
	state.pending_worker_requests.clear();
	Ok(())
}

pub(super) fn validate_candidate_initialize(
	active: &Value,
	candidate: &Value,
) -> anyhow::Result<()> {
	for (label, pointer) in [
		("protocol version", "/result/protocolVersion"),
		("server capabilities", "/result/capabilities"),
		("server name", "/result/serverInfo/name"),
	] {
		let active_value = active
			.pointer(pointer)
			.with_context(|| format!("active MCP {label} is unavailable"))?;
		let candidate_value = candidate
			.pointer(pointer)
			.with_context(|| format!("candidate MCP {label} is unavailable"))?;
		ensure!(
			candidate_value == active_value,
			"candidate MCP {label} changed from {active_value} to {candidate_value}"
		);
	}
	Ok(())
}

fn request_id_value(message: &Value) -> Option<&Value> {
	message.get("method")?;
	message.get("id").filter(|id| !id.is_null())
}

fn response_id(message: &Value) -> Option<String> {
	if message.get("method").is_some()
		|| (message.get("result").is_none() && message.get("error").is_none())
	{
		return None;
	}
	message.get("id").filter(|id| !id.is_null()).map(id_key)
}

fn id_key(id: &Value) -> String {
	id.to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn initialize_response(version: &str, list_changed: bool) -> Value {
		json!({
			"jsonrpc": "2.0",
			"id": 1,
			"result": {
				"protocolVersion": version,
				"capabilities": { "tools": { "listChanged": list_changed } },
				"serverInfo": { "name": "code-moniker", "version": "0.6.0" }
			}
		})
	}

	#[test]
	fn protocol_state_waits_for_handshake_and_idle_transport() {
		let mut state = ProtocolState::default();
		state.observe_client_frame(br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
		state.observe_worker_frame(
			serde_json::to_string(&initialize_response("2025-03-26", true))
				.unwrap()
				.as_bytes(),
		);
		state.observe_client_frame(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
		assert!(state.can_cut_over());

		state.observe_client_frame(br#"{"jsonrpc":"2.0","id":"q","method":"tools/list"}"#);
		assert!(!state.can_cut_over());
		state.observe_worker_frame(br#"{"jsonrpc":"2.0","id":"q","result":{"tools":[]}}"#);
		assert!(state.can_cut_over());
	}

	#[test]
	fn protocol_state_tracks_worker_initiated_requests() {
		let mut state = ProtocolState {
			initialize: Some(Vec::new()),
			initialize_response: Some(initialize_response("2025-03-26", true)),
			initialized: Some(Vec::new()),
			..Default::default()
		};
		state
			.observe_worker_frame(br#"{"jsonrpc":"2.0","id":7,"method":"sampling/createMessage"}"#);
		assert!(!state.can_cut_over());
		state.observe_client_frame(br#"{"jsonrpc":"2.0","id":7,"result":{}}"#);
		assert!(state.can_cut_over());
	}

	#[test]
	fn candidate_must_preserve_protocol_and_capabilities() {
		let active = initialize_response("2025-03-26", true);
		assert!(validate_candidate_initialize(&active, &active).is_ok());
		let wrong_protocol = initialize_response("2025-06-18", true);
		assert!(validate_candidate_initialize(&active, &wrong_protocol).is_err());
		let wrong_capabilities = initialize_response("2025-03-26", false);
		assert!(validate_candidate_initialize(&active, &wrong_capabilities).is_err());
	}
}
