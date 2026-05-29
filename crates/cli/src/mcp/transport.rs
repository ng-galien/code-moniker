use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;

use serde_json::{Value, json};

pub(super) const MCP_PATH: &str = "/mcp";

pub(super) struct HttpRequest {
	pub(super) method: String,
	pub(super) path: String,
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
		}
	}
	let mut body = vec![0; content_length];
	reader.read_exact(&mut body)?;
	Ok(HttpRequest { method, path, body })
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
	http_response(status, &body.to_string())
}

pub(super) fn http_response(status: u16, body: &str) -> String {
	let reason = match status {
		200 => "OK",
		204 => "No Content",
		400 => "Bad Request",
		404 => "Not Found",
		_ => "OK",
	};
	format!(
		"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type, mcp-session-id\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
		body.len()
	)
}
