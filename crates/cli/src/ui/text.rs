#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum FitMode {
	Middle,
	Tail,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum Align {
	Left,
	Right,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct Column {
	pub(super) title: &'static str,
	pub(super) width: usize,
	pub(super) align: Align,
}

impl Column {
	pub(super) const fn left(title: &'static str, width: usize) -> Self {
		Self {
			title,
			width,
			align: Align::Left,
		}
	}

	pub(super) const fn right(title: &'static str, width: usize) -> Self {
		Self {
			title,
			width,
			align: Align::Right,
		}
	}
}

pub(super) fn table_width(columns: &[Column], max_width: usize) -> usize {
	let widths = fitted_widths(columns, max_width);
	widths.iter().sum::<usize>() + gap_width(columns)
}

pub(super) fn fitted_widths(columns: &[Column], max_width: usize) -> Vec<usize> {
	if columns.is_empty() {
		return Vec::new();
	}
	let gaps = gap_width(columns);
	let available = max_width.saturating_sub(gaps);
	let requested: Vec<_> = columns.iter().map(|column| column.width).collect();
	if requested.iter().sum::<usize>() <= available {
		return requested;
	}
	let mut widths = vec![0; columns.len()];
	let mut remaining = available;
	while remaining > 0
		&& widths
			.iter()
			.zip(&requested)
			.any(|(width, max)| width < max)
	{
		for (width, max) in widths.iter_mut().zip(&requested) {
			if remaining == 0 {
				break;
			}
			if *width < *max {
				*width += 1;
				remaining -= 1;
			}
		}
	}
	widths
}

fn gap_width(columns: &[Column]) -> usize {
	columns.len().saturating_sub(1) * 2
}

pub(super) fn format_cell(value: &str, width: usize, align: Align) -> String {
	let value = fit_text(value, width, FitMode::Tail);
	match align {
		Align::Left => format!("{value:<width$}"),
		Align::Right => format!("{value:>width$}"),
	}
}

pub(super) fn fit_text(value: &str, width: usize, mode: FitMode) -> String {
	if visible_len(value) <= width {
		return value.to_string();
	}
	match mode {
		FitMode::Middle => fit_middle(value, width),
		FitMode::Tail => fit_tail(value, width),
	}
}

fn fit_middle(value: &str, width: usize) -> String {
	if width == 0 {
		return String::new();
	}
	if width <= 3 {
		return ".".repeat(width);
	}
	let available = width - 3;
	let left = available / 2;
	let right = available - left;
	format!("{}...{}", take_start(value, left), take_end(value, right))
}

fn fit_tail(value: &str, width: usize) -> String {
	if width == 0 {
		return String::new();
	}
	if width <= 3 {
		return ".".repeat(width);
	}
	format!("...{}", take_end(value, width - 3))
}

fn take_start(value: &str, count: usize) -> String {
	value.chars().take(count).collect()
}

fn take_end(value: &str, count: usize) -> String {
	let chars: Vec<_> = value.chars().collect();
	chars[chars.len().saturating_sub(count)..].iter().collect()
}

pub(super) fn visible_len(value: &str) -> usize {
	value.chars().count()
}
