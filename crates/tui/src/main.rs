//! `hernees` — terminal UI for the hernees coding agent.

mod app;
mod backend;
mod catalog;
mod client;
mod clipboard;
mod commands;
mod dialog;
mod input;
mod keys;
mod render;
mod screen;
mod store;
mod text;
mod theme;
mod ui;

use app::{Action, App, Mode, StartOpts};
use clap::Parser;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture, Event as CEvent, EventStream, KeyEventKind, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "hernees",
    version,
    about = "hernees coding agent (terminal UI)",
    after_help = "Without --server, hernees connects to a running backend found in the instance \
registry ($XDG_STATE_HOME/hernees/instances) or starts `hernees-server` (next to this binary, \
in PATH, or $HERNEES_SERVER) with --idle-timeout 0, so the backend exits when the last frontend \
closes. Sessions are persisted and can be resumed with --session / --continue.\n\n\
Press ctrl+p for commands and ctrl+x h for key bindings."
)]
struct Cli {
    /// Prompt to send as soon as the session is ready
    prompt: Option<String>,
    /// Resume (attach to) a session by id
    #[arg(short, long, value_name = "ID", conflicts_with = "background")]
    session: Option<String>,
    /// Continue the most recently updated session of the current directory
    #[arg(short = 'c', long = "continue", conflicts_with_all = ["session", "background"])]
    cont: bool,
    /// Agent for a new session [default: `default_agent` from the config, else "build"]
    #[arg(long, value_name = "NAME")]
    agent: Option<String>,
    /// Model for a new session, as provider/model[#variant] (e.g. openai/gpt-5#high)
    /// [default: the agent's model, else `model` from the config]
    #[arg(short, long, value_name = "MODEL")]
    model: Option<String>,
    /// Connect to this backend (ws://host:port or host:port) instead of discovering or
    /// starting one; reconnects to the same URL if the connection drops
    #[arg(long, value_name = "URL")]
    server: Option<String>,
    /// Background monitor: tile every open background session (sessions no frontend has in
    /// the foreground) without creating one; panes appear and disappear automatically
    #[arg(short, long)]
    background: bool,
}

type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

fn setup_terminal(mouse: bool) -> std::io::Result<Term> {
    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    execute!(out, EnterAlternateScreen, EnableBracketedPaste, EnableFocusChange)?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
    }
    // Disambiguate shift+enter etc. where supported (kitty protocol); harmless elsewhere.
    let _ = execute!(
        out,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        )
    );
    let mut t = Terminal::new(CrosstermBackend::new(out))?;
    t.clear()?;
    Ok(t)
}

fn restore_terminal() {
    let mut out = std::io::stdout();
    let _ = execute!(out, PopKeyboardEnhancementFlags);
    let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, DisableFocusChange, LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    let _ = execute!(out, crossterm::cursor::Show);
    let _ = out.flush();
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        prev(info);
        // A panic on any thread (including the terminal reader) leaves the UI unusable:
        // the terminal is already restored, so exit instead of running headless.
        std::process::exit(101);
    }));
}

fn main() {
    let cli = Cli::parse();
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("hernees: cannot start runtime: {e}");
            std::process::exit(1);
        }
    };
    let code = rt.block_on(async {
        match run(cli).await {
            Ok(()) => 0,
            Err(e) => {
                restore_terminal();
                eprintln!("hernees: {e:#}");
                1
            }
        }
    });
    rt.shutdown_timeout(Duration::from_millis(200));
    std::process::exit(code);
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let model = match &cli.model {
        Some(m) => Some(hn_proto::ModelRef::parse(m).ok_or_else(|| anyhow::anyhow!("--model must be provider/model[#variant]"))?),
        None => None,
    };
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| ".".into());
    eprint!("\x1b[2mconnecting to backend…\x1b[0m\r");
    let be = backend::connect(cli.server.as_deref(), &cwd).await?;
    eprint!("\x1b[2K\r");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let base = be.url.clone();
    let client = client::spawn(base.clone(), cwd.clone(), Some(be.ws), cli.server.is_none(), tx);
    let mode = if cli.background { Mode::Monitor } else { Mode::Normal };
    let start = StartOpts { prompt: cli.prompt, session: cli.session, cont: cli.cont, agent: cli.agent, model };
    let mut app = App::new(client, cwd, base, mode, start);
    if be.spawned {
        app.toast(hn_proto::ToastLevel::Info, "Started a new backend");
    }

    install_panic_hook();
    let mut term = setup_terminal(true)?;
    let mut events = EventStream::new();
    let res = event_loop(&mut app, &mut term, &mut events, &mut rx).await;
    drop(events);
    restore_terminal();
    res
}

const FRAME: Duration = Duration::from_millis(33);

async fn event_loop(
    app: &mut App,
    term: &mut Term,
    events: &mut EventStream,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<client::Incoming<app::Tag>>,
) -> anyhow::Result<()> {
    let mut dirty = true;
    let mut last_draw = Instant::now() - FRAME;
    loop {
        if dirty {
            // Coalesce bursts (streaming deltas) to at most ~30fps.
            let since = last_draw.elapsed();
            if since < FRAME {
                tokio::time::sleep(FRAME - since).await;
                // Drain anything that arrived meanwhile.
                while let Ok(m) = rx.try_recv() {
                    app.on_incoming(m);
                }
            }
            term.draw(|f| screen::draw(app, f))?;
            last_draw = Instant::now();
            dirty = false;
        }
        if app.quit {
            return Ok(());
        }
        // Handle deferred actions that need the terminal.
        for a in std::mem::take(&mut app.actions) {
            match a {
                Action::SetMouse(on) => {
                    let mut out = std::io::stdout();
                    if on {
                        let _ = execute!(out, EnableMouseCapture);
                    } else {
                        let _ = execute!(out, DisableMouseCapture);
                    }
                }
                Action::RunEditor => {
                    run_editor(app, term, events)?;
                }
            }
            dirty = true;
        }
        if dirty {
            continue;
        }
        let deadline = if let Some(iv) = app.anim_interval() {
            Some(last_draw + iv)
        } else {
            app.next_timer().map(|t| t.max(Instant::now()))
        };
        let sleep = async {
            match deadline {
                Some(d) => tokio::time::sleep_until(d.into()).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            ev = events.next() => {
                match ev {
                    Some(Ok(ev)) => { handle_event(app, term, ev); dirty = true; }
                    Some(Err(_)) => {}
                    None => return Ok(()),
                }
            }
            m = rx.recv() => {
                match m {
                    Some(m) => { app.on_incoming(m); dirty = true; }
                    None => return Ok(()),
                }
            }
            _ = sleep => {
                app.tick();
                dirty = true;
            }
        }
    }
}

fn handle_event(app: &mut App, term: &mut Term, ev: CEvent) {
    let size = term.size().unwrap_or_default();
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    match ev {
        CEvent::Key(k) if k.kind != KeyEventKind::Release => app.on_key(k, area),
        CEvent::Mouse(m) => app.on_mouse(m),
        CEvent::Paste(s) => app.on_paste(s),
        CEvent::Resize(..) => {
            let _ = term.autoresize();
        }
        _ => {}
    }
}

fn run_editor(app: &mut App, term: &mut Term, events: &mut EventStream) -> anyhow::Result<()> {
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    let path = std::env::temp_dir().join(format!("hernees-prompt-{}.md", std::process::id()));
    std::fs::write(&path, app.active_editor().text())?;
    let _ = events;
    restore_terminal();
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(&path).status();
    *term = setup_terminal(app.mouse_capture)?;
    match status {
        Ok(s) if s.success() => {
            let t = std::fs::read_to_string(&path).unwrap_or_default();
            let t = t.trim_end_matches('\n');
            app.active_editor().set_text(t);
        }
        Ok(_) => app.toast(hn_proto::ToastLevel::Warning, "Editor exited with an error; prompt unchanged"),
        Err(e) => app.toast(hn_proto::ToastLevel::Error, format!("Cannot run {editor}: {e}")),
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}
