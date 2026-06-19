// code-moniker: ignore-file[smell-clone-reflex]
// Terminal bootstrapping clones handles/configuration into long-lived shell state.
use std::io::Write;
use std::time::Instant;

use crossterm::event::{Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::DEFAULT_SCHEME;
use crate::args::UiArgs;
use crate::session::SessionOptions;

use super::{EventSource, ShellEvent};
use crate::ui::app::{
	App, AppAction, boot_app, handle_key, queue_startup_load, set_event_sender,
	take_watch_roots_update, update,
};
use crate::ui::perf;
use crate::ui::render::view;
use code_moniker_workspace::live::WorkspaceLiveEvent;

pub(crate) struct UiSession {
	app: App,
}

pub(crate) fn boot(args: &UiArgs) -> UiSession {
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let opts = SessionOptions {
		paths: args.paths.clone(),
		project: args.project.clone(),
		cache_dir: args.cache.clone(),
	};
	let app = boot_app(
		opts.clone(),
		scheme.clone(),
		args.rules.clone(),
		args.profile.clone(),
		args.debug,
		args.live_refresh,
	);
	UiSession { app }
}

pub(crate) fn run_session<W: Write>(stdout: &mut W, session: UiSession) -> anyhow::Result<()> {
	run_terminal(stdout, session.app)
}

fn run_terminal<W: Write>(stdout: &mut W, mut app: App) -> anyhow::Result<()> {
	enable_raw_mode()?;
	if let Err(error) = execute!(stdout, EnterAlternateScreen) {
		let _ = disable_raw_mode();
		return Err(error.into());
	}
	let result = (|| -> anyhow::Result<()> {
		let backend = CrosstermBackend::new(&mut *stdout);
		let mut terminal = Terminal::new(backend)?;
		let result = app_loop(&mut terminal, &mut app);
		let _ = terminal.show_cursor();
		result
	})();
	let _ = disable_raw_mode();
	let _ = execute!(stdout, LeaveAlternateScreen);
	result
}

fn app_loop<W: Write>(
	terminal: &mut Terminal<CrosstermBackend<&mut W>>,
	app: &mut App,
) -> anyhow::Result<()> {
	let mut events = EventSource::start(Vec::new());
	set_event_sender(app, events.sender());
	queue_startup_load(app);
	draw_terminal(terminal, app, "initial")?;
	loop {
		let batch = events.recv_batch()?;
		if handle_app_events(batch, app)? {
			return Ok(());
		}
		if let Some(watch_roots) = take_watch_roots_update(app) {
			if let Some(status) = events.replace_watch_roots(watch_roots) {
				crate::ui::app::append_status(app, status);
			}
		}
		draw_terminal(terminal, app, "after_event")?;
	}
}

fn draw_terminal<W: Write>(
	terminal: &mut Terminal<CrosstermBackend<&mut W>>,
	app: &mut App,
	label: &str,
) -> anyhow::Result<()> {
	let started = Instant::now();
	terminal.draw(|frame| draw_app(frame, app))?;
	perf::record("terminal.draw", started.elapsed(), label);
	Ok(())
}

fn draw_app(frame: &mut ratatui::Frame<'_>, app: &mut App) {
	let started = Instant::now();
	let vm = crate::ui::explorer::view_model(app);
	perf::record(
		"draw.view_model",
		started.elapsed(),
		crate::ui::app::status(app),
	);
	let started = Instant::now();
	view::render_shell(frame, frame.area(), &vm);
	perf::record(
		"draw.render_shell",
		started.elapsed(),
		crate::ui::app::status(app),
	);
}

fn handle_app_events(events: Vec<ShellEvent>, app: &mut App) -> anyhow::Result<bool> {
	let mut store_event: Option<WorkspaceLiveEvent> = None;
	for event in events {
		match event {
			ShellEvent::Terminal(Event::Key(key)) if key.kind == KeyEventKind::Press => {
				if handle_key(app, key)? {
					return Ok(true);
				}
			}
			ShellEvent::Terminal(_) => {}
			ShellEvent::Store(event) => {
				store_event = Some(match store_event {
					Some(current) => current.coalesce(event),
					None => event,
				});
			}
			ShellEvent::TaskCompleted(result) => {
				if update(app, AppAction::TaskCompleted(result)) {
					return Ok(true);
				}
			}
			ShellEvent::HeaderSearchDebounced(generation) => {
				if update(app, AppAction::HeaderSearchDebounced(generation)) {
					return Ok(true);
				}
			}
			ShellEvent::UsageLensDebounced(generation) => {
				if update(app, AppAction::UsageLensDebounced(generation)) {
					return Ok(true);
				}
			}
			ShellEvent::Clipboard(result) => {
				if update(app, AppAction::Clipboard(result)) {
					return Ok(true);
				}
			}
			ShellEvent::Error(error) => return Err(anyhow::anyhow!(error)),
		}
	}
	if let Some(event) = store_event
		&& update(app, AppAction::Store(event))
	{
		return Ok(true);
	}
	Ok(false)
}
