use code_moniker_query::{Page, QueryCursor, QueryError, WorkspaceGeneration};

#[derive(Debug)]
pub(super) struct Paged<T> {
	pub(super) items: Vec<T>,
	pub(super) total: usize,
	pub(super) next_cursor: Option<QueryCursor>,
}

pub(super) fn page_rows<T>(
	rows: Vec<T>,
	page: Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<Paged<T>, QueryError> {
	validate_page_cursor(&page, generation)?;
	let total = rows.len();
	let start = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0)
		.min(total);
	let end = start.saturating_add(page.limit).min(total);
	let next_cursor = (end < total).then(|| QueryCursor::new(end, generation));
	Ok(Paged {
		items: rows.into_iter().skip(start).take(end - start).collect(),
		total,
		next_cursor,
	})
}

pub(super) fn validate_page_cursor(
	page: &Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<(), QueryError> {
	if let Some(cursor) = page.cursor.as_ref()
		&& cursor.generation != generation
	{
		return Err(QueryError::new(
			"cursor_generation_mismatch",
			"query cursor belongs to a different workspace generation",
		));
	}
	Ok(())
}
