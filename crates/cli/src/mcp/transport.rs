use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;

use serde_json::{Value, json};

pub(super) const MCP_PATH: &str = "/mcp";
pub(super) const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct HttpRequest {
	pub(super) method: String,
	pub(super) path: String,
	pub(super) origin: Option<String>,
	pub(super) body: Vec<u8>,
}

pub(super) fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<HttpRequest> {
	let mut reader = BufReader::new(stream);
	let mut request_line = String::new();
	reader.read_line(&mut request_line)?;
	let mut parts = request_line.split_whitespace();
	let method = parts
		.next()
		.ok_or_else(|| anyhow::anyhow!("missing HTTP method"))?
		.to_string();
	let path = parts
		.next()
		.ok_or_else(|| anyhow::anyhow!("missing HTTP path"))?
		.to_string();
	let mut content_length = 0usize;
	let mut origin = None;
	loop {
		let mut line = String::new();
		reader.read_line(&mut line)?;
		let header = line.trim_end();
		if header.is_empty() {
			break;
		}
		if let Some((name, value)) = header.split_once(':')
			&& name.eq_ignore_ascii_case("content-length")
		{
			content_length = value.trim().parse()?;
			if content_length > MAX_BODY_BYTES {
				anyhow::bail!("request body exceeds {MAX_BODY_BYTES} bytes");
			}
		}
		if let Some((name, value)) = header.split_once(':')
			&& name.eq_ignore_ascii_case("origin")
		{
			origin = Some(value.trim().to_string());
		}
	}
	let mut body = vec![0; content_length];
	reader.read_exact(&mut body)?;
	Ok(HttpRequest {
		method,
		path,
		origin,
		body,
	})
}

pub(super) fn error_response(error: anyhow::Error) -> String {
	http_json(
		400,
		json!({
			"error": "bad_request",
			"message": error.to_string()
		}),
	)
}

pub(super) fn http_json(status: u16, body: Value) -> String {
	http_response(status, &body.to_string(), None)
}

pub(super) fn http_json_with_origin(status: u16, body: Value, origin: Option<&str>) -> String {
	http_response(status, &body.to_string(), origin)
}

pub(super) fn http_response(status: u16, body: &str, origin: Option<&str>) -> String {
	let reason = match status {
		200 => "OK",
		204 => "No Content",
		403 => "Forbidden",
		400 => "Bad Request",
		404 => "Not Found",
		_ => "OK",
	};
	let cors = origin
		.filter(|origin| is_allowed_origin(origin))
		.map(|origin| {
			format!(
				"Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Headers: content-type, mcp-session-id\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\n"
			)
		})
		.unwrap_or_default();
	format!(
		"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
		body.len()
	)
}

pub(super) fn is_allowed_origin(origin: &str) -> bool {
	let Some(host) = origin
		.strip_prefix("http://")
		.or_else(|| origin.strip_prefix("https://"))
	else {
		return false;
	};
	is_loopback_host_port(host)
}

fn is_loopback_host_port(value: &str) -> bool {
	for host in ["localhost", "127.0.0.1", "[::1]"] {
		if value == host {
			return true;
		}
		if let Some(port) = value
			.strip_prefix(host)
			.and_then(|rest| rest.strip_prefix(':'))
		{
			return !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit());
		}
	}
	false
}
