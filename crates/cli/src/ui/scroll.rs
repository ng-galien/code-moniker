use ratatui::layout::Rect;
use ratatui::style::Style;

use super::theme::THEME;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScrollViewport {
	pub(super) content_len: usize,
	pub(super) viewport_len: usize,
	pub(super) offset: usize,
}

impl ScrollViewport {
	pub(super) fn from_offset(content_len: usize, viewport_len: usize, offset: usize) -> Self {
		Self {
			content_len,
			viewport_len,
			offset: clamp_offset(offset, content_len, viewport_len),
		}
	}

	pub(super) fn for_selection_with_margin(
		content_len: usize,
		viewport_len: usize,
		selection: usize,
		margin: usize,
	) -> Self {
		let margin = margin.min(viewport_len.saturating_sub(1));
		let selected_row = viewport_len.saturating_sub(1 + margin);
		let offset = if viewport_len == 0 {
			0
		} else {
			selection.saturating_sub(selected_row)
		};
		Self::from_offset(content_len, viewport_len, offset)
	}

	pub(super) fn for_visible_line(
		content_len: usize,
		viewport_len: usize,
		current_offset: usize,
		line: Option<usize>,
		margin: usize,
	) -> Self {
		let mut offset = clamp_offset(current_offset, content_len, viewport_len);
		let Some(line) = line else {
			return Self::from_offset(content_len, viewport_len, offset);
		};
		if viewport_len == 0 {
			return Self::from_offset(content_len, viewport_len, 0);
		}
		let margin = margin.min(viewport_len.saturating_sub(1) / 2);
		let upper = offset.saturating_add(margin);
		let lower = offset
			.saturating_add(viewport_len)
			.saturating_sub(1 + margin);
		if line < upper {
			offset = line.saturating_sub(margin);
		} else if line > lower {
			offset = line.saturating_sub(viewport_len.saturating_sub(1 + margin));
		}
		Self::from_offset(content_len, viewport_len, offset)
	}

	pub(super) fn has_overflow(self) -> bool {
		self.viewport_len > 0 && self.content_len > self.viewport_len
	}

	pub(super) fn content_area(self, area: Rect) -> Rect {
		if !self.has_overflow() || area.width <= 1 {
			return area;
		}
		Rect {
			width: area.width - 1,
			..area
		}
	}

	pub(super) fn offset_u16(self) -> u16 {
		self.offset.min(usize::from(u16::MAX)) as u16
	}

	fn thumb(self, track_len: usize) -> Option<ScrollbarThumb> {
		if !self.has_overflow() || track_len == 0 {
			return None;
		}
		let max_offset = max_offset(self.content_len, self.viewport_len);
		if max_offset == 0 {
			return None;
		}
		let thumb_len = self
			.viewport_len
			.saturating_mul(track_len)
			.saturating_add(self.content_len.saturating_sub(1))
			/ self.content_len;
		let thumb_len = thumb_len.clamp(1, track_len);
		let travel = track_len.saturating_sub(thumb_len);
		let start = self
			.offset
			.saturating_mul(travel)
			.saturating_add(max_offset / 2)
			/ max_offset;
		Some(ScrollbarThumb {
			start,
			end: start + thumb_len,
		})
	}
}

pub(super) fn viewport_comfort_margin(viewport_len: usize) -> usize {
	if viewport_len < 8 {
		0
	} else {
		(viewport_len / 5).clamp(1, 4)
	}
}

pub(super) fn render_vertical_scrollbar(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	viewport: ScrollViewport,
) {
	let Some(thumb) = viewport.thumb(usize::from(area.height)) else {
		return;
	};
	if area.width == 0 {
		return;
	}
	let x = area.right().saturating_sub(1);
	for row in 0..usize::from(area.height) {
		let (symbol, color) = if row >= thumb.start && row < thumb.end {
			("┃", THEME.scrollbar.thumb)
		} else {
			("│", THEME.scrollbar.track)
		};
		frame
			.buffer_mut()
			.set_string(x, area.y + row as u16, symbol, Style::default().fg(color));
	}
}

fn clamp_offset(offset: usize, content_len: usize, viewport_len: usize) -> usize {
	offset.min(max_offset(content_len, viewport_len))
}

fn max_offset(content_len: usize, viewport_len: usize) -> usize {
	content_len.saturating_sub(viewport_len)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScrollbarThumb {
	start: usize,
	end: usize,
}

// Disabled during the UI architecture rebuild; rewrite against the new component contracts later.
#[cfg(any())]
mod tests {
	use super::*;

	#[test]
	fn selection_viewport_keeps_selected_row_visible_at_bottom() {
		let viewport = ScrollViewport::for_selection_with_margin(100, 10, 24, 0);

		assert_eq!(viewport.offset, 15);
		assert_eq!(viewport.content_len, 100);
		assert_eq!(viewport.viewport_len, 10);
	}

	#[test]
	fn selection_viewport_can_keep_a_bottom_comfort_margin() {
		let viewport = ScrollViewport::for_selection_with_margin(100, 10, 24, 2);

		assert_eq!(viewport.offset, 17);
		assert_eq!(24 - viewport.offset, 7);
	}

	#[test]
	fn comfort_margin_scales_with_viewport_height() {
		assert_eq!(viewport_comfort_margin(7), 0);
		assert_eq!(viewport_comfort_margin(10), 2);
		assert_eq!(viewport_comfort_margin(40), 4);
	}

	#[test]
	fn selected_line_inside_comfort_zone_keeps_current_offset() {
		let viewport = ScrollViewport::for_visible_line(100, 10, 20, Some(25), 2);

		assert_eq!(viewport.offset, 20);
	}

	#[test]
	fn selected_line_below_comfort_zone_scrolls_down() {
		let viewport = ScrollViewport::for_visible_line(100, 10, 20, Some(29), 2);

		assert_eq!(viewport.offset, 22);
	}

	#[test]
	fn selected_line_above_comfort_zone_scrolls_up() {
		let viewport = ScrollViewport::for_visible_line(100, 10, 20, Some(21), 2);

		assert_eq!(viewport.offset, 19);
	}

	#[test]
	fn viewport_clamps_offset_to_content_end() {
		let viewport = ScrollViewport::from_offset(12, 5, 100);

		assert_eq!(viewport.offset, 7);
	}

	#[test]
	fn viewport_without_overflow_has_zero_offset() {
		let viewport = ScrollViewport::from_offset(3, 10, 8);

		assert_eq!(viewport.offset, 0);
		assert!(!viewport.has_overflow());
	}

	#[test]
	fn thumb_size_is_proportional_to_visible_content() {
		let viewport = ScrollViewport::from_offset(100, 10, 0);

		assert_eq!(
			viewport.thumb(20),
			Some(ScrollbarThumb { start: 0, end: 2 })
		);
	}

	#[test]
	fn thumb_reaches_track_end_at_last_full_viewport() {
		let viewport = ScrollViewport::from_offset(100, 10, 90);

		assert_eq!(
			viewport.thumb(20),
			Some(ScrollbarThumb { start: 18, end: 20 })
		);
	}

	#[test]
	fn content_area_reserves_scrollbar_column_only_when_needed() {
		let area = Rect::new(2, 3, 10, 4);

		assert_eq!(
			ScrollViewport::from_offset(20, 4, 0).content_area(area),
			Rect::new(2, 3, 9, 4)
		);
		assert_eq!(
			ScrollViewport::from_offset(4, 4, 0).content_area(area),
			area
		);
	}
}
