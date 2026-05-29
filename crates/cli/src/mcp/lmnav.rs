pub(super) fn problem(uri: &str, tool: &str, message: &str) -> String {
	format!(
		"uri: {uri}\ncompleteness: partial (error)\n\nproblem: {message}\nwhere: {tool}\nfix_hint: retry with a supported URI and bounded arguments\n"
	)
}
