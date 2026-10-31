//! `ref` — TUI frontend for ref-server.

mod app;
mod clipboard;
mod input;
mod mock;
mod net;
mod render;
mod state;
mod ui;

#[cfg(test)]
mod question_tests;
#[cfg(test)]
mod tests;

use std::io::Write;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    EventStream, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ref_proto::ModelRef;
use tokio::sync::mpsc;

use crate::app::{App, InitPlan};
use crate::net::Client;
use ref_client::Target;

#[derive(Parser, Debug)]
#[command(name = "ref", version, about = "ref — coding agent TUI")]
struct Cli {
    /// Directory to open a new session in (default: current directory).
    path: Option<String>,
    /// Attach to an existing session (live or history) by id.
    #[arg(short, long)]
    session: Option<String>,
    /// Continue the most recent session for this directory.
    #[arg(short = 'c', long = "continue")]
    continue_last: bool,
    /// Split view of all live background sessions (does not create a session).
    #[arg(short, long)]
    watch: bool,
    /// Agent for the new session.
    #[arg(long)]
    agent: Option<String>,
    /// Model for the new session (`provider/model[#variant]`).
    #[arg(long)]
    model: Option<String>,
    /// Explicit backend URL (ws://host:port); disables auto-spawn.
    #[arg(long)]
    server: Option<String>,
    /// Token for --server.
    #[arg(long)]
    token: Option<String>,
    /// Initial prompt to send.
    #[arg(short, long)]
    prompt: Option<String>,
    /// Run against a built-in fake backend (development).
    #[arg(long)]
    mock: bool,
}

fn setup_terminal() -> std::io::Result<()> {
    enable_raw_mode()?;
    let mut out = std::io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        cursor::SetCursorStyle::SteadyBar
    )?;
    if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(
            out,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        );
    }
    Ok(())
}

fn restore_terminal() {
    let mut out = std::io::stdout();
    let _ = execute!(out, PopKeyboardEnhancementFlags);
    let _ = execute!(
        out,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        cursor::SetCursorStyle::DefaultUserShape,
        cursor::Show
    );
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// Suspends the TUI, runs $VISUAL/$EDITOR on the prompt text, returns the edited text.
fn run_external_editor(initial: &str) -> Option<String> {
    let default = if cfg!(windows) { "notepad" } else { "vi" };
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| default.into());
    let path = std::env::temp_dir().join(format!(
        "ref-prompt-{}-{}.md",
        std::process::id(),
        ref_proto::now_ms()
    ));
    std::fs::write(&path, initial).ok()?;
    restore_terminal();
    let status = editor_command(&editor, &path).status();
    let _ = setup_terminal();
    let out = std::fs::read_to_string(&path).ok();
    let _ = std::fs::remove_file(&path);
    match status {
        Ok(s) if s.success() => out.map(|s| s.trim_end_matches('\n').to_string()),
        _ => None,
    }
}

/// `$EDITOR` may contain arguments (`code --wait`), so it goes through the shell.
#[cfg(unix)]
fn editor_command(editor: &str, path: &std::path::Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(path);
    cmd
}

#[cfg(windows)]
fn editor_command(editor: &str, path: &std::path::Path) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("cmd");
    cmd.arg("/C")
        .raw_arg(format!("{editor} \"{}\"", path.display()));
    cmd
}

/// The absolute start directory. `dunce` keeps Windows paths free of the `\\?\` prefix, which
/// the server never reports for its own paths.
fn start_dir(path: Option<&str>) -> anyhow::Result<String> {
    let dir = match path {
        Some(p) => dunce::canonicalize(p).map_err(|e| anyhow::anyhow!("{p}: {e}"))?,
        None => std::env::current_dir()?,
    };
    if !dir.is_dir() {
        anyhow::bail!("{} is not a directory", dir.display());
    }
    Ok(dir.to_string_lossy().into_owned())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = start_dir(cli.path.as_deref())?;
    let model = match &cli.model {
        Some(m) => Some(
            ModelRef::parse(m)
                .ok_or_else(|| anyhow::anyhow!("invalid --model {m:?}; expected provider/model"))?,
        ),
        None => None,
    };
    let plan = InitPlan {
        session: cli.session.clone(),
        continue_last: cli.continue_last,
        watch: cli.watch,
        agent: cli.agent.clone(),
        model,
        prompt: cli.prompt.clone(),
    };
    let target = match cli.server.clone() {
        Some(url) => Target::Explicit {
            url,
            token: cli.token.clone(),
        },
        None => Target::Auto,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    // Panic hook: restore the terminal before printing.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));
    setup_terminal()?;
    let res = rt.block_on(run(cli.mock, target, cwd, plan));
    restore_terminal();
    rt.shutdown_timeout(Duration::from_millis(200));
    // Exit screen (logo + how to resume), printed to the normal screen like opencode does.
    if let Ok(Some(text)) = &res {
        let mut out = std::io::stdout();
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
    res.map(|_| ())
}

/// Clears the screen and forces a full repaint (without the cursor-position query that
/// `Terminal::clear` performs, which some PTYs never answer).
fn full_clear(t: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> std::io::Result<()> {
    let (w, h) = crossterm::terminal::size()?;
    t.resize(ratatui::layout::Rect::new(0, 0, w, h))
}

/// Runs the TUI; returns the exit-screen text to print after the terminal is restored.
async fn run(
    mock: bool,
    target: Target,
    cwd: String,
    plan: InitPlan,
) -> anyhow::Result<Option<String>> {
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    if mock {
        tokio::spawn(mock::run(cwd.clone(), out_rx, ev_tx));
    } else {
        tokio::spawn(net::run(target, cwd.clone(), out_rx, ev_tx));
    }
    let mut app = App::new(Client::new(out_tx), cwd, plan);
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    full_clear(&mut terminal)?;
    let mut events = EventStream::new();
    let mut last_draw = std::time::Instant::now() - Duration::from_secs(1);
    loop {
        if app.run_editor {
            app.run_editor = false;
            // Stop the terminal reader thread so it doesn't steal the editor's input.
            drop(events);
            let initial = app.editor.text.clone();
            if let Some(t) = run_external_editor(&initial) {
                app.editor.set_text(&t);
            }
            events = EventStream::new();
            full_clear(&mut terminal)?;
            app.dirty = true;
        }
        if app.force_clear {
            app.force_clear = false;
            full_clear(&mut terminal)?;
            app.dirty = true;
        }
        if app.quit {
            break;
        }
        let interval = app.frame_interval();
        // Animation frames are due relative to the last draw, so a stream of input / network
        // events (each re-creating the sleep below) can never starve the spinners.
        if let Some(iv) = interval
            && last_draw.elapsed() >= iv
        {
            app.dirty = true;
        }
        if app.dirty {
            // Cap at ~60fps.
            let since = last_draw.elapsed();
            if since >= Duration::from_millis(14) {
                // Advance scroll easing once per drawn frame.
                for v in app.views.values_mut() {
                    v.chat.step_anim();
                }
                terminal.draw(|f| render::draw(f, &mut app))?;
                last_draw = std::time::Instant::now();
                app.dirty = false;
                app.load_older();
                let title = app.terminal_title();
                if app.last_title.as_deref() != Some(title.as_str()) {
                    let _ = execute!(std::io::stdout(), crossterm::terminal::SetTitle(&title));
                    app.last_title = Some(title);
                }
            }
        }
        let wait = if app.dirty {
            Duration::from_millis(14).saturating_sub(last_draw.elapsed())
        } else {
            interval
                .map(|iv| {
                    iv.saturating_sub(last_draw.elapsed())
                        .max(Duration::from_millis(1))
                })
                .unwrap_or(Duration::from_secs(3600))
        };
        tokio::select! {
            ev = events.next() => {
                match ev {
                    Some(Ok(e)) => input::handle(&mut app, e),
                    Some(Err(_)) | None => break,
                }
                // Drain any further pending terminal events without redrawing in between.
            }
            ev = ev_rx.recv() => {
                match ev {
                    Some(e) => {
                        app.on_net(e);
                        // Coalesce bursts of frames into one render.
                        let mut n = 0;
                        while n < 512 {
                            match ev_rx.try_recv() {
                                Ok(e) => { app.on_net(e); n += 1; }
                                Err(_) => break,
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep(wait) => {
                if interval.is_some() {
                    app.dirty = true;
                }
            }
        }
        app.housekeeping();
    }
    // Detach politely (server counts frontends; idle-timeout handles the rest).
    if let Some(sid) = app.current.clone() {
        app.client
            .send(None, ref_proto::Request::Detach { session_id: sid });
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    if app.last_title.is_some() {
        let _ = execute!(std::io::stdout(), crossterm::terminal::SetTitle(""));
    }
    Ok(app.epilogue())
}

#[cfg(test)]
mod start_dir_tests {
    use super::start_dir;

    #[test]
    fn start_dir_is_absolute_and_must_be_a_directory() {
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            start_dir(None).unwrap(),
            here.to_string_lossy().into_owned()
        );
        assert_eq!(
            start_dir(Some(".")).unwrap(),
            dunce::canonicalize(&here).unwrap().to_string_lossy()
        );
        assert!(!start_dir(Some(".")).unwrap().starts_with(r"\\?\"));
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        assert!(start_dir(Some(manifest)).is_err(), "a file is no directory");
        assert!(start_dir(Some("/no/such/ref-dir")).is_err());
    }
}
