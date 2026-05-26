use std::io::Write;
use std::time::Instant;

use crossterm::event::{Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::args::UiArgs;
use crate::session::SessionOptions;
use crate::{DEFAULT_SCHEME, Exit};

use super::{EventSource, ShellEvent};
use crate::perf;
use crate::ui::app::{App, AppAction};
use crate::ui::live::StoreEvent;
use crate::ui::render::view;

pub(in crate::ui) fn run<W1: Write, W2: Write>(
	args: &UiArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match run_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W: Write>(args: &UiArgs, stdout: &mut W) -> anyhow::Result<()> {
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let opts = SessionOptions {
		paths: args.paths.clone(),
		project: args.project.clone(),
		cache_dir: args.cache.clone(),
	};
	let app = App::boot(opts, scheme, args.rules.clone(), args.profile.clone());
	run_terminal(stdout, app)
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
	let mut events = EventSource::start(app.store().watch_roots());
	app.set_event_sender(events.sender());
	if let Some(status) = events.status.as_deref() {
		app.set_status(status);
	}
	app.queue_startup_load();
	draw_terminal(terminal, app, "initial")?;
	loop {
		let batch = events.recv_batch()?;
		if handle_app_events(batch, app)? {
			return Ok(());
		}
		if let Some(watch_roots) = app.take_watch_roots_update() {
			if let Some(status) = events.replace_watch_roots(watch_roots) {
				app.append_status(status);
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
	perf::record("draw.view_model", started.elapsed(), app.status());
	let started = Instant::now();
	view::render_shell(frame, frame.area(), &vm);
	perf::record("draw.render_shell", started.elapsed(), app.status());
}

fn handle_app_events(events: Vec<ShellEvent>, app: &mut App) -> anyhow::Result<bool> {
	let mut store_event: Option<StoreEvent> = None;
	for event in events {
		match event {
			ShellEvent::Terminal(Event::Key(key)) if key.kind == KeyEventKind::Press => {
				if app.handle_key(key)? {
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
				if app.update(AppAction::TaskCompleted(result)) {
					return Ok(true);
				}
			}
			ShellEvent::HeaderSearchDebounced(generation) => {
				if app.update(AppAction::HeaderSearchDebounced(generation)) {
					return Ok(true);
				}
			}
			ShellEvent::Clipboard(result) => {
				if app.update(AppAction::Clipboard(result)) {
					return Ok(true);
				}
			}
			ShellEvent::Error(error) => return Err(anyhow::anyhow!(error)),
		}
	}
	if let Some(event) = store_event
		&& app.update(AppAction::Store(event))
	{
		return Ok(true);
	}
	Ok(false)
}
