mod app;
mod cli;
mod history;
mod model;
mod new_session;
mod park;
mod session;
mod tmux;
mod ui;
mod view_ui;

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use app::{App, RefreshHandle, ViewMode};
use cli::{Cli, Command};
use session::Session;

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::New) => {
            let result = new_session::run_new_session_form()?;
            if let Some(name) = result {
                tmux::switch_to_pane(&name);
            }
        }
        Some(Command::Launch { name, cwd, command, attach, tag }) => {
            let (default_name, default_cwd) = tmux::default_new_session_info();
            let session_name = name.as_deref().unwrap_or(&default_name);
            let session_cwd = cwd.as_deref().unwrap_or(&default_cwd);
            match tmux::create_session(session_name, session_cwd, command.as_deref(), &tag) {
                Ok(name) => {
                    if attach {
                        tmux::switch_to_pane(&name);
                    }
                    eprintln!("Session: {name}");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Resume { id, name, no_attach }) => {
            if let Some(session_id) = id {
                match tmux::resume_session(&session_id, name.as_deref()) {
                    Ok(sess) => {
                        if !no_attach {
                            tmux::switch_to_pane(&sess);
                        }
                        eprintln!("Resumed in session: {sess}");
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                let result = history::run_resume_picker()?;
                if let Some((session_id, sess_name)) = result {
                    match tmux::resume_session(&session_id, Some(&sess_name)) {
                        Ok(sess) => {
                            tmux::switch_to_pane(&sess);
                            eprintln!("Resumed in session: {sess}");
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        Some(Command::Next) => {
            let mut app = App::new();
            app.refresh();
            if let Some(session) = app.sessions.iter().find(|s| s.status == session::SessionStatus::Input) {
                if let Some(target) = &session.pane_target {
                    tmux::switch_to_pane(target);
                }
            }
        }
        Some(Command::Json { tag }) => {
            let mut app = App::new();
            app.refresh();
            println!("{}", app.to_json(&tag));
        }
        Some(Command::Park) => {
            park::park();
        }
        Some(Command::Unpark) => {
            park::unpark();
        }
        Some(Command::View) | None => {
            let start_mode = if matches!(cli.command, Some(Command::View)) {
                ViewMode::View
            } else {
                ViewMode::Table
            };
            run_tui(start_mode)?;
        }
    }

    Ok(())
}

fn run_tui(start_mode: ViewMode) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, start_mode);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

/// Spawn a background thread that runs `discover_sessions` on a 2-second cadence
/// (or sooner when woken). The thread owns `prev_sessions` so the main thread
/// never has to touch any subprocess work.
fn spawn_refresh_worker() -> (mpsc::Receiver<Vec<Session>>, RefreshHandle) {
    let (tx_data, rx_data) = mpsc::channel::<Vec<Session>>();
    let (tx_wake, rx_wake) = mpsc::channel::<()>();

    thread::spawn(move || {
        let mut prev: HashMap<String, Session> = HashMap::new();
        loop {
            let sessions: Vec<Session> = session::discover_sessions(&prev)
                .into_iter()
                .filter(|s| s.tmux_session.is_some())
                .collect();

            prev = sessions
                .iter()
                .map(|s| (s.session_id.clone(), s.clone()))
                .collect();

            if tx_data.send(sessions).is_err() {
                return; // main thread dropped the receiver
            }

            match rx_wake.recv_timeout(Duration::from_secs(2)) {
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // Coalesce burst wakes (e.g. multiple `x` presses).
            while rx_wake.try_recv().is_ok() {}
        }
    });

    (rx_data, RefreshHandle { wake: tx_wake })
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, start_mode: ViewMode) -> io::Result<()> {
    let (rx_data, handle) = spawn_refresh_worker();

    let mut app = App::new();
    app.view_mode = start_mode;
    app.set_refresh_handle(handle);

    // Block briefly for the first snapshot so we don't paint an empty UI.
    if let Ok(initial) = rx_data.recv_timeout(Duration::from_secs(5)) {
        app.ingest_sessions(initial);
    }

    let tick_interval = Duration::from_millis(200);

    loop {
        // Drain pending input first so key repeat collapses into a single draw.
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        if app.should_quit {
            return Ok(());
        }

        // Pull the latest snapshot the worker has produced, discarding stale ones.
        let mut latest: Option<Vec<Session>> = None;
        while let Ok(sessions) = rx_data.try_recv() {
            latest = Some(sessions);
        }
        if let Some(sessions) = latest {
            app.ingest_sessions(sessions);
        }

        if app.view_mode == ViewMode::View {
            view_ui::resolve_zoom(&mut app);
        }
        terminal.draw(|f| {
            match app.view_mode {
                ViewMode::Table => ui::render(f, &app),
                ViewMode::View => view_ui::render(f, &app),
            }
        })?;

        app.advance_tick();

        event::poll(tick_interval)?;
    }
}
