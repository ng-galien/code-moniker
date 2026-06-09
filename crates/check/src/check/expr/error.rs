#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
	#[error("expression `{expr}`: {msg}")]
	BadExpr { expr: String, msg: String },
}
