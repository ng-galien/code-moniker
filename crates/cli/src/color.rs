use std::io::IsTerminal;

use crate::args::ColorChoice;

pub(crate) fn resolve_color(arg: ColorChoice) -> bool {
	match arg {
		ColorChoice::Always => return true,
		ColorChoice::Never => return false,
		ColorChoice::Auto => {}
	}
	if std::env::var_os("NO_COLOR").is_some() {
		return false;
	}
	if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
		return true;
	}
	if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
		return false;
	}
	if std::env::var("CLICOLOR").is_ok_and(|v| v == "0") {
		return false;
	}
	std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn explicit_color_choice_wins_over_terminal_environment() {
		unsafe { std::env::set_var("NO_COLOR", "1") };
		assert!(resolve_color(ColorChoice::Always));
		unsafe { std::env::remove_var("NO_COLOR") };

		unsafe { std::env::set_var("CLICOLOR_FORCE", "1") };
		assert!(!resolve_color(ColorChoice::Never));
		unsafe { std::env::remove_var("CLICOLOR_FORCE") };
	}
}
