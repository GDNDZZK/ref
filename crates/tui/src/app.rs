//! App state, server-frame handling and actions.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::Color;
use ref_client::BackendTarget;
use ref_proto::{
    ImageData, Message, ModelRef, Request, ServerConfigInfo, ServerFrame, SessionEvent,
    SessionInfo, SessionSnapshot, SessionStatus, StashGroup,
};
use serde_json::Value;

use crate::net::{Client, NetEvent};
use crate::prefs::{BackendSource, TuiPrefs};
use crate::state::{PAGE_SIZE, Queued, RANGE_DEBOUNCE, SessionView};
use crate::text;
pub use crate::ui::autocomplete::{AcKind, Autocomplete};
use crate::ui::autocomplete::{
    DirCache, DirKey, DirListing, ShellCompleteCache, ShellCompleteKey, ShellTokenPos,
    build_dir_items, build_shell_arg_items, has_drive_prefix, has_drive_root_prefix, is_bare_drive,
    last_arg_token, shell_token_context, split_dir_query,
};
use crate::ui::dialog::{
    BackendAction, BackendDialog, CreatorFilter, Dialog, HelpDialog, Item, Kind, ListDialog,
    OPEN_SETTLE,
};
use crate::ui::editor::{Editor, History};
use crate::ui::permission::PermissionView;
use crate::ui::question::QuestionView;
use crate::ui::stash::{StashCommand, StashDialog};
use crate::ui::theme;
use crate::ui::toast::Toasts;
use crate::util::{cmd, placeholder_info};

/// What to do at first connect.
#[derive(Debug, Clone, Default)]
pub struct InitPlan {
    pub session: Option<String>,
    pub continue_last: bool,
    pub watch: bool,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
    pub prompt: Option<String>,
}

/// Configuration for the next lazily-created session while `current == None`.
#[derive(Debug, Clone, Default)]
pub struct DraftConfig {
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Chat,
    Watch,
}

/// What the backend answers to a reply for a question that is no longer open.
const NO_PENDING_QUESTION: &str = ref_proto::NO_PENDING_QUESTION;

#[derive(Debug)]
pub enum PostCreate {
    Prompt(String, Vec<ImageData>),
    Shell(String),
}

#[derive(Debug)]
pub enum Pending {
    Config {
        toast_warnings: bool,
    },
    ReloadConfig,
    LiveList,
    Created {
        post: Option<PostCreate>,
    },
    Attach {
        sid: String,
        fg: bool,
    },
    HistoryDialog {
        creator: Option<String>,
        query: Option<String>,
    },
    HistoryBg,
    SwitchSession {
        query: String,
    },
    Continue,
    /// `count`: messages removed (known when undoing from a given message). The response carries
    /// the undone prompt's text and images (messages here only hold image stubs).
    Undo {
        sid: String,
        count: Option<usize>,
    },
    /// Sequence range `[start_seq, end_seq)` requested for `sid` (view generation `generation`).
    MessagesRange {
        sid: String,
        generation: u64,
        start_seq: usize,
        end_seq: usize,
    },
    /// A session was closed; refresh dialogs / history afterwards.
    Closed {
        name: String,
    },
    /// `/cd` on `sid`.
    Cwd {
        sid: String,
    },
    /// `/cd` validation when `current == None`.
    DraftCd,
    Expand {
        sid: Option<String>,
    },
    Files {
        query: String,
    },
    Dirs {
        key: DirKey,
    },
    Binaries {
        query: String,
    },
    ShellComplete {
        cwd: String,
        line: String,
    },
    /// Generic: toast `ok` on success (if non-empty); errors always toast.
    Simple {
        ok: String,
    },
    Export,
    /// Toast the resulting variant.
    Variant,
    /// Show the `{text}` result in a text dialog.
    Text {
        title: String,
    },
    /// A prompt; on failure its text goes back into the (empty) editor so it isn't lost.
    /// `key`: its `Queued` entry when it was shown as queued.
    Prompt {
        sid: String,
        text: String,
        images: Vec<ImageData>,
        key: Option<u64>,
    },
    /// `RemoveQueued` for the `Queued` entry `key`.
    Unqueue {
        sid: String,
        key: u64,
    },
    /// `McpStatus`; errors (older backend) fall back to the `GetConfig` status lines.
    Mcp,
    /// `GetConfig` used only to refresh MCP status lines (fallback polling).
    ConfigMcp,
    /// `PermissionReply`: drops stale-request errors silently.
    Permission,
    /// `QuestionReply`: closes the dialog, or shows why the answers were refused.
    Question {
        sid: String,
        call_id: String,
    },
    /// `RetryNow` response.
    RetryNow,
    /// Stash group list / mutation response carrying `{ groups }`.
    Groups {
        ok: Option<String>,
    },
    StashAll,
    CreateGroup,
    RestoreGroup,
    RestoreGroupMember,
}

/// Error from a `ServerFrame::Response` with `ok: false`.
#[derive(Debug, Clone)]
struct ReplyError {
    message: String,
    code: Option<String>,
}

impl std::fmt::Display for ReplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<&str> for ReplyError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
            code: None,
        }
    }
}

#[derive(Default)]
pub struct Watch {
    pub focus: Option<String>,
    pub page: usize,
    pub pages: usize,
    pub inputs: HashMap<String, Editor>,
    pub shell_modes: HashSet<String>,
    pub drag_tile: Option<String>,
    /// Tiles at last draw.
    pub tiles: Vec<(String, Rect)>,
    /// Direct-parent filter (`All` / `UserCreated` / `ByCreator`).
    pub filter: CreatorFilter,
    /// Remembered direct parent `(id, name)` when cycling `All -> UserCreated -> ByCreator`.
    pending_parent: Option<(String, String)>,
    /// Clickable rect of the filter pill in the watch status line.
    pub filter_pill_rect: Option<Rect>,
    /// WatchAll state last sent to the server.
    subscribed: bool,
}
#[derive(Clone)]
pub struct Cmd {
    pub name: &'static str,
    aliases: &'static [&'static str],
    pub desc: &'static str,
    pub key: &'static str,
    pub requires_args: bool,
}

pub const COMMANDS: &[Cmd] = &[
    Cmd {
        name: cmd::NEW,
        aliases: &["clear"],
        desc: "New session",
        key: "ctrl+x n",
        requires_args: false,
    },
    Cmd {
        name: cmd::SESSIONS,
        aliases: &["resume", "continue"],
        desc: "Switch session (live + history)",
        key: "ctrl+x l",
        requires_args: false,
    },
    Cmd {
        name: cmd::MODELS,
        aliases: &["model"],
        desc: "Switch model (this session only)",
        key: "ctrl+x m",
        requires_args: false,
    },
    Cmd {
        name: cmd::AGENTS,
        aliases: &["agent"],
        desc: "Switch agent",
        key: "ctrl+x a",
        requires_args: false,
    },
    Cmd {
        name: cmd::VARIANT,
        aliases: &["variants", "effort"],
        desc: "Set or cycle model variant",
        key: "ctrl+t",
        requires_args: false,
    },
    Cmd {
        name: cmd::COMPACT,
        aliases: &["summarize"],
        desc: "Compact the conversation",
        key: "ctrl+x c",
        requires_args: false,
    },
    Cmd {
        name: cmd::UNDO,
        aliases: &[],
        desc: "Undo last prompt",
        key: "ctrl+x u",
        requires_args: false,
    },
    Cmd {
        name: cmd::RENAME,
        aliases: &[],
        desc: "Rename the session",
        key: "",
        requires_args: true,
    },
    Cmd {
        name: cmd::CD,
        aliases: &["cwd"],
        desc: "Change working directory",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::DETACH,
        aliases: &["background", "leave"],
        desc: "Detach from current session",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::RETRY,
        aliases: &[],
        desc: "Retry immediately (skip backoff)",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::STASH,
        aliases: &[],
        desc: "Stash all live sessions into a group",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::STASHED,
        aliases: &["groups", "stash-groups"],
        desc: "Manage stashed session groups",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::BACKEND,
        aliases: &["server"],
        desc: "Switch backend connection",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::CLOSE,
        aliases: &[],
        desc: "Close this session",
        key: "ctrl+x k",
        requires_args: false,
    },
    Cmd {
        name: cmd::CLOSE_SESSION,
        aliases: &["kill"],
        desc: "Close live sessions (picker, or by name / id)",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::PEERS,
        aliases: &["list-agents"],
        desc: "List other live sessions you can message",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::PEERS_NAME,
        aliases: &[],
        desc: "Show or set this session's peer name",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::PEERS_INBOX,
        aliases: &[],
        desc: "Messages received from other sessions",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::PEERS_OUTBOX,
        aliases: &[],
        desc: "Messages sent to other sessions and their status",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::EXPORT,
        aliases: &[],
        desc: "Export transcript as markdown",
        key: "ctrl+x x",
        requires_args: false,
    },
    Cmd {
        name: cmd::COPY,
        aliases: &[],
        desc: "Copy last assistant message (raw)",
        key: "ctrl+x y",
        requires_args: false,
    },
    Cmd {
        name: cmd::THINKING,
        aliases: &[],
        desc: "Toggle expanded thinking",
        key: "ctrl+x t",
        requires_args: false,
    },
    Cmd {
        name: cmd::DETAILS,
        aliases: &[],
        desc: "Toggle expanded tool details",
        key: "ctrl+x d",
        requires_args: false,
    },
    Cmd {
        name: cmd::WATCH,
        aliases: &["split"],
        desc: "Split view of background sessions",
        key: "ctrl+x w",
        requires_args: false,
    },
    Cmd {
        name: cmd::SIDEBAR,
        aliases: &[],
        desc: "Toggle sidebar",
        key: "ctrl+x b",
        requires_args: false,
    },
    Cmd {
        name: cmd::MCP,
        aliases: &["mcps"],
        desc: "MCP servers: status, errors and tools",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::RELOAD,
        aliases: &["reload-config"],
        desc: "Reload configuration",
        key: "",
        requires_args: false,
    },
    Cmd {
        name: cmd::EDITOR,
        aliases: &[],
        desc: "Compose prompt in $EDITOR",
        key: "ctrl+x e",
        requires_args: false,
    },
    Cmd {
        name: cmd::HELP,
        aliases: &[],
        desc: "Keybindings and commands",
        key: "ctrl+x h",
        requires_args: false,
    },
    Cmd {
        name: cmd::EXIT,
        aliases: &["quit", "q"],
        desc: "Exit",
        key: "ctrl+x q",
        requires_args: false,
    },
];

pub fn find_cmd(name: &str) -> Option<&'static Cmd> {
    COMMANDS.iter().find(|c| {
        c.name.eq_ignore_ascii_case(name) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })
}

fn find_custom_cmd<'a>(
    cfg: Option<&'a ServerConfigInfo>,
    name: &str,
) -> Option<&'a ref_proto::CommandInfo> {
    cfg?.commands
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
}

pub struct App {
    pub client: Client,
    pub cwd: String,
    next_id: u64,
    pub pending: HashMap<u64, Pending>,
    pub config: Option<ServerConfigInfo>,
    pub views: HashMap<String, SessionView>,
    pub current: Option<String>,
    pub live: Vec<SessionInfo>,
    pub history_list: Vec<SessionInfo>,
    pub mode: Mode,
    pub editor: Editor,
    pub history: History,
    history_warned: bool,
    pub dialog: Option<Dialog>,
    pub perm: Option<PermissionView>,
    pub question: Option<QuestionView>,
    pub toasts: Toasts,
    pub details: bool,
    pub thinking: bool,
    pub start: Instant,
    pub leader: Option<Instant>,
    pub esc_armed: Option<Instant>,
    pub ctrlc_armed: Option<Instant>,
    /// Status-line hint: (text, colour, expires at).
    pub hint: Option<(String, Color, Instant)>,
    /// Pending "press again" confirmation to close a session: (session id, armed at).
    close_armed: Option<(String, Instant)>,
    /// Pending "click again" confirmation to undo from a message of a busy session.
    undo_armed: Option<(String, Instant)>,
    /// Terminal title last sent (main loop only sends changes).
    pub last_title: Option<String>,
    pub connected: bool,
    pub ever_connected: bool,
    pub net_status: Option<String>,
    pub refused: Option<String>,
    pub backend_too_old: Option<u32>,
    pub quit: bool,
    pub ac: Option<Autocomplete>,
    pub watch: Watch,
    pub sidebar: bool,
    pub draft: DraftConfig,
    init: Option<InitPlan>,
    pub shell_mode: bool,
    pub run_editor: bool,
    pub dirty: bool,
    pub last_click: Option<(Instant, u16, u16, u8)>,
    pub last_mouse_pos: Option<(u16, u16)>,
    /// Session id whose transcript holds the active selection.
    pub sel_owner: Option<String>,
    files_inflight: bool,
    files_want: Option<String>,
    /// Debounced `@` file query: (fire at, query).
    pub files_due: Option<(Instant, String)>,
    dir_cache: DirCache,
    pub(crate) dirs_inflight: HashSet<(String, String, bool)>,
    bin_cache: HashMap<String, Vec<String>>,
    bins_inflight: HashSet<String>,
    shell_complete_cache: ShellCompleteCache,
    shell_complete_inflight: HashSet<(String, String)>,
    /// Debounced `CompleteShell` query: `(fire_at, cwd, line)`.
    pub shell_complete_due: Option<(Instant, String, String)>,
    shell_complete_errors: HashSet<(String, String)>,
    history_loaded: bool,
    history_inflight: bool,
    /// Latest server-filtered history response for the open `Kind::Sessions` dialog:
    /// `(creator, query, sessions)`. Kept separate from `history_list` so filtered subsets
    /// never erase unfiltered history or parent lookup entries.
    history_filtered: Option<(Option<String>, Option<String>, Vec<SessionInfo>)>,
    /// Debounced `Kind::Sessions` filter query deadline.
    pub sessions_due: Option<Instant>,
    /// Sessions we've asked to attach (watch) and are waiting on.
    attaching: HashSet<String>,
    pub screen: Rect,
    /// Sidebar live-session rows at last draw: (y, session id).
    pub sidebar_rows: Vec<(u16, String)>,
    pub sidebar_rect: Rect,
    /// Sidebar scroll offset (rows) and its maximum at last draw.
    pub sidebar_scroll: usize,
    pub sidebar_max_scroll: usize,
    /// Full terminal repaint requested (ctrl+l).
    pub force_clear: bool,
    /// Command waiting for config / session snapshot: (target_sid, name, args, requested at).
    deferred: Option<(Option<String>, String, String, Instant)>,
    /// Autocomplete dismissed with esc until the next edit.
    pub suppress_ac: bool,
    shell_tab_requested: bool,
    /// MCP servers (sorted by name).
    pub mcp: Vec<crate::ui::mcp::McpEntry>,
    /// An MCP status answer (or fallback) has arrived since connecting.
    pub mcp_loaded: bool,
    /// The backend answered `McpStatus` with an error: use `GetConfig` status lines instead.
    pub mcp_fallback: bool,
    mcp_inflight: bool,
    /// Next status poll while some server is still connecting.
    pub mcp_poll_at: Option<Instant>,
    /// Sidebar MCP rows at last draw: (y, server name).
    pub sidebar_mcp_rows: Vec<(u16, String)>,
    /// Sidebar visibility at the last housekeeping pass (refresh MCP when it appears).
    sidebar_was_shown: bool,
    /// Stash groups synced from the server.
    pub groups: Vec<StashGroup>,
    pub groups_loaded: bool,
    /// Active backend target and how it was selected.
    pub backend_target: BackendTarget,
    pub backend_source: BackendSource,
    /// Clickable `"Switch Backend..."` regions on banners / connect screen.
    pub switch_backend_rects: Vec<Rect>,
}
/// Poll interval for MCP status while any server is connecting.
const MCP_POLL: Duration = Duration::from_secs(5);
/// Redraw interval while something animates (spinners, pulsing dots).
const TICK: Duration = Duration::from_millis(80);
/// Redraw interval during fast animations (scrolling, toasts sliding, dialogs opening).
const FAST_FRAME: Duration = Duration::from_millis(16);
/// Poll interval while a deferred command waits for its data.
const DEFERRED_POLL: Duration = Duration::from_millis(50);
/// Shortest wait before a timer deadline is re-checked.
const MIN_DEADLINE_WAIT: Duration = Duration::from_millis(5);
/// Redraw interval while only a timer (hint, leader, armed key) is pending.
const IDLE_TIMER_FRAME: Duration = Duration::from_millis(250);
/// How long a command waits for the backend data it needs before it is dropped.
const DEFERRED_COMMAND_TTL: Duration = Duration::from_secs(10);
/// How long the leader key (ctrl+x) waits for its second key.
const LEADER_TIMEOUT: Duration = Duration::from_secs(2);
/// "Press again" window of the armed keys (esc to interrupt, ctrl+c to exit).
pub const ARMED_KEY_TIMEOUT: Duration = Duration::from_millis(1500);
/// Toast when an action needs the backend while it is unreachable.
const NOT_CONNECTED: &str = "Not connected to the backend";
/// Marks the selected row of a dialog.
const SELECTED_GLYPH: &str = "●";
/// Row-key prefixes of the dialogs: a live session, a history session, a server command.
const LIVE_KEY_PREFIX: &str = "live:";
const HISTORY_KEY_PREFIX: &str = "hist:";
const SERVER_COMMAND_KEY_PREFIX: &str = "srv:";
/// How long a hint stays in the footer.
const HINT_DURATION: Duration = Duration::from_millis(1500);
/// How long the second press of close / undo confirmation is accepted.
const CONFIRM_WINDOW: Duration = Duration::from_secs(4);
/// Debounce before an `@` file query is sent.
const FILE_QUERY_DEBOUNCE: Duration = Duration::from_millis(80);
/// Debounce before a `CompleteShell` query is sent while typing.
const SHELL_COMPLETE_DEBOUNCE: Duration = Duration::from_millis(60);
/// Debounce before a `Kind::Sessions` filter query requests filtered history.
const HISTORY_QUERY_DEBOUNCE: Duration = Duration::from_millis(80);
/// `ListHistory.creator` value meaning "sessions without a creator (user-created)".
const CREATOR_USER: &str = "user";
/// Rows of one history page request.
const HISTORY_LIST_LIMIT: usize = 200;
const FILE_QUERY_LIMIT: usize = 20;
const MAX_COMPLETIONS: usize = 50;
const SHELL_HISTORY_MAX: usize = 5;
/// Slash-command typo thresholds: words of at least `TYPO_LONG_WORD_CHARS` allow edit distance 2.
const TYPO_LONG_WORD_CHARS: usize = 4;
const TYPO_MAX_DIST_SHORT: usize = 1;
const TYPO_MAX_DIST_LONG: usize = 2;
const TOAST_ERROR_MAX_CHARS: usize = 200;
const SESSION_DESC_MAX_CHARS: usize = 40;
const TITLE_MAX_CHARS: usize = 80;
/// Extra score of a canonical command name over its aliases.
const CANONICAL_NAME_SCORE_BONUS: i64 = 5;
/// Letters of a logo line printed in the primary colour; the rest is muted.
pub(crate) const LOGO_PRIMARY_CHARS: usize = 10;
const EPILOGUE_LABEL_WIDTH: usize = 10;

pub(crate) fn load_prompt_history_from(
    dir: Result<std::path::PathBuf, String>,
    toasts: &mut Toasts,
) -> History {
    match dir {
        Ok(d) => {
            let path = d.join("prompt-history.jsonl");
            match History::load(path.clone()) {
                Ok(h) => h,
                Err(e) => {
                    toasts.warn(format!("Cannot load prompt history: {e}"));
                    History::with_path(path)
                }
            }
        }
        Err(e) => {
            toasts.warn(format!("Cannot locate state dir for prompt history: {e}"));
            History::default()
        }
    }
}

impl App {
    pub fn new(client: Client, cwd: String, init: InitPlan) -> Self {
        #[cfg(test)]
        let (toasts, history) = (Toasts::default(), History::default());
        #[cfg(not(test))]
        let (toasts, history) = {
            let mut toasts = Toasts::default();
            let history = load_prompt_history_from(ref_proto::state_dir(), &mut toasts);
            (toasts, history)
        };
        let mode = if init.watch { Mode::Watch } else { Mode::Chat };
        let draft = DraftConfig {
            agent: init.agent.clone(),
            variant: init.model.as_ref().and_then(|m| m.variant.clone()),
            model: init.model.clone(),
        };
        App {
            client,
            cwd,
            next_id: 1,
            pending: HashMap::new(),
            config: None,
            views: HashMap::new(),
            current: None,
            live: Vec::new(),
            history_list: Vec::new(),
            mode,
            editor: Editor::default(),
            history,
            history_warned: false,
            dialog: None,
            perm: None,
            question: None,
            details: false,
            toasts,
            thinking: false,
            start: Instant::now(),
            leader: None,
            esc_armed: None,
            ctrlc_armed: None,
            hint: None,
            close_armed: None,
            undo_armed: None,
            last_title: None,
            connected: false,
            ever_connected: false,
            net_status: None,
            refused: None,
            backend_too_old: None,
            quit: false,
            ac: None,
            watch: Watch::default(),
            sidebar: true,
            draft,
            init: Some(init),
            shell_mode: false,
            run_editor: false,
            dirty: true,
            last_click: None,
            last_mouse_pos: None,
            sel_owner: None,
            files_inflight: false,
            files_want: None,
            files_due: None,
            dir_cache: DirCache::default(),
            dirs_inflight: HashSet::new(),
            bin_cache: HashMap::new(),
            bins_inflight: HashSet::new(),
            shell_complete_cache: ShellCompleteCache::default(),
            shell_complete_inflight: HashSet::new(),
            shell_complete_due: None,
            shell_complete_errors: HashSet::new(),
            history_loaded: false,
            history_inflight: false,
            history_filtered: None,
            sessions_due: None,
            attaching: HashSet::new(),
            screen: Rect::default(),
            sidebar_rows: Vec::new(),
            sidebar_rect: Rect::default(),
            sidebar_scroll: 0,
            sidebar_max_scroll: 0,
            force_clear: false,
            suppress_ac: false,
            shell_tab_requested: false,
            deferred: None,
            mcp: Vec::new(),
            mcp_loaded: false,
            mcp_fallback: false,
            mcp_inflight: false,
            mcp_poll_at: None,
            sidebar_mcp_rows: Vec::new(),
            sidebar_was_shown: false,
            groups: Vec::new(),
            groups_loaded: false,
            backend_target: BackendTarget::Auto,
            backend_source: BackendSource::Auto,
            switch_backend_rects: Vec::new(),
        }
    }

    pub fn with_backend(mut self, target: BackendTarget, source: BackendSource) -> Self {
        self.backend_target = target;
        self.backend_source = source;
        self
    }

    pub fn tick(&self) -> u64 {
        (self.start.elapsed().as_millis() / 80) as u64
    }

    pub fn req(&mut self, req: Request, p: Pending) {
        if !self.connected {
            // Requests can't be answered while disconnected; user actions get feedback.
            match p {
                Pending::Files { .. } => self.files_inflight = false,
                Pending::Dirs { key } => {
                    self.dirs_inflight
                        .remove(&(key.cwd, key.path, key.include_files));
                }
                Pending::Binaries { query } => {
                    self.bins_inflight.remove(&query);
                }
                Pending::ShellComplete { cwd, line } => {
                    self.shell_complete_inflight.remove(&(cwd, line));
                }
                Pending::HistoryBg => self.history_inflight = self.has_history_inflight(),
                Pending::HistoryDialog { .. } => {
                    self.history_inflight = self.has_history_inflight();
                    self.toasts.warn(NOT_CONNECTED);
                }
                _ => self.toasts.warn(NOT_CONNECTED),
            }
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        if self.client.send(Some(id), req) {
            self.pending.insert(id, p);
        }
    }

    /// Asks the backend to attach `sid` and marks it as attaching until the answer arrives.
    fn send_attach(&mut self, sid: String, fg: bool) {
        self.attaching.insert(sid.clone());
        self.req(
            Request::Attach {
                session_id: sid.clone(),
                foreground: fg,
                limit: Some(PAGE_SIZE as u32),
            },
            Pending::Attach { sid, fg },
        );
    }
    /// Fire-and-forget request; errors are toasted.
    pub fn send(&mut self, req: Request) {
        self.req(req, Pending::Simple { ok: String::new() });
    }

    pub fn set_hint(&mut self, s: impl Into<String>, c: Color) {
        self.set_hint_for(s, c, HINT_DURATION);
    }

    /// Hint shown in the status line until `dur` has passed.
    fn set_hint_for(&mut self, s: impl Into<String>, c: Color, dur: Duration) {
        self.hint = Some((s.into(), c, Instant::now() + dur));
    }

    pub fn cur(&self) -> Option<&SessionView> {
        self.current.as_ref().and_then(|id| self.views.get(id))
    }

    pub fn cur_mut(&mut self) -> Option<&mut SessionView> {
        let id = self.current.clone()?;
        self.views.get_mut(&id)
    }

    /// Active target session: the focused split-view tile in Watch mode, else `current`.
    pub fn active_target_session(&self) -> Option<String> {
        match self.mode {
            Mode::Watch => self.watch.focus.clone().or_else(|| self.current.clone()),
            Mode::Chat => self.current.clone(),
        }
    }

    pub fn clear_autocomplete(&mut self) {
        self.ac = None;
        self.suppress_ac = false;
        self.shell_tab_requested = false;
        self.files_due = None;
        self.shell_complete_due = None;
    }

    pub fn dismiss_autocomplete(&mut self) {
        self.ac = None;
        self.suppress_ac = true;
        self.shell_tab_requested = false;
        self.shell_complete_due = None;
    }

    pub fn clear_shell_complete_cache(&mut self) {
        self.shell_complete_cache.clear();
        self.shell_complete_errors.clear();
    }

    pub fn exit_shell_mode(&mut self) {
        self.shell_mode = false;
        self.clear_autocomplete();
        self.clear_shell_complete_cache();
    }

    pub fn exit_watch_shell_mode(&mut self, tile: &str) -> bool {
        let removed = self.watch.shell_modes.remove(tile);
        if removed {
            self.clear_autocomplete();
            self.clear_shell_complete_cache();
        }
        removed
    }

    pub fn set_watch_focus(&mut self, focus: Option<String>) {
        if self.watch.focus != focus {
            self.watch.focus = focus;
            self.clear_autocomplete();
        }
    }
    pub fn active_shell_mode(&self) -> bool {
        match self.mode {
            Mode::Chat => self.shell_mode,
            Mode::Watch => self
                .watch
                .focus
                .as_ref()
                .is_some_and(|f| self.watch.shell_modes.contains(f)),
        }
    }

    pub fn active_editor(&self) -> Option<&Editor> {
        match self.mode {
            Mode::Chat => Some(&self.editor),
            Mode::Watch => self
                .watch
                .focus
                .as_ref()
                .and_then(|f| self.watch.inputs.get(f)),
        }
    }

    fn active_editor_mut(&mut self) -> Option<&mut Editor> {
        match self.mode {
            Mode::Chat => Some(&mut self.editor),
            Mode::Watch => self
                .watch
                .focus
                .clone()
                .map(|f| self.watch.inputs.entry(f).or_default()),
        }
    }

    /// Working directory of the active session/tile (wrapped in `Some` when non-empty).
    pub fn current_cwd(&self) -> Option<String> {
        let s = if self.mode == Mode::Watch
            && let Some(f) = &self.watch.focus
            && let Some(v) = self.views.get(f)
            && !v.info.cwd.is_empty()
        {
            v.info.cwd.clone()
        } else if let Some(v) = self.cur()
            && !v.info.cwd.is_empty()
        {
            v.info.cwd.clone()
        } else {
            self.cwd.clone()
        };
        (!s.is_empty()).then_some(s)
    }

    fn cwd_of(&self, sid: Option<&str>) -> String {
        sid.and_then(|id| self.views.get(id))
            .map(|v| v.info.cwd.as_str())
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .or_else(|| self.current_cwd())
            .unwrap_or_else(|| self.cwd.clone())
    }
    pub fn agents(&self) -> &[ref_proto::AgentInfo] {
        config_agents(&self.config)
    }

    pub fn models(&self) -> &[ref_proto::ModelInfo] {
        config_models(&self.config)
    }

    /// Whether `sid` is not the session in the foreground chat view.
    pub fn is_background_view(&self, sid: &str) -> bool {
        self.mode == Mode::Watch || self.current.as_deref() != Some(sid)
    }

    /// Name of the session a modal belongs to, shown only when it is not the one in front.
    pub fn modal_origin_name(&self, sid: &str) -> Option<String> {
        self.is_background_view(sid).then(|| {
            self.views
                .get(sid)
                .map(|v| v.info.name.clone())
                .unwrap_or_default()
        })
    }

    pub fn agent_color(&self, name: &str) -> Color {
        theme::agent_color(self.agents(), name)
    }

    // -----------------------------------------------------------------------
    // Network events
    // -----------------------------------------------------------------------

    pub fn on_net(&mut self, ev: NetEvent) {
        self.dirty = true;
        match ev {
            NetEvent::Status(s) => self.net_status = Some(s),
            NetEvent::Warn(msg) => self.toasts.warn(msg),
            NetEvent::Connected => {
                let reconnect = self.ever_connected;
                self.connected = true;
                self.ever_connected = true;
                self.net_status = None;
                self.refused = None;
                self.backend_too_old = None;
                self.watch.subscribed = false;
                // A restarted backend may support McpStatus now.
                self.mcp_fallback = false;
                self.mcp_inflight = false;
                self.mcp_poll_at = None;
                self.dir_cache.clear();
                self.dirs_inflight.clear();
                self.bin_cache.clear();
                self.bins_inflight.clear();
                self.history_inflight = false;
                self.sessions_due = None;
                let cwd = self.current_cwd();
                self.req(
                    Request::GetConfig { cwd },
                    Pending::Config {
                        toast_warnings: true,
                    },
                );
                self.req(Request::ListSessions, Pending::LiveList);
                self.req(Request::ListGroups, Pending::Groups { ok: None });
                self.refresh_mcp();
                self.update_autocomplete();
                if reconnect {
                    self.toasts.info("Reconnected");
                    // Re-attach everything we had.
                    let ids: Vec<(String, bool)> = self
                        .views
                        .iter()
                        .map(|(k, v)| (k.clone(), v.foreground))
                        .collect();
                    for (sid, fg) in ids {
                        self.send_attach(sid, fg);
                    }
                } else if let Some(plan) = self.init.take() {
                    self.run_init(plan);
                }
            }
            NetEvent::BackendTooOld { protocol } => {
                self.backend_too_old = Some(protocol);
            }
            NetEvent::Disconnected(reason) => {
                if self.connected {
                    self.toasts.warn(format!("Disconnected: {reason}"));
                }
                self.connected = false;
                if !self.ever_connected {
                    self.net_status = Some(reason);
                }
                self.pending.clear();
                // The reply in flight is lost; the snapshot after reconnecting says if the
                // question is still open.
                if let Some(q) = self.question.as_mut() {
                    q.submitting = false;
                }
                self.attaching.clear();
                self.files_inflight = false;
                self.dirs_inflight.clear();
                self.bins_inflight.clear();
                self.history_inflight = false;
                self.sessions_due = None;
                self.mcp_inflight = false;
                self.mcp_poll_at = None;
            }
            NetEvent::Refused(reason) => {
                if self.connected {
                    self.toasts.error(reason.clone());
                }
                self.connected = false;
                self.refused = Some(reason.clone());
                if !self.ever_connected {
                    self.net_status = Some(reason);
                }
                self.pending.clear();
                if let Some(q) = self.question.as_mut() {
                    q.submitting = false;
                }
                self.attaching.clear();
                self.files_inflight = false;
                self.dirs_inflight.clear();
                self.bins_inflight.clear();
                self.history_inflight = false;
                self.sessions_due = None;
                self.mcp_inflight = false;
                self.mcp_poll_at = None;
            }
            NetEvent::Frame(f) => self.on_frame(f),
        }
    }

    fn run_init(&mut self, plan: InitPlan) {
        if let Some(sid) = plan.session.clone() {
            self.attach_fg(&sid);
            if let Some(p) = plan.prompt {
                self.send_prompt(&sid, p, Vec::new());
            }
        } else if plan.continue_last {
            self.init = Some(InitPlan {
                continue_last: false,
                ..plan
            });
            self.req(
                Request::ListHistory {
                    cwd: Some(self.cwd.clone()),
                    limit: Some(1),
                    creator: None,
                    query: None,
                },
                Pending::Continue,
            );
        } else if plan.watch {
            self.mode = Mode::Watch;
        } else if let Some(p) = plan.prompt {
            self.create_session(Some(PostCreate::Prompt(p, Vec::new())));
        }
    }
    fn on_frame(&mut self, f: ServerFrame) {
        match f {
            ServerFrame::Response {
                id,
                ok,
                result,
                error,
                code,
            } => {
                let Some(p) = self.pending.remove(&id) else {
                    return;
                };
                if ok {
                    self.on_response(p, result);
                } else {
                    self.on_error(
                        p,
                        ReplyError {
                            message: error.unwrap_or_else(|| "request failed".into()),
                            code,
                        },
                    );
                }
            }
            ServerFrame::Event { session_id, event } => self.on_event(session_id, event),
            ServerFrame::Sessions { sessions } => self.on_sessions(sessions),
            ServerFrame::Toast { level, text } => self.toasts.push(level, text),
            // The TUI does not ask for notifications (Hello.notify is empty).
            ServerFrame::Notify { .. } => {}
            ServerFrame::ConfigChanged { warnings } => {
                // Skip redundant GetConfig + duplicate warning toast when our own ReloadConfig is in flight.
                let reload_inflight = self
                    .pending
                    .values()
                    .any(|p| matches!(p, Pending::ReloadConfig));
                if !reload_inflight {
                    if !warnings.is_empty() {
                        self.toasts.warn(warnings.join("; "));
                    }
                    let cwd = self.current_cwd();
                    self.req(
                        Request::GetConfig { cwd },
                        Pending::Config {
                            toast_warnings: warnings.is_empty(),
                        },
                    );
                    self.mcp_inflight = false;
                    self.refresh_mcp();
                }
            }
            ServerFrame::GroupsChanged { groups } => {
                self.groups = groups;
                self.groups_loaded = true;
                self.sync_stash_dialog();
            }
        }
    }

    fn on_error(&mut self, p: Pending, err: ReplyError) {
        match p {
            Pending::Attach { sid, fg } => {
                self.attaching.remove(&sid);
                if fg && self.current.as_deref() == Some(sid.as_str()) {
                    // E.g. an empty session that didn't survive a backend restart.
                    let empty = self.views.get(&sid).is_none_or(|v| v.is_empty());
                    if let Some(v) = self.views.get_mut(&sid) {
                        v.closed = true;
                        v.info.live = false;
                        v.info.status = SessionStatus::Idle;
                    }
                    if empty {
                        self.views.remove(&sid);
                        self.current = None;
                        self.toasts.warn("Session is gone (backend restarted)");
                    } else {
                        self.toasts.error(format!(
                            "Cannot open session: {err} — your next prompt starts a new session"
                        ));
                    }
                } else if fg {
                    self.toasts.error(format!("Cannot open session: {err}"));
                } else {
                    self.views.remove(&sid);
                }
            }
            Pending::MessagesRange {
                sid, generation, ..
            } => {
                // Stop asking until the next snapshot / reset (no retry loop); keep the rest.
                if let Some(v) = self
                    .views
                    .get_mut(&sid)
                    .filter(|v| v.history_gen == generation)
                {
                    v.range_pending = None;
                    v.range_halted = true;
                }
            }
            Pending::Prompt { sid, text, key, .. } => {
                if let Some(v) = self.views.get_mut(&sid) {
                    v.queued.retain(|q| Some(q.key) != key);
                }
                if self.editor.is_empty() && !text.is_empty() && self.mode == Mode::Chat {
                    self.editor.set_text(&text);
                }
                self.toasts.error(format!("Prompt not sent: {err}"));
            }
            Pending::Files { .. } => {
                self.files_inflight = false;
            }
            Pending::Dirs { key } => {
                self.dirs_inflight
                    .remove(&(key.cwd.clone(), key.path.clone(), key.include_files));
                if err.code.as_deref() == Some(ref_proto::ERR_DIR_NOT_FOUND) {
                    self.dir_cache.insert(
                        key,
                        DirListing {
                            resolved_path: String::new(),
                            dirs: Vec::new(),
                            files: Vec::new(),
                            not_found: true,
                        },
                    );
                    self.refresh_cd_dialog();
                    self.update_autocomplete();
                }
            }
            Pending::Binaries { query } => {
                self.bins_inflight.remove(&query);
            }
            Pending::ShellComplete { cwd, line } => {
                self.shell_complete_inflight
                    .remove(&(cwd.clone(), line.clone()));
                self.shell_complete_errors.insert((cwd, line));
                self.update_autocomplete();
            }
            Pending::HistoryDialog { .. } => {
                self.history_inflight = self.has_history_inflight();
                self.toasts.error(err.message);
            }
            Pending::HistoryBg => {
                self.history_inflight = self.has_history_inflight();
            }
            Pending::SwitchSession { query } => {
                self.history_inflight = self.has_history_inflight();
                self.switch_session_by_query(&query, true);
            }
            Pending::Continue => {
                self.init = None;
                self.toasts.warn(format!("No session to continue: {err}"));
            }
            Pending::Mcp => {
                // Older backend ("unknown op" / deserialize error): use the GetConfig lines.
                self.mcp_inflight = false;
                self.mcp_fallback = true;
                if let Some(c) = &self.config {
                    let lines = c.mcp.clone();
                    self.set_mcp_lines(&lines);
                } else {
                    self.mcp_poll_at = Some(Instant::now());
                }
            }
            Pending::ConfigMcp => self.mcp_inflight = false,
            Pending::Permission => {
                let gone = err.code.as_deref() == Some(ref_proto::ERR_NO_PENDING_PERMISSION)
                    || err.message == ref_proto::NO_PENDING_PERMISSION; // remove fallback with protocol 5
                if !gone {
                    self.toasts.error(err.message);
                }
            }
            Pending::Question { sid, call_id } => {
                let answered_elsewhere = err.code.as_deref()
                    == Some(ref_proto::ERR_NO_PENDING_QUESTION)
                    || err.message == NO_PENDING_QUESTION; // remove fallback with protocol 5
                if answered_elsewhere {
                    // Answered somewhere else: nothing to say.
                    self.close_question(&sid, &call_id);
                } else if let Some(q) = self.question.as_mut().filter(|q| q.call_id == call_id) {
                    q.submitting = false;
                    q.error = Some(err.message);
                }
            }
            Pending::Unqueue { sid, key } => {
                if let Some(q) = self.views.get_mut(&sid).and_then(|v| v.queued_mut(key)) {
                    q.removing = false;
                }
                self.toasts
                    .error(format!("Cannot remove queued prompt: {err}"));
            }
            _ => self.toasts.error(err.message),
        }
    }

    fn on_response(&mut self, p: Pending, result: Value) {
        match p {
            Pending::Permission => {}
            Pending::Question { sid, call_id } => self.close_question(&sid, &call_id),
            Pending::Config { toast_warnings } => self.on_config_response(&result, toast_warnings),
            Pending::ReloadConfig => self.on_reload_config_response(&result),
            Pending::LiveList => match sessions_of(&result) {
                Ok(s) => self.on_sessions(s),
                Err(e) => self
                    .toasts
                    .error(format!("Malformed sessions response: {e}")),
            },
            Pending::Created { post } => {
                let info = match session_info_of(&result) {
                    Ok(i) => i,
                    Err(e) => {
                        self.toasts
                            .error(format!("Malformed create_session response: {e}"));
                        return;
                    }
                };
                let sid = info.id.clone();
                self.views
                    .entry(sid.clone())
                    .or_insert_with(|| SessionView::new(info, true));
                self.attach_fg(&sid);
                match post {
                    Some(PostCreate::Prompt(t, imgs)) => self.send_prompt(&sid, t, imgs),
                    Some(PostCreate::Shell(cmd_str)) => self.req(
                        Request::Shell {
                            session_id: sid,
                            command: cmd_str,
                        },
                        Pending::Simple { ok: String::new() },
                    ),
                    None => {}
                }
            }
            Pending::Attach { sid, fg } => {
                self.attaching.remove(&sid);
                let snap = result
                    .get("snapshot")
                    .and_then(|s| serde_json::from_value::<SessionSnapshot>(s.clone()).ok());
                let Some(snap) = snap else {
                    self.toasts.error("Malformed attach response");
                    return;
                };
                // Keep an existing view's flag: it may have changed while the request was in flight
                // (e.g. the current session went to the background on entering split view).
                let v = self
                    .views
                    .entry(sid.clone())
                    .or_insert_with(|| SessionView::new(snap.info.clone(), fg));
                v.load_snapshot(snap);
            }
            Pending::HistoryDialog { creator, query } => {
                self.history_inflight = self.has_history_inflight();
                match sessions_of(&result) {
                    Ok(s) => {
                        if creator.is_none() && query.is_none() {
                            self.history_loaded = true;
                            self.history_list = s;
                            if self.session_dialog_filter() == Some((None, None)) {
                                self.history_filtered = None;
                            }
                            self.refresh_session_dialog();
                        } else if self.session_dialog_filter()
                            == Some((creator.clone(), query.clone()))
                        {
                            self.history_filtered = Some((creator, query, s));
                            self.refresh_session_dialog();
                        }
                    }
                    Err(e) => self
                        .toasts
                        .error(format!("Malformed history response: {e}")),
                }
            }
            Pending::HistoryBg => {
                self.history_inflight = self.has_history_inflight();
                if let Ok(s) = sessions_of(&result) {
                    self.history_loaded = true;
                    self.history_list = s;
                    self.update_autocomplete();
                }
            }
            Pending::SwitchSession { query } => {
                self.history_inflight = self.has_history_inflight();
                if let Ok(s) = sessions_of(&result) {
                    self.history_loaded = true;
                    self.history_list = s;
                }
                self.switch_session_by_query(&query, true);
            }
            Pending::Continue => self.on_continue_response(&result),
            Pending::Undo { sid, count } => {
                let t = match text_of(&result) {
                    Ok(t) => t,
                    Err(e) => {
                        self.toasts.error(format!("Malformed undo response: {e}"));
                        return;
                    }
                };
                let images = response_images(&result);
                let what = match count {
                    Some(1) => "Undid 1 message".to_string(),
                    Some(n) => format!("Undid {n} messages"),
                    None => "Undid last prompt".to_string(),
                };
                // Undo also clears the server queue.
                if let Some(v) = self.views.get_mut(&sid) {
                    v.queued.clear();
                }
                // Only human prompts come back (text and images are empty otherwise).
                if !t.trim().is_empty() || !images.is_empty() {
                    self.restore_prompt(&sid, t, images);
                }
                self.toasts.info(what);
            }
            Pending::Closed { name } => {
                if !name.is_empty() {
                    self.toasts.info(format!("Closed \"{name}\""));
                }
                if matches!(&self.dialog, Some(Dialog::List(d)) if d.kind == Kind::Sessions) {
                    // The closed session moves to History.
                    self.request_sessions_history();
                }
            }
            Pending::DraftCd => {
                let path = parse_field::<String>(&result, "path").unwrap_or_default();
                if !path.is_empty() {
                    self.apply_draft_cwd(path);
                }
            }
            Pending::Expand { sid } => {
                let t = match text_of(&result) {
                    Ok(t) => t.to_string(),
                    Err(e) => {
                        self.toasts.error(format!("Malformed expand response: {e}"));
                        return;
                    }
                };
                if t.trim().is_empty() {
                    self.toasts.warn("Command expanded to nothing");
                    return;
                }
                match sid {
                    Some(s) => self.send_prompt(&s, t, Vec::new()),
                    None => self.create_session(Some(PostCreate::Prompt(t, Vec::new()))),
                }
            }
            Pending::Files { query } => self.on_files_response(&query, &result),
            Pending::Dirs { key } => self.on_dirs_response(key, &result),
            Pending::Binaries { query } => self.on_binaries_response(query, &result),
            Pending::ShellComplete { cwd, line } => {
                self.on_complete_shell_response(cwd, line, &result)
            }
            Pending::Cwd { sid } => match session_info_of(&result) {
                Ok(info) => {
                    if self.current.as_deref() == Some(sid.as_str()) {
                        // New sessions, `@` mentions and exports follow the directory.
                        self.cwd = info.cwd.clone();
                    }
                    self.toasts
                        .info(format!("Working directory: {}", text::tilde(&info.cwd)));
                    if let Some(v) = self.views.get_mut(&sid) {
                        v.set_info(info);
                    }
                    self.dir_cache.clear();
                    let cwd = self.current_cwd();
                    self.req(
                        Request::GetConfig { cwd },
                        Pending::Config {
                            toast_warnings: true,
                        },
                    );
                    self.mcp_inflight = false;
                    self.refresh_mcp();
                    self.update_autocomplete();
                }
                Err(e) => self
                    .toasts
                    .error(format!("Malformed set_cwd response: {e}")),
            },
            Pending::Simple { ok } => {
                if result.get("session").is_some() {
                    match session_info_of(&result) {
                        Ok(info) => {
                            if let Some(v) = self.views.get_mut(&info.id) {
                                v.set_info(info);
                            }
                        }
                        Err(e) => {
                            self.toasts
                                .error(format!("Malformed session response: {e}"));
                            return;
                        }
                    }
                }
                if !ok.is_empty() {
                    self.toasts.info(ok);
                }
            }
            Pending::Variant => match session_info_of(&result) {
                Ok(info) => {
                    let v = info
                        .model
                        .variant
                        .clone()
                        .unwrap_or_else(|| "default".into());
                    if let Some(view) = self.views.get_mut(&info.id) {
                        view.set_info(info);
                    }
                    self.toasts.info(format!("Variant: {v}"));
                }
                Err(e) => self
                    .toasts
                    .error(format!("Malformed set_variant response: {e}")),
            },
            Pending::Export => match parse_field::<String>(&result, "path") {
                Ok(p) => self.toasts.info(format!("Exported to {p}")),
                Err(e) => self.toasts.error(format!("Malformed export response: {e}")),
            },
            Pending::Text { title } => match text_of(&result) {
                Ok(text) => self.dialog = Some(Dialog::Help(HelpDialog::text(&title, text))),
                Err(e) => self.toasts.error(format!("Malformed response: {e}")),
            },
            Pending::Prompt {
                sid,
                text,
                images,
                key,
            } => self.on_prompt_sent(&sid, text, images, key, &result),
            Pending::MessagesRange {
                sid,
                generation,
                start_seq,
                ..
            } => self.on_messages_range_response(&sid, generation, start_seq, &result),
            Pending::Unqueue { sid, key } => self.on_unqueue_response(&sid, key, &result),
            Pending::Mcp => {
                self.mcp_inflight = false;
                let servers = result
                    .get("servers")
                    .map(|s| serde_json::from_value::<Vec<ref_proto::McpServerStatus>>(s.clone()));
                match servers {
                    Some(Ok(list)) => {
                        let entries = list
                            .iter()
                            .map(crate::ui::mcp::McpEntry::from_status)
                            .collect();
                        self.set_mcp(entries);
                    }
                    _ => self.on_error(Pending::Mcp, "malformed mcp_status response".into()),
                }
            }
            Pending::ConfigMcp => {
                self.mcp_inflight = false;
                match parse_field::<ServerConfigInfo>(&result, "config") {
                    Ok(c) => self.set_mcp_lines(&c.mcp),
                    Err(e) => self
                        .toasts
                        .error(format!("Malformed config from backend: {e}")),
                }
            }
            Pending::RetryNow => {
                if flag_of(&result, "retried") {
                    self.toasts.info("Retrying now…");
                } else {
                    self.toasts.warn("Session is not waiting to retry");
                }
            }
            Pending::Groups { ok } => self.on_groups_response(&result, ok),
            Pending::StashAll => {
                let count = result
                    .get("group")
                    .and_then(|g| g.get("members"))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                let msg = if count == 0 {
                    Some("No live sessions to stash".to_string())
                } else {
                    Some(format!(
                        "Stashed {count} session{}",
                        if count == 1 { "" } else { "s" }
                    ))
                };
                self.on_groups_response(&result, msg);
            }
            Pending::CreateGroup => {
                self.on_groups_response(&result, Some("Created stash group".into()));
            }
            Pending::RestoreGroup => {
                let n = parse_field::<Vec<String>>(&result, "restored_ids")
                    .map(|v| v.len())
                    .unwrap_or(0);
                let msg = if n == 0 {
                    Some("No restorable sessions in group".into())
                } else {
                    Some(format!(
                        "Restored {n} session{}",
                        if n == 1 { "" } else { "s" }
                    ))
                };
                self.on_groups_response(&result, msg);
                self.req(Request::ListSessions, Pending::LiveList);
            }
            Pending::RestoreGroupMember => {
                let restored = flag_of(&result, "restored");
                let msg = if restored {
                    "Restored session".into()
                } else {
                    "Session is already live".into()
                };
                self.on_groups_response(&result, Some(msg));
                self.req(Request::ListSessions, Pending::LiveList);
            }
        }
    }
    fn on_config_response(&mut self, result: &Value, toast_warnings: bool) {
        let Some(c) = self.parse_and_apply_config(result) else {
            return;
        };
        if toast_warnings && !c.warnings.is_empty() {
            self.toasts.warn(c.warnings.join("; "));
        }
        if !self.mcp_fallback {
            self.refresh_mcp();
        }
    }

    fn on_reload_config_response(&mut self, result: &Value) {
        let Some(c) = self.parse_and_apply_config(result) else {
            return;
        };
        let warnings = parse_field::<Vec<String>>(result, "warnings")
            .ok()
            .unwrap_or(c.warnings);
        self.mcp_inflight = false;
        self.refresh_mcp();
        self.toasts.info("Configuration reloaded");
        if !warnings.is_empty() {
            self.toasts.warn(warnings.join("; "));
        }
    }

    fn parse_and_apply_config(&mut self, result: &Value) -> Option<ServerConfigInfo> {
        let parsed = result
            .get("config")
            .map(|c| serde_json::from_value::<ServerConfigInfo>(c.clone()));
        if let Some(Err(e)) = &parsed {
            self.toasts
                .error(format!("Malformed config from backend: {e}"));
        }
        let Some(Ok(c)) = parsed else {
            return None;
        };
        if let Some(t) = c.tui.get("thinking").and_then(Value::as_bool) {
            self.thinking = t;
        }
        if let Some(t) = c.tui.get("details").and_then(Value::as_bool) {
            self.details = t;
        }
        if let Some(t) = c.tui.get("sidebar").and_then(Value::as_bool) {
            self.sidebar = t;
        }
        if self.mcp_fallback || !self.mcp_loaded {
            let lines = c.mcp.clone();
            self.set_mcp_lines(&lines);
        }
        self.config = Some(c.clone());
        for v in self.views.values_mut() {
            v.chat.invalidate();
        }
        self.update_autocomplete();
        Some(c)
    }
    /// Answer to `--continue`: attach the newest session here, or stay on empty startup.
    fn on_continue_response(&mut self, result: &Value) {
        let s = match sessions_of(result) {
            Ok(s) => s,
            Err(e) => {
                self.toasts
                    .error(format!("Malformed continue response: {e}"));
                return;
            }
        };
        let Some(info) = s.first() else {
            self.init = None;
            self.toasts.warn("No previous session in this directory");
            return;
        };
        let sid = info.id.clone();
        self.views
            .entry(sid.clone())
            .or_insert_with(|| SessionView::new(info.clone(), true));
        self.attach_fg(&sid);
        if let Some(p) = self.init.take().and_then(|i| i.prompt) {
            self.send_prompt(&sid, p, Vec::new());
        }
    }

    fn on_files_response(&mut self, query: &str, result: &Value) {
        self.files_inflight = false;
        let files: Vec<String> = match parse_field(result, "files") {
            Ok(f) => f,
            Err(e) => {
                self.toasts.error(format!("Malformed file list: {e}"));
                return;
            }
        };
        if let Some((start, q)) = self.active_editor().and_then(Editor::mention)
            && q == query
        {
            let items: Vec<(String, String, String)> = files
                .into_iter()
                .map(|f| (format!("@{f} "), f, String::new()))
                .collect();
            let prev = self.ac.take();
            self.ac =
                Autocomplete::new(AcKind::File, " files ", items, start, false, prev.as_ref());
        }
        if let Some(w) = self.files_want.take()
            && w != query
        {
            self.request_files(w);
        }
    }

    fn on_dirs_response(&mut self, key: DirKey, result: &Value) {
        self.dirs_inflight
            .remove(&(key.cwd.clone(), key.path.clone(), key.include_files));
        let path = parse_field::<String>(result, "path").unwrap_or_default();
        let dirs = parse_field::<Vec<String>>(result, "dirs").unwrap_or_default();
        let files = parse_field::<Vec<String>>(result, "files").unwrap_or_default();
        self.dir_cache.insert(
            key,
            DirListing {
                resolved_path: path,
                dirs,
                files,
                not_found: false,
            },
        );
        self.refresh_cd_dialog();
        self.update_autocomplete();
    }

    fn on_binaries_response(&mut self, query: String, result: &Value) {
        self.bins_inflight.remove(&query);
        if let Ok(list) = parse_field::<Vec<String>>(result, "binaries") {
            self.bin_cache.insert(query, list);
            self.update_autocomplete();
        }
    }

    fn on_complete_shell_response(&mut self, cwd: String, line: String, result: &Value) {
        self.shell_complete_inflight
            .remove(&(cwd.clone(), line.clone()));
        let items = match parse_field::<Vec<ref_proto::ShellCompletionItem>>(result, "items") {
            Ok(items) => items,
            Err(e) => {
                self.toasts
                    .warn(format!("Malformed shell completions: {e}"));
                self.shell_complete_errors.insert((cwd, line));
                self.update_autocomplete();
                return;
            }
        };
        self.shell_complete_errors
            .remove(&(cwd.clone(), line.clone()));
        self.shell_complete_cache
            .insert(ShellCompleteKey { cwd, line }, items);
        self.update_autocomplete();
    }

    fn on_messages_range_response(
        &mut self,
        sid: &str,
        generation: u64,
        requested_start: usize,
        result: &Value,
    ) {
        let parsed = (|| -> Result<(Vec<Message>, bool, usize), String> {
            let msgs: Vec<Message> = parse_field(result, "messages")?;
            let more: bool = parse_field(result, "more_before")?;
            let actual_start = result
                .get("start_seq")
                .and_then(Value::as_u64)
                .map_or(requested_start, |s| s as usize);
            Ok((msgs, more, actual_start))
        })();
        let (msgs, more, actual_start) = match parsed {
            Ok(x) => x,
            Err(e) => {
                if let Some(v) = self
                    .views
                    .get_mut(sid)
                    .filter(|v| v.history_gen == generation)
                {
                    v.range_pending = None;
                }
                self.toasts.error(format!("Malformed history page: {e}"));
                return;
            }
        };
        if let Some(v) = self.views.get_mut(sid)
            && !v.apply_messages_range(generation, actual_start, msgs, more)
            && v.history_gen == generation
        {
            v.range_pending = None;
        }
    }

    fn on_groups_response(&mut self, result: &Value, ok: Option<String>) {
        match parse_field::<Vec<StashGroup>>(result, "groups") {
            Ok(groups) => {
                self.groups = groups;
                self.groups_loaded = true;
                self.sync_stash_dialog();
                if let Some(msg) = ok
                    && !msg.is_empty()
                {
                    self.toasts.info(msg);
                }
            }
            Err(e) => self
                .toasts
                .error(format!("Malformed stash groups response: {e}")),
        }
    }

    fn on_unqueue_response(&mut self, sid: &str, key: u64, result: &Value) {
        let Some(v) = self.views.get_mut(sid) else {
            return;
        };
        if !flag_of(result, "removed") {
            // Delivered first: the row goes away once the message is in the transcript.
            if let Some(q) = v.queued_mut(key) {
                q.removing = false;
            }
            self.toasts
                .info("Already sent; the prompt can no longer be removed");
            return;
        }
        // Back into the input box so it can be edited and sent again. The response has
        // the image bytes (rows built from server messages only hold stubs).
        let Some(i) = v.queued.iter().position(|q| q.key == key) else {
            return;
        };
        let q = v.queued.remove(i);
        let text = text_of(result).map_or(q.text, str::to_string);
        let images = restored_images(result, q.images);
        self.restore_prompt(sid, &text, images);
    }

    // -----------------------------------------------------------------------
    // MCP status
    // -----------------------------------------------------------------------

    /// Requests fresh MCP status (McpStatus, or GetConfig on older backends). No-op while a
    /// request is in flight.
    pub fn refresh_mcp(&mut self) {
        if !self.connected {
            // Re-requested on reconnect; don't keep a poll deadline that can't be served.
            self.mcp_poll_at = None;
            return;
        }
        if self.mcp_inflight {
            return;
        }
        self.mcp_inflight = true;
        self.mcp_poll_at = None;
        let cwd = self.current_cwd();
        if self.mcp_fallback {
            self.req(Request::GetConfig { cwd }, Pending::ConfigMcp);
        } else {
            self.req(Request::McpStatus { cwd }, Pending::Mcp);
        }
        self.mcp_inflight = self
            .pending
            .values()
            .any(|p| matches!(p, Pending::Mcp | Pending::ConfigMcp));
    }

    fn set_mcp_lines(&mut self, lines: &[(String, String)]) {
        let entries = lines
            .iter()
            .map(|(n, l)| crate::ui::mcp::McpEntry::from_line(n, l))
            .collect();
        self.set_mcp(entries);
    }

    fn set_mcp(&mut self, entries: Vec<crate::ui::mcp::McpEntry>) {
        self.mcp = crate::ui::mcp::sorted(entries);
        self.mcp_loaded = true;
        // Poll only while something is still connecting.
        self.mcp_poll_at = self
            .mcp
            .iter()
            .any(|e| e.connecting())
            .then(|| Instant::now() + MCP_POLL);
    }

    /// `/mcp`: read-only dialog of servers, errors and tools.
    pub fn open_mcp(&mut self, select: Option<&str>) {
        self.dialog = Some(Dialog::Mcp(crate::ui::mcp::McpDialog::new(select)));
        self.refresh_mcp();
    }

    fn on_event(&mut self, sid: String, ev: SessionEvent) {
        let closed = matches!(ev, SessionEvent::Closed);
        let in_background = self.is_background_view(&sid);
        let Some(v) = self.views.get_mut(&sid) else {
            return;
        };
        if !v.loaded && !closed {
            // Events before the snapshot are already reflected in it.
            return;
        }
        if let SessionEvent::Error { text } = &ev
            && in_background
        {
            let name = v.info.name.clone();
            self.toasts.error(text::truncate(
                &format!("{name}: {}", text::one_line(text)),
                TOAST_ERROR_MAX_CHARS,
            ));
        }
        v.apply(ev);
        if closed {
            if self.current.as_deref() == Some(sid.as_str()) {
                self.views.remove(&sid);
                self.current = None;
                self.perm = None;
                self.question = None;
                self.toasts.warn("Session closed");
            } else if !self.views.get(&sid).is_some_and(|v| v.foreground) {
                self.views.remove(&sid);
            }
        }
    }

    fn on_sessions(&mut self, sessions: Vec<SessionInfo>) {
        for s in &sessions {
            if let Some(v) = self.views.get_mut(&s.id) {
                // Keep our view's info fresh (status, name, title, model, context).
                v.set_info(s.clone());
            }
        }
        self.live = sessions;
        self.sync_watch();
        self.refresh_session_dialog();
    }

    /// Background sessions shown in split view (excluding the current foreground session),
    /// filtered by the direct-parent filter pill (`watch.filter`).
    pub fn tile_ids(&self) -> Vec<String> {
        let mut v: Vec<&SessionInfo> = self
            .live
            .iter()
            .filter(|s| {
                s.live
                    && s.attached == 0
                    && self.current.as_deref() != Some(s.id.as_str())
                    && self.matches_watch_filter(s)
            })
            .collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v.into_iter().map(|s| s.id.clone()).collect()
    }

    fn matches_watch_filter(&self, s: &SessionInfo) -> bool {
        let pid = s.parent.as_deref().filter(|p| !p.is_empty());
        match &self.watch.filter {
            CreatorFilter::All => true,
            CreatorFilter::UserCreated => pid.is_none(),
            CreatorFilter::ByCreator { id, .. } => pid == Some(id.as_str()),
        }
    }
    /// In watch mode: watch-attach new background sessions, drop ones that left.
    pub fn sync_watch(&mut self) {
        if !self.connected {
            return;
        }
        let want = self.mode == Mode::Watch;
        if want != self.watch.subscribed {
            self.watch.subscribed = want;
            self.send(Request::WatchAll { enabled: want });
        }
        let tiles: HashSet<String> = if self.mode == Mode::Watch {
            self.ensure_history_loaded();
            self.tile_ids().into_iter().collect()
        } else {
            HashSet::new()
        };
        for sid in &tiles {
            if !self.views.contains_key(sid) && !self.attaching.contains(sid) {
                if let Some(info) = self.live.iter().find(|s| &s.id == sid).cloned() {
                    self.views
                        .insert(sid.clone(), SessionView::new(info, false));
                }
                self.send_attach(sid.clone(), false);
            }
        }
        let drop: Vec<String> = self
            .views
            .iter()
            .filter(|(k, v)| {
                !v.foreground && !tiles.contains(*k) && self.current.as_deref() != Some(k.as_str())
            })
            .map(|(k, _)| k.clone())
            .collect();
        for sid in drop {
            self.views.remove(&sid);
            self.watch.inputs.remove(&sid);
            self.watch.shell_modes.remove(&sid);
            if self.watch.drag_tile.as_deref() == Some(sid.as_str()) {
                self.watch.drag_tile = None;
            }
            self.client.notify(Request::Detach { session_id: sid });
        }
        // No automatic focus: only drop it when the focused session left the split view.
        if let Some(f) = &self.watch.focus
            && !tiles.contains(f)
        {
            self.set_watch_focus(None);
        }
    }

    // -----------------------------------------------------------------------
    // Session actions
    // -----------------------------------------------------------------------

    /// Attach `sid` as foreground and make it current (detaching the previous one).
    pub fn attach_fg(&mut self, sid: &str) {
        if let Some(old) = self.current.clone()
            && old != sid
        {
            self.views.remove(&old);
            self.client.notify(Request::Detach { session_id: old });
        }
        if self.current.as_deref() != Some(sid) {
            self.sidebar_scroll = 0;
        }
        self.current = Some(sid.to_string());
        self.mode = Mode::Chat;
        let info = self
            .views
            .get(sid)
            .map(|v| v.info.clone())
            .or_else(|| {
                self.live
                    .iter()
                    .chain(self.history_list.iter())
                    .find(|s| s.id == sid)
                    .cloned()
            })
            .unwrap_or_else(|| placeholder_info(sid, &self.cwd));
        let v = self
            .views
            .entry(sid.to_string())
            .or_insert_with(|| SessionView::new(info, true));
        v.foreground = true;
        v.chat.scroll_to_bottom(false);
        self.send_attach(sid.to_string(), true);
        self.perm = None;
        self.question = None;
        self.exit_shell_mode();
        self.clear_selection();
    }

    /// Detaches from the current foreground session and returns to `current = None`.
    pub fn detach_current(&mut self) {
        let Some(sid) = self.current.take() else {
            self.toasts.info("No active session");
            return;
        };
        self.views.remove(&sid);
        if let Some(info) = self.live.iter_mut().find(|s| s.id == sid) {
            info.attached = info.attached.saturating_sub(1);
        }
        if self.connected {
            self.client.notify(Request::Detach { session_id: sid });
        }
        self.perm = None;
        self.question = None;
        self.exit_shell_mode();
        self.clear_selection();
        self.sync_watch();
        self.refresh_session_dialog();
    }

    pub fn create_session(&mut self, post: Option<PostCreate>) {
        let model = self.effective_draft_model_for_create();
        self.req(
            Request::CreateSession {
                cwd: self.cwd.clone(),
                agent: self.draft.agent.clone(),
                model,
                name: None,
            },
            Pending::Created { post },
        );
    }

    pub fn send_prompt(&mut self, sid: &str, text: String, images: Vec<ImageData>) {
        let mut key = None;
        if let Some(v) = self.views.get_mut(sid) {
            if v.is_busy() {
                let q = Queued::new(text.clone(), images.clone());
                key = Some(q.key);
                v.queued.push(q);
            }
            v.error = None;
            v.chat.scroll_to_bottom(true);
        }
        let p = Pending::Prompt {
            sid: sid.to_string(),
            text: text.clone(),
            images: images.clone(),
            key,
        };
        self.req(
            Request::Prompt {
                session_id: sid.to_string(),
                text,
                images,
            },
            p,
        );
    }

    /// `Prompt` response: binds the queued row to the server message id (or shows a row when the
    /// server queued a prompt we sent as idle). A delete clicked meanwhile is sent now.
    fn on_prompt_sent(
        &mut self,
        sid: &str,
        text: String,
        images: Vec<ImageData>,
        key: Option<u64>,
        result: &Value,
    ) {
        let queued = flag_of(result, "queued");
        // Older backends don't return the id: rows are then matched by text only.
        let Some(id) = result
            .get("message_id")
            .and_then(Value::as_str)
            .map(String::from)
        else {
            return;
        };
        let Some(v) = self.views.get_mut(sid) else {
            return;
        };
        if !queued || v.find(&id).is_some() {
            // Delivered right away (or already appended).
            v.queued.retain(|q| Some(q.key) != key);
            return;
        }
        // Listed under another row already (snapshot after a re-attach): keep that row.
        let listed_elsewhere = v
            .queued
            .iter()
            .any(|q| q.id.as_deref() == Some(id.as_str()) && Some(q.key) != key);
        if listed_elsewhere {
            let mut removing = false;
            if let Some(i) = key.and_then(|k| v.queued.iter().position(|q| q.key == k)) {
                removing = v.queued.remove(i).removing;
            }
            let listed = v
                .queued
                .iter()
                .find(|q| q.id.as_deref() == Some(id.as_str()))
                .map(|q| q.key);
            if removing && let Some(k) = listed {
                self.unqueue(sid, k);
            }
            return;
        }
        let q = match key.and_then(|k| v.queued.iter().position(|q| q.key == k)) {
            Some(i) => &mut v.queued[i],
            None => {
                v.queued.push(Queued::new(text, images));
                v.queued.last_mut().expect("just pushed")
            }
        };
        q.id = Some(id);
        if q.removing {
            let key = q.key;
            q.removing = false;
            self.unqueue(sid, key);
        }
    }

    /// Puts a withdrawn / undone prompt back into the input box (the focused tile's input in
    /// split view): fills it when empty, otherwise appends after a blank line.
    fn restore_prompt(&mut self, sid: &str, text: &str, images: Vec<(ImageData, Option<String>)>) {
        let editor = if self.mode == Mode::Watch {
            self.watch.inputs.entry(sid.to_string()).or_default()
        } else {
            &mut self.editor
        };
        editor.restore(text, images);
        if self.mode == Mode::Chat {
            self.ac = None;
        }
    }

    /// ✕ on a queued prompt: withdraw it from the server queue before it is sent.
    pub fn unqueue(&mut self, sid: &str, key: u64) {
        if !self.connected {
            self.toasts.warn(NOT_CONNECTED);
            return;
        }
        let Some(q) = self.views.get_mut(sid).and_then(|v| v.queued_mut(key)) else {
            return;
        };
        if q.removing || q.from.is_some() {
            return;
        }
        q.removing = true;
        // Without an id yet, the request goes out when the `Prompt` response arrives.
        if let Some(id) = q.id.clone() {
            self.req(
                Request::RemoveQueued {
                    session_id: sid.to_string(),
                    message_id: id,
                },
                Pending::Unqueue {
                    sid: sid.to_string(),
                    key,
                },
            );
        }
    }

    /// Sends to the current session, creating one if needed.
    fn submit_prompt_text(&mut self, text: String, images: Vec<ImageData>) {
        // Typed before the first connection: becomes the startup prompt.
        let before_first_connect = !self.ever_connected;
        if before_first_connect
            && let Some(plan) = self.init.as_mut()
            && images.is_empty()
        {
            match &mut plan.prompt {
                Some(p) => {
                    p.push_str("\n\n");
                    p.push_str(&text);
                }
                None => plan.prompt = Some(text),
            }
            self.toasts.info("Will send once connected");
            return;
        }
        match self.current.clone() {
            Some(sid) if self.views.get(&sid).is_some_and(|v| !v.closed) => {
                self.send_prompt(&sid, text, images)
            }
            _ => self.create_session(Some(PostCreate::Prompt(text, images))),
        }
    }

    pub fn abort(&mut self, sid: &str) {
        self.send(Request::Abort {
            session_id: sid.to_string(),
        });
    }

    pub fn reply_permission(&mut self, sid: &str, call_id: &str, allow: bool, always: bool) {
        self.req(
            Request::PermissionReply {
                session_id: sid.to_string(),
                call_id: call_id.to_string(),
                allow,
                always,
            },
            Pending::Permission,
        );
        if let Some(v) = self.views.get_mut(sid) {
            v.permissions.retain(|p| p.call_id != call_id);
        }
        self.perm = None;
    }

    /// The permission request to show (current session first, then watched tiles).
    pub fn active_permission(&self) -> Option<(String, ref_proto::PermissionRequest, usize)> {
        let order = self.modal_order();
        let total: usize = order
            .iter()
            .filter_map(|k| self.views.get(*k))
            .map(|v| v.permissions.len())
            .sum();
        for k in order {
            if let Some(v) = self.views.get(k)
                && let Some(p) = v.permissions.first()
            {
                return Some((k.clone(), p.clone(), total - 1));
            }
        }
        None
    }

    /// Answers a question (`None`: dismiss). The dialog stays until the backend has accepted
    /// the reply, so a refused answer can be corrected.
    pub fn reply_question(&mut self, sid: &str, call_id: &str, answers: Option<Vec<Vec<String>>>) {
        if !self.connected {
            self.toasts.warn(NOT_CONNECTED);
            return;
        }
        if let Some(q) = self.question.as_mut() {
            q.submitting = true;
        }
        self.req(
            Request::QuestionReply {
                session_id: sid.to_string(),
                call_id: call_id.to_string(),
                answers,
            },
            Pending::Question {
                sid: sid.to_string(),
                call_id: call_id.to_string(),
            },
        );
    }

    fn close_question(&mut self, sid: &str, call_id: &str) {
        if let Some(v) = self.views.get_mut(sid) {
            v.questions.retain(|q| q.call_id != call_id);
        }
        if self.question.as_ref().is_some_and(|q| q.call_id == call_id) {
            self.question = None;
        }
    }

    /// The question to show: like the permission request, current session first.
    pub fn active_question(&self) -> Option<(String, ref_proto::QuestionRequest)> {
        let order = self.modal_order();
        order.into_iter().find_map(|k| {
            let q = self.views.get(k)?.questions.first()?;
            Some((k.clone(), q.clone()))
        })
    }

    /// Sessions whose modals may be shown, in the order they are tried.
    fn modal_order(&self) -> Vec<&String> {
        let mut order: Vec<&String> = Vec::new();
        if let Some(c) = &self.current
            && self.mode == Mode::Chat
        {
            order.push(c);
        }
        if self.mode == Mode::Watch {
            if let Some(f) = &self.watch.focus {
                order.push(f);
            }
            for (k, _) in &self.watch.tiles {
                order.push(k);
            }
            // The current session (background while the split view is shown) may not be a tile.
            if let Some(c) = &self.current
                && !order.contains(&c)
            {
                order.push(c);
            }
        }
        order
    }

    pub fn copy_text(&mut self, t: &str, what: &str) {
        let n = t.chars().count();
        if n == 0 {
            self.toasts.warn("Nothing to copy");
            return;
        }
        match crate::clipboard::copy(t) {
            Ok(crate::clipboard::CopyOutcome::Native) => {
                self.toasts.info(format!("Copied {what} ({n} chars)"));
            }
            Ok(crate::clipboard::CopyOutcome::Osc52Only) => {
                self.toasts.info("Copied via terminal (OSC 52)");
            }
            Err(e) => {
                self.toasts.error(format!("Copy failed: {e}"));
            }
        }
    }

    pub fn copy_message(&mut self, sid: &str, msg: &str) {
        let raw = self
            .views
            .get(sid)
            .and_then(|v| v.find(msg).and_then(|i| v.message_at(i)))
            .map(ref_proto::Message::raw_text);
        match raw {
            Some(r) if !r.is_empty() => self.copy_text(&r, "raw message"),
            Some(_) => self.toasts.warn("Message has no text"),
            None => {}
        }
    }

    /// Copies the active selection, if any. Returns true when something was copied.
    pub fn copy_selection(&mut self) -> bool {
        let owner = self.sel_owner.clone();
        let t = owner
            .as_ref()
            .and_then(|o| self.views.get(o))
            .and_then(|v| v.chat.selected_text());
        if let Some(t) = t {
            self.copy_text(&t, "selection");
            if let Some(v) = owner.and_then(|o| self.views.get_mut(&o)) {
                v.chat.sel = None;
                v.chat.drag_edge = 0;
            }
            self.sel_owner = None;
            self.watch.drag_tile = None;
            return true;
        }
        if let Some(t) = self.active_editor().and_then(|e| e.selected_text()) {
            self.copy_text(&t, "selection");
            if let Some(ed) = self.active_editor_mut() {
                ed.select_mode(false);
            }
            return true;
        }
        false
    }

    pub fn has_selection(&self) -> bool {
        self.sel_owner
            .as_ref()
            .and_then(|o| self.views.get(o))
            .is_some_and(|v| v.chat.has_selection())
    }

    pub fn clear_selection(&mut self) {
        if let Some(sid) = self.watch.drag_tile.take()
            && let Some(v) = self.views.get_mut(&sid)
        {
            v.chat.sel = None;
            v.chat.drag_edge = 0;
        }
        if let Some(v) = self.sel_owner.take().and_then(|o| self.views.get_mut(&o)) {
            v.chat.sel = None;
            v.chat.drag_edge = 0;
        }
    }
    fn toggle_details(&mut self) {
        self.details = !self.details;
        self.reset_toggles();
        self.toasts.info(if self.details {
            "Tool details expanded"
        } else {
            "Tool details collapsed"
        });
    }

    fn toggle_thinking(&mut self) {
        self.thinking = !self.thinking;
        self.reset_toggles();
        self.toasts.info(if self.thinking {
            "Thinking expanded"
        } else {
            "Thinking collapsed"
        });
    }

    fn reset_toggles(&mut self) {
        for v in self.views.values_mut() {
            v.chat.toggled.clear();
        }
    }

    pub fn effective_draft_agent(&self) -> String {
        self.draft
            .agent
            .clone()
            .or_else(|| self.config.as_ref().map(|c| c.default_agent.clone()))
            .unwrap_or_default()
    }

    pub fn effective_draft_model(&self) -> Option<ModelRef> {
        let mut base = self.draft.model.clone().or_else(|| {
            let agent_name = self.effective_draft_agent();
            self.agents()
                .iter()
                .find(|a| a.name == agent_name)
                .and_then(|a| a.model.clone())
                .or_else(|| self.models().first().map(|m| m.model_ref(None)))
        })?;
        if let Some(var) = &self.draft.variant {
            base.variant = (!var.is_empty()).then(|| var.clone());
        }
        Some(base)
    }

    fn effective_draft_model_for_create(&self) -> Option<ModelRef> {
        if let Some(mut m) = self.draft.model.clone() {
            if let Some(var) = &self.draft.variant {
                m.variant = (!var.is_empty()).then(|| var.clone());
            }
            return Some(m);
        }
        let var = self.draft.variant.as_deref()?;
        let mut m = self.effective_draft_model()?;
        m.variant = (!var.is_empty()).then(|| var.to_string());
        Some(m)
    }

    pub fn cycle_agent(&mut self, dir: i32) {
        let agents: Vec<String> = self.agents().iter().map(|a| a.name.clone()).collect();
        if agents.is_empty() {
            return;
        }
        let target_sid = self.active_target_session();
        let cur = match target_sid.as_deref().and_then(|id| self.views.get(id)) {
            Some(v) => v.info.agent.clone(),
            None => self.effective_draft_agent(),
        };
        let i = agents
            .iter()
            .position(|a| *a == cur)
            .map(|i| i as i32)
            .unwrap_or(-1);
        let n = agents.len() as i32;
        let next = agents[((i + dir).rem_euclid(n)) as usize].clone();
        self.apply_agent(target_sid.as_deref(), &next, false);
    }

    pub fn cycle_variant(&mut self) {
        if let Some(sid) = self.active_target_session() {
            self.req(
                Request::SetVariant {
                    session_id: sid,
                    variant: None,
                },
                Pending::Variant,
            );
            return;
        }
        let Some(m) = self.effective_draft_model() else {
            return;
        };
        let variants = find_model(self.models(), &m)
            .map(|x| x.variants.clone())
            .unwrap_or_default();
        if variants.is_empty() {
            self.toasts.info("This model has no variants");
            return;
        }
        let cur = m.variant.as_deref();
        let pos = cur.and_then(|v| variants.iter().position(|x| x == v));
        let next = match pos {
            None => variants.first().cloned(),
            Some(i) if i + 1 < variants.len() => Some(variants[i + 1].clone()),
            Some(_) => None,
        };
        self.apply_draft_variant(next);
    }

    fn apply_draft_variant(&mut self, next: Option<String>) {
        let label = next.as_deref().unwrap_or("default").to_string();
        if let Some(m) = self.draft.model.as_mut() {
            m.variant = next.clone();
        }
        self.draft.variant = Some(next.unwrap_or_default());
        self.toasts.info(format!("Variant: {label}"));
    }

    // -----------------------------------------------------------------------
    // Dialogs
    // -----------------------------------------------------------------------

    pub fn open_palette(&mut self) {
        let target_sid = self.active_target_session();
        let mut items: Vec<Item> = COMMANDS
            .iter()
            .map(|c| Item {
                key: c.name.into(),
                title: c.desc.into(),
                desc: format!("/{}", c.name),
                right: c.key.into(),
                keywords: c.aliases.join(" "),
                ..Default::default()
            })
            .collect();
        if let Some(cfg) = &self.config {
            for c in &cfg.commands {
                items.push(Item {
                    key: format!("{SERVER_COMMAND_KEY_PREFIX}{}", c.name),
                    title: if c.description.is_empty() {
                        format!("/{}", c.name)
                    } else {
                        c.description.clone()
                    },
                    desc: format!("/{}", c.name),
                    right: c.source.clone(),
                    section: "Custom commands".into(),
                    ..Default::default()
                });
            }
        }
        for it in items.iter_mut().filter(|i| i.section.is_empty()) {
            it.section = "Commands".into();
        }
        self.dialog = Some(Dialog::List(
            ListDialog::new(Kind::Palette, "Commands", items).for_session(target_sid),
        ));
    }

    pub fn open_sessions(&mut self) {
        self.open_sessions_filtered(None);
    }

    fn has_history_inflight(&self) -> bool {
        self.pending.values().any(|p| {
            matches!(
                p,
                Pending::HistoryDialog { .. } | Pending::HistoryBg | Pending::SwitchSession { .. }
            )
        })
    }

    fn session_dialog_filter(&self) -> Option<(Option<String>, Option<String>)> {
        let Some(Dialog::List(d)) = self.dialog.as_ref() else {
            return None;
        };
        if d.kind != Kind::Sessions {
            return None;
        }
        let creator = match &d.creator_filter {
            CreatorFilter::All => None,
            CreatorFilter::UserCreated => Some(CREATOR_USER.to_string()),
            CreatorFilter::ByCreator { id, .. } => Some(id.clone()),
        };
        let q = d.filter.text.trim();
        let query = (!q.is_empty()).then(|| q.to_string());
        Some((creator, query))
    }

    pub fn request_sessions_history(&mut self) {
        let Some((creator, query)) = self.session_dialog_filter() else {
            return;
        };
        self.sessions_due = None;
        self.history_inflight = true;
        self.req(
            Request::ListHistory {
                cwd: None,
                limit: Some(HISTORY_LIST_LIMIT),
                creator: creator.clone(),
                query: query.clone(),
            },
            Pending::HistoryDialog { creator, query },
        );
    }

    pub fn cycle_dialog_creator_filter(&mut self) {
        let is_sessions = {
            let Some(Dialog::List(d)) = self.dialog.as_mut() else {
                return;
            };
            d.cycle_creator_filter();
            d.kind == Kind::Sessions
        };
        if is_sessions {
            if self.history_filtered.is_some() {
                self.history_filtered = None;
                self.refresh_session_dialog();
            }
            self.request_sessions_history();
        }
    }

    pub fn on_sessions_filter_edited(&mut self, text_changed: bool) {
        if let Some(Dialog::List(d)) = self.dialog.as_mut() {
            d.refilter();
        }
        if !text_changed {
            return;
        }
        let Some(filter) = self.session_dialog_filter() else {
            return;
        };
        let keep_filtered = self.history_filtered.as_ref().is_some_and(|(fc, fq, _)| {
            filter != (None, None)
                && *fc == filter.0
                && fq.as_deref().is_none_or(|prev| {
                    let prev_l = prev.to_lowercase();
                    filter
                        .1
                        .as_deref()
                        .is_some_and(|q| q.to_lowercase().contains(&prev_l))
                })
        });
        if !keep_filtered && self.history_filtered.take().is_some() {
            self.refresh_session_dialog();
        }
        self.sessions_due = Some(Instant::now() + HISTORY_QUERY_DEBOUNCE);
    }

    pub fn open_sessions_filtered(&mut self, initial_filter: Option<&str>) {
        self.history_filtered = None;
        self.sessions_due = None;
        let mut d = ListDialog::new(Kind::Sessions, "Sessions", Vec::new())
            .with_hint("enter open · tab filter · ctrl+d close live / delete history · esc");
        if let Some(q) = initial_filter.filter(|q| !q.is_empty()) {
            d.filter.set_text(q);
        }
        self.dialog = Some(Dialog::List(d));
        self.refresh_session_dialog();
        if initial_filter.is_none_or(str::is_empty)
            && let Some(c) = &self.current
            && let Some(Dialog::List(d)) = self.dialog.as_mut()
        {
            d.select_key(&live_key(c));
        }
        self.request_sessions_history();
        self.req(Request::ListSessions, Pending::LiveList);
    }

    fn active_dialog_history(&self) -> &[SessionInfo] {
        let Some((cur_creator, cur_query)) = self.session_dialog_filter() else {
            return &self.history_list;
        };
        if cur_creator.is_none() && cur_query.is_none() {
            return &self.history_list;
        }
        if let Some((fc, fq, list)) = &self.history_filtered
            && *fc == cur_creator
            && fq.as_deref().is_none_or(|prev| {
                let prev_l = prev.to_lowercase();
                cur_query
                    .as_deref()
                    .is_some_and(|q| q.to_lowercase().contains(&prev_l))
            })
        {
            return list;
        }
        &self.history_list
    }

    fn refresh_session_dialog(&mut self) {
        let elsewhere: HashSet<String> = self
            .live
            .iter()
            .filter(|s| self.attached_elsewhere(&s.id))
            .map(|s| s.id.clone())
            .collect();
        let active_history_owned = self.active_dialog_history().to_vec();
        let extra_history_owned = self
            .history_filtered
            .as_ref()
            .map(|(_, _, l)| l.clone())
            .unwrap_or_default();
        let Some(Dialog::List(d)) = self.dialog.as_mut() else {
            return;
        };
        if d.kind != Kind::Sessions && d.kind != Kind::CloseSessions {
            return;
        }
        let close_only = d.kind == Kind::CloseSessions;
        let now = ref_proto::now_ms();
        // Field-level helpers keep `self.dialog` mutably borrowed alongside.
        let models = config_models(&self.config);
        let agents = config_agents(&self.config);
        let creator_of = |parent: Option<&str>| -> Option<(String, String, String)> {
            let pid = parent.filter(|p| !p.is_empty())?;
            let info = self
                .live
                .iter()
                .chain(self.history_list.iter())
                .chain(extra_history_owned.iter())
                .find(|x| x.id == pid);
            let name = info
                .map(|x| x.name.as_str())
                .filter(|n| !n.is_empty())
                .unwrap_or(pid)
                .to_string();
            let title = info.map(|x| text::one_line(&x.title)).unwrap_or_default();
            Some((pid.to_string(), name, title))
        };
        let mut items = Vec::new();
        let mut live: Vec<&SessionInfo> = self.live.iter().filter(|s| s.live).collect();
        live.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        for s in &live {
            let current = self.current.as_deref() == Some(s.id.as_str());
            let model = find_model(models, &s.model)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| s.model.model.clone());
            let (badge, bc) = if elsewhere.contains(&s.id) {
                ("fg elsewhere", theme::WARNING)
            } else if s.attached > 0 {
                ("fg", theme::SUCCESS)
            } else {
                ("bg", theme::SECONDARY)
            };
            let desc = if current && close_only {
                "(current)".to_string()
            } else {
                text::truncate(&text::one_line(&s.title), SESSION_DESC_MAX_CHARS)
            };
            let creator_info = creator_of(s.parent.as_deref());
            let keywords = match &creator_info {
                Some((pid, cname, ctitle)) => {
                    format!("{} {} {} {} {}", s.id, s.cwd, pid, cname, ctitle)
                }
                None => format!("{} {}", s.id, s.cwd),
            };
            items.push(Item {
                key: live_key(&s.id),
                title: s.name.clone(),
                desc,
                right: format!("{} · {} · {}", s.agent, model, text::tilde(&s.cwd)),
                section: "Live".into(),
                badge: Some((badge.into(), bc)),
                title_color: Some(theme::agent_color(agents, &s.agent)),
                marker: Some((
                    if current {
                        "●".into()
                    } else if s.status.is_idle() {
                        "○".into()
                    } else {
                        "◉".into()
                    },
                    if current {
                        theme::PRIMARY
                    } else {
                        theme::MUTED
                    },
                )),
                spinner: !s.status.is_idle() && !current,
                keywords,
                creator: creator_info.map(|(pid, cname, _)| (pid, cname)),
            });
        }
        let live_ids: HashSet<&str> = live.iter().map(|s| s.id.as_str()).collect();
        let history: &[SessionInfo] = if close_only {
            &[]
        } else {
            &active_history_owned
        };
        for s in history.iter().filter(|s| !live_ids.contains(s.id.as_str())) {
            let title = if s.title.trim().is_empty() {
                s.name.clone()
            } else {
                text::one_line(&s.title)
            };
            let creator_info = creator_of(s.parent.as_deref());
            let keywords = match &creator_info {
                Some((pid, cname, ctitle)) => {
                    format!("{} {} {} {} {}", s.id, s.cwd, pid, cname, ctitle)
                }
                None => format!("{} {}", s.id, s.cwd),
            };
            items.push(Item {
                key: format!("{HISTORY_KEY_PREFIX}{}", s.id),
                title,
                desc: s.name.clone(),
                right: format!(
                    "{} · {}",
                    text::tilde(&s.cwd),
                    text::fmt_ago(s.updated_at, now)
                ),
                section: "History".into(),
                keywords,
                creator: creator_info.map(|(pid, cname, _)| (pid, cname)),
                ..Default::default()
            });
        }
        d.set_items(items);
    }
    #[cfg(test)]
    pub fn open_models(&mut self) {
        let sid = self.active_target_session();
        self.open_models_for(sid, None);
    }

    pub fn open_models_for(&mut self, target_sid: Option<String>, filter: Option<&str>) {
        let Some(cfg) = &self.config else { return };
        let cur = target_sid
            .as_deref()
            .and_then(|id| self.views.get(id))
            .or_else(|| self.cur())
            .map(|v| v.info.model.clone())
            .or_else(|| self.effective_draft_model());
        let items: Vec<Item> = cfg
            .models
            .iter()
            .map(|m| {
                let is_cur = cur
                    .as_ref()
                    .is_some_and(|c| c.provider == m.provider && c.model == m.id);
                let mut caps = Vec::new();
                if m.context > 0 {
                    caps.push(format!("{} ctx", text::fmt_tokens(m.context)));
                }
                if m.images {
                    caps.push("images".into());
                }
                if m.reasoning {
                    caps.push("reasoning".into());
                }
                Item {
                    key: format!("{}/{}", m.provider, m.id),
                    title: m.name.clone(),
                    desc: caps.join(text::SEPARATOR),
                    right: if m.variants.is_empty() {
                        String::new()
                    } else {
                        m.variants.join(" ")
                    },
                    section: if m.provider_name.is_empty() {
                        m.provider.clone()
                    } else {
                        m.provider_name.clone()
                    },
                    marker: Some(selected_marker(is_cur)),
                    keywords: format!("{} {}", m.provider, m.id),
                    ..Default::default()
                }
            })
            .collect();
        let title = if target_sid.is_some() {
            "Switch model · this session only"
        } else {
            "Select model · next session"
        };
        let mut d = ListDialog::new(Kind::Models, title, items).for_session(target_sid);
        if let Some(q) = filter.filter(|q| !q.is_empty()) {
            d.filter.set_text(q);
            d.refilter();
        } else if let Some(c) = cur {
            d.select_key(&c.key());
        }
        self.dialog = Some(Dialog::List(d));
    }

    fn open_agents_for(&mut self, target_sid: Option<String>, filter: Option<&str>) {
        let cur = target_sid
            .as_deref()
            .and_then(|id| self.views.get(id))
            .or_else(|| self.cur())
            .map(|v| v.info.agent.clone())
            .unwrap_or_else(|| self.effective_draft_agent());
        let agents = self.agents().to_vec();
        let items: Vec<Item> = agents
            .iter()
            .map(|a| Item {
                key: a.name.clone(),
                title: a.name.clone(),
                desc: text::one_line(&a.description),
                right: a.model.as_ref().map(|m| m.key()).unwrap_or_default(),
                title_color: Some(theme::agent_color(&agents, &a.name)),
                marker: Some(selected_marker(a.name == cur)),
                ..Default::default()
            })
            .collect();
        let mut d = ListDialog::new(Kind::Agents, "Switch agent", items).for_session(target_sid);
        if let Some(q) = filter.filter(|q| !q.is_empty()) {
            d.filter.set_text(q);
            d.refilter();
        } else {
            d.select_key(&cur);
        }
        self.dialog = Some(Dialog::List(d));
    }

    fn open_variants_for(&mut self, target_sid: Option<String>) {
        let m = target_sid
            .as_deref()
            .and_then(|id| self.views.get(id))
            .or_else(|| self.cur())
            .map(|v| v.info.model.clone())
            .or_else(|| self.effective_draft_model());
        let Some(m) = m else { return };
        let variants = find_model(self.models(), &m)
            .map(|x| x.variants.clone())
            .unwrap_or_default();
        if variants.is_empty() {
            self.toasts.info("This model has no variants");
            return;
        }
        let mut items = vec![Item {
            key: String::new(),
            title: "default".into(),
            marker: Some(selected_marker(m.variant.is_none())),
            ..Default::default()
        }];
        for x in variants {
            let is = m.variant.as_deref() == Some(x.as_str());
            items.push(Item {
                key: x.clone(),
                title: x,
                marker: Some(selected_marker(is)),
                ..Default::default()
            });
        }
        let mut d = ListDialog::new(Kind::Variants, "Model variant", items).for_session(target_sid);
        d.select_key(m.variant.as_deref().unwrap_or(""));
        self.dialog = Some(Dialog::List(d));
    }

    /// Opens the interactive `/cd` directory picker for `sid` (or draft cwd when `None`).
    pub fn open_cd(&mut self, sid: Option<&str>) {
        let base_cwd = self.cwd_of(sid);
        let initial = format_cd_dir(&base_cwd);
        let mut d = ListDialog::new(Kind::Cd, "Change working directory", Vec::new())
            .for_session(sid.map(str::to_string))
            .with_hint("↑↓ select · tab open dir · backspace up · enter confirm · esc");
        d.cd_base_cwd = base_cwd.clone();
        d.cd_resolved_dir = base_cwd;
        d.filter.set_text(&initial);
        self.dialog = Some(Dialog::List(d));
        self.refresh_cd_dialog();
    }

    /// Clears stale `cd_resolved_dir` after a filter edit in `Kind::Cd` and refreshes items.
    pub fn on_cd_filter_edited(&mut self) {
        if let Some(Dialog::List(d)) = self.dialog.as_mut()
            && d.kind == Kind::Cd
        {
            d.cd_resolved_dir.clear();
        }
        self.refresh_cd_dialog();
    }

    /// Updates `Kind::Cd` items from `DirCache` (or requests `ListDirs` without clearing items).
    pub fn refresh_cd_dialog(&mut self) {
        let (base_cwd, raw) = {
            let Some(Dialog::List(d)) = self.dialog.as_ref() else {
                return;
            };
            if d.kind != Kind::Cd {
                return;
            }
            (d.cd_base_cwd.clone(), d.filter.text.trim().to_string())
        };
        let sp = split_dir_query(&raw);
        let key = DirKey {
            cwd: base_cwd,
            path: sp.req_dir.clone(),
            include_files: false,
        };
        let Some(listing) = self.dir_cache.get(&key).cloned() else {
            self.request_dirs(key);
            return;
        };
        let Some(Dialog::List(d)) = self.dialog.as_mut() else {
            return;
        };
        if !listing.not_found && !listing.resolved_path.is_empty() {
            d.cd_resolved_dir = listing.resolved_path.clone();
        } else {
            d.cd_resolved_dir.clear();
        }
        let mut items = Vec::new();
        let cur_target = normalize_cd_target(&sp.display_prefix);
        if sp.name_prefix.is_empty() && !listing.not_found {
            items.push(Item {
                key: format!("use:{cur_target}"),
                title: "✓ Use this directory".into(),
                desc: text::tilde(&d.cd_resolved_dir),
                right: "enter".into(),
                title_color: Some(theme::SUCCESS),
                ..Default::default()
            });
        }
        let at_root = is_fs_root(&d.cd_resolved_dir);
        if !at_root
            && (sp.name_prefix.is_empty() || "..".starts_with(&sp.name_prefix.to_lowercase()))
        {
            items.push(Item {
                key: "nav:..".into(),
                title: "../".into(),
                desc: "parent directory".into(),
                right: "tab".into(),
                title_color: Some(theme::ACCENT),
                ..Default::default()
            });
        }
        for (_, label, _) in build_dir_items(&listing, &sp, false, false, false) {
            if matches!(label.as_str(), "./" | "../" | ".\\" | "..\\") {
                continue;
            }
            let short = label
                .strip_prefix(&sp.display_prefix)
                .unwrap_or(&label)
                .to_string();
            items.push(Item {
                key: format!("dir:{label}"),
                title: short,
                desc: String::new(),
                right: "tab open · enter select".into(),
                ..Default::default()
            });
        }
        if !sp.name_prefix.is_empty() && !raw.is_empty() {
            items.push(Item {
                key: format!("use:{raw}"),
                title: format!("✓ Go to {raw}"),
                desc: if listing.not_found {
                    "custom path".into()
                } else {
                    String::new()
                },
                right: "enter".into(),
                title_color: Some(theme::SUCCESS),
                ..Default::default()
            });
        }
        d.items = items;
        d.refilter();
        d.sel = 0;
    }

    /// `Tab` inside `Kind::Cd`: drills into the selected directory or goes up on `../`.
    pub fn cd_dialog_tab(&mut self) {
        let next_text = {
            let Some(Dialog::List(d)) = self.dialog.as_ref() else {
                return;
            };
            if d.kind != Kind::Cd {
                return;
            }
            let raw = d.filter.text.trim();
            if raw == "~" {
                Some("~/".to_string())
            } else if is_bare_drive(raw) {
                Some(format!("{raw}/"))
            } else if let Some(it) = d.selected() {
                if it.key == "nav:.." {
                    Some(cd_parent_nav(&d.filter.text, &d.cd_resolved_dir))
                } else {
                    it.key.strip_prefix("dir:").map(str::to_string)
                }
            } else {
                None
            }
        };
        if let Some(t) = next_text {
            if let Some(Dialog::List(d)) = self.dialog.as_mut() {
                d.cd_resolved_dir.clear();
                d.filter.set_text(&t);
            }
            self.refresh_cd_dialog();
        }
    }

    /// Confirms the raw typed path in `Kind::Cd` (`Ctrl+Enter`).
    pub fn cd_dialog_confirm_typed(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_ref() else {
            return;
        };
        if d.kind != Kind::Cd {
            return;
        }
        let raw = d.filter.text.trim().to_string();
        let target_sid = d
            .target_sid
            .clone()
            .or_else(|| self.active_target_session());
        self.dialog = None;
        match target_sid {
            Some(sid) => {
                let path = normalize_cd_target(&raw);
                self.send_set_cwd(&sid, &path);
            }
            None => {
                let path = normalize_cd_target(&raw);
                self.send_draft_cwd(&path);
            }
        }
    }

    pub fn open_help(&mut self) {
        let mut cmds: Vec<(String, String)> = COMMANDS
            .iter()
            .map(|c| (c.name.to_string(), c.desc.to_string()))
            .collect();
        if let Some(cfg) = &self.config {
            cmds.extend(
                cfg.commands
                    .iter()
                    .map(|c| (c.name.clone(), c.description.clone())),
            );
        }
        self.dialog = Some(Dialog::Help(HelpDialog::new(&cmds)));
    }

    /// Enter on a list dialog.
    pub fn dialog_submit(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_ref() else {
            return;
        };
        let kind = d.kind.clone();
        let target_sid = d.target_sid.clone();
        if kind == Kind::CloseSessions {
            self.close_from_picker();
            return;
        }
        if let Some(arg) = d.asking_arg.clone() {
            let val = d.filter.text.trim().to_string();
            if arg.cmd_key == cmd::RENAME && val.is_empty() {
                self.toasts.warn("Session name cannot be empty");
                return;
            }
            self.dialog = None;
            if let Some(srv) = arg.cmd_key.strip_prefix(SERVER_COMMAND_KEY_PREFIX) {
                let sid = target_sid.or_else(|| self.active_target_session());
                self.expand_custom_command(sid, srv, &val);
            } else {
                self.run_command_for(target_sid, &arg.cmd_key, &val);
            }
            return;
        }
        if kind == Kind::Cd {
            let raw = d.filter.text.trim().to_string();
            let sel_key = d.selected().map(|i| i.key.clone());
            if sel_key.as_deref() == Some("nav:..") {
                self.cd_dialog_tab();
                return;
            }
            let chosen = match sel_key.as_deref() {
                Some(k) if k.starts_with("use:") => normalize_cd_target(&k["use:".len()..]),
                Some(k) if k.starts_with("dir:") => normalize_cd_target(&k["dir:".len()..]),
                _ if !raw.is_empty() => normalize_cd_target(&raw),
                _ => return,
            };
            self.dialog = None;
            match target_sid.or_else(|| self.active_target_session()) {
                Some(sid) => self.send_set_cwd(&sid, &chosen),
                None => self.send_draft_cwd(&chosen),
            }
            return;
        }
        let Some(item) = d.selected().cloned() else {
            return;
        };
        let key = item.key;
        if kind == Kind::Palette {
            if let Some(name) = key.strip_prefix(SERVER_COMMAND_KEY_PREFIX) {
                if let Some(Dialog::List(d)) = self.dialog.as_mut() {
                    d.begin_arg_prompt(
                        key.clone(),
                        format!("/{name}"),
                        item.title,
                        "Arguments (optional — enter to run)…".into(),
                    );
                }
                return;
            }
            if let Some(c) = find_cmd(&key)
                && matches!(c.name, cmd::RENAME | cmd::PEERS_NAME)
            {
                let placeholder = if c.name == cmd::RENAME {
                    "New session name…"
                } else {
                    "Peer name (leave empty to view current)…"
                };
                if let Some(Dialog::List(d)) = self.dialog.as_mut() {
                    d.begin_arg_prompt(
                        c.name.to_string(),
                        format!("/{}", c.name),
                        c.desc.to_string(),
                        placeholder.into(),
                    );
                }
                return;
            }
        }
        self.dialog = None;
        match kind {
            Kind::Palette => {
                if let Some(c) = find_cmd(&key) {
                    self.run_command_for(target_sid, c.name, "");
                }
            }
            Kind::Sessions => {
                let sid = key.split_once(':').map(|x| x.1).unwrap_or(&key).to_string();
                self.switch_to_session(&sid);
            }
            Kind::Models => {
                if let Some(m) = ModelRef::parse(&key) {
                    let sid = target_sid.or_else(|| self.active_target_session());
                    self.apply_model(sid.as_deref(), m, None);
                }
            }
            Kind::Agents => {
                let sid = target_sid.or_else(|| self.active_target_session());
                self.apply_agent(sid.as_deref(), &key, false);
            }
            Kind::Variants => {
                match target_sid.or_else(|| self.active_target_session()) {
                    Some(sid) => {
                        // Empty `key` is "default": send `Some("")` to clear variant (`None` cycles).
                        self.req(
                            Request::SetVariant {
                                session_id: sid,
                                variant: Some(key),
                            },
                            Pending::Variant,
                        );
                    }
                    None => {
                        let next = (!key.is_empty()).then_some(key);
                        self.apply_draft_variant(next);
                    }
                }
            }
            Kind::CloseSessions | Kind::Cd => {}
        }
    }

    /// ctrl+d in the sessions dialog: confirm, then close (live) / delete (history).
    pub fn dialog_delete(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_ref() else {
            return;
        };
        if d.kind == Kind::CloseSessions {
            self.close_from_picker();
            return;
        }
        if d.kind != Kind::Sessions {
            return;
        }
        let Some(key) = d.selected().map(|i| i.key.clone()) else {
            return;
        };
        let elsewhere = key
            .strip_prefix(LIVE_KEY_PREFIX)
            .is_some_and(|s| self.attached_elsewhere(s));
        let Some(Dialog::List(d)) = self.dialog.as_mut() else {
            return;
        };
        if d.confirm.as_deref() != Some(key.as_str()) {
            d.confirm_text = if key.starts_with(LIVE_KEY_PREFIX) {
                if elsewhere {
                    "open in another frontend · ctrl+d again to close "
                } else {
                    "ctrl+d again to close "
                }
            } else {
                "ctrl+d again to delete permanently "
            }
            .into();
            d.confirm = Some(key);
            return;
        }
        d.confirm = None;
        if let Some(sid) = key.strip_prefix(LIVE_KEY_PREFIX) {
            let sid = sid.to_string();
            self.close_session(&sid);
        } else if let Some(sid) = key.strip_prefix(HISTORY_KEY_PREFIX) {
            let sid = sid.to_string();
            self.history_list.retain(|s| s.id != sid);
            if let Some((_, _, list)) = &mut self.history_filtered {
                list.retain(|s| s.id != sid);
            }
            self.req(
                Request::Delete { session_id: sid },
                Pending::Simple {
                    ok: "Session deleted".into(),
                },
            );
            self.refresh_session_dialog();
        }
    }

    // -----------------------------------------------------------------------
    // Closing sessions / undo
    // -----------------------------------------------------------------------

    /// Display name of a session we know about (view, live list or history).
    pub fn session_name(&self, sid: &str) -> String {
        self.views
            .get(sid)
            .map(|v| v.info.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| {
                self.live
                    .iter()
                    .chain(self.history_list.iter())
                    .find(|s| s.id == sid)
                    .map(|s| s.name.clone())
            })
            .unwrap_or_else(|| sid.to_string())
    }

    /// Whether `sid` is foreground in a frontend other than this one.
    pub fn attached_elsewhere(&self, sid: &str) -> bool {
        let Some(info) = self.live.iter().find(|s| s.id == sid) else {
            return false;
        };
        let ours = u32::from(
            self.current.as_deref() == Some(sid)
                && self.views.get(sid).is_some_and(|v| v.foreground),
        );
        info.attached.saturating_sub(ours) > 0
    }

    /// Unloads `sid` now (no confirmation). Closing the current session opens a new one (chat mode).
    pub fn close_session(&mut self, sid: &str) {
        if self.close_armed.take().is_some() {
            // Drop the "press again to close" hint.
            self.hint = None;
        }
        if !self.connected {
            self.toasts.warn(NOT_CONNECTED);
            return;
        }
        let name = self.session_name(sid);
        self.req(
            Request::Close {
                session_id: sid.to_string(),
            },
            Pending::Closed { name },
        );
        // Optimistic: the server's session broadcast follows, but lists update right away.
        self.live.retain(|s| s.id != sid);
        if self.current.as_deref() == Some(sid) {
            self.perm = None;
            self.question = None;
            if self.mode == Mode::Chat {
                self.leave_closed_session(sid);
            } else {
                self.views.remove(sid);
                self.watch.inputs.remove(sid);
                self.watch.shell_modes.remove(sid);
                if self.watch.drag_tile.as_deref() == Some(sid) {
                    self.watch.drag_tile = None;
                }
                self.attaching.remove(sid);
                self.current = None;
            }
        } else if self.views.get(sid).is_some_and(|v| !v.foreground) {
            self.views.remove(sid);
            self.watch.inputs.remove(sid);
            self.watch.shell_modes.remove(sid);
            if self.watch.drag_tile.as_deref() == Some(sid) {
                self.watch.drag_tile = None;
            }
            self.attaching.remove(sid);
        }
        self.sync_watch();
        self.refresh_session_dialog();
    }

    /// The current session `sid` was closed: deselect to `current = None` without quitting
    /// or auto-creating a replacement session.
    fn leave_closed_session(&mut self, sid: &str) {
        self.views.remove(sid);
        self.current = None;
        self.exit_shell_mode();
        self.clear_selection();
    }
    /// `/cd [PATH]` on `sid` (or draft cwd when `None`); without a path opens the directory picker.
    pub fn change_dir(&mut self, sid: Option<&str>, args: &str) {
        let path = args.trim().trim_matches(|c| c == '"' || c == '\'');
        if path.is_empty() {
            self.open_cd(sid);
            return;
        }
        match sid {
            Some(s) => self.send_set_cwd(s, path),
            None => self.send_draft_cwd(path),
        }
    }

    fn send_set_cwd(&mut self, sid: &str, path: &str) {
        let path = path.trim().trim_matches(|c| c == '"' || c == '\'');
        if path.is_empty() {
            return;
        }
        self.req(
            Request::SetCwd {
                session_id: sid.to_string(),
                path: path.to_string(),
            },
            Pending::Cwd {
                sid: sid.to_string(),
            },
        );
    }

    fn send_draft_cwd(&mut self, path: &str) {
        let path = path.trim().trim_matches(|c| c == '"' || c == '\'');
        if path.is_empty() {
            return;
        }
        if !self.connected {
            self.apply_draft_cwd(path.to_string());
            return;
        }
        self.req(
            Request::ListDirs {
                path: path.to_string(),
                cwd: Some(self.cwd.clone()),
                include_files: false,
            },
            Pending::DraftCd,
        );
    }

    fn apply_draft_cwd(&mut self, path: String) {
        self.cwd = path;
        self.toasts
            .info(format!("Working directory: {}", text::tilde(&self.cwd)));
        self.dir_cache.clear();
        let cwd = self.current_cwd();
        if self.connected {
            self.req(
                Request::GetConfig { cwd },
                Pending::Config {
                    toast_warnings: true,
                },
            );
            self.mcp_inflight = false;
            self.refresh_mcp();
        }
        self.update_autocomplete();
    }
    /// Directory of the active session/tile (falls back to the frontend's).
    fn session_cwd(&self) -> String {
        self.current_cwd().unwrap_or_else(|| self.cwd.clone())
    }

    /// Closes `sid`, asking for a second confirmation when it is open in another frontend.
    /// `again` describes how to confirm (e.g. "run the command again").
    fn close_confirmed(&mut self, sid: &str, again: &str) {
        if self.attached_elsewhere(sid) && !self.close_is_armed(sid) {
            self.arm_close(
                sid,
                &format!(
                    "\"{}\" is open in another frontend — {again} to close it",
                    self.session_name(sid)
                ),
            );
            return;
        }
        self.close_session(sid);
    }

    fn close_is_armed(&self, sid: &str) -> bool {
        self.close_armed
            .as_ref()
            .is_some_and(|(s, t)| s == sid && t.elapsed() < CONFIRM_WINDOW)
    }

    fn arm_close(&mut self, sid: &str, hint: &str) {
        self.close_armed = Some((sid.to_string(), Instant::now()));
        self.set_hint_for(hint, theme::WARNING, CONFIRM_WINDOW);
    }

    /// `ctrl+x k`: close the current session (chat) or the focused tile (split view); press twice.
    pub fn close_key(&mut self) {
        let target = match self.mode {
            Mode::Chat => self.current.clone(),
            Mode::Watch => self.watch.focus.clone(),
        };
        let Some(sid) = target else {
            self.toasts.info("No session to close");
            return;
        };
        if self.mode == Mode::Chat && self.views.get(&sid).is_some_and(|v| v.closed) {
            // Already gone on the server: just move on to a fresh session.
            self.run_command(cmd::CLOSE, "");
            return;
        }
        if self.close_is_armed(&sid) {
            self.close_session(&sid);
            return;
        }
        let name = self.session_name(&sid);
        let extra = if self.attached_elsewhere(&sid) {
            " (open in another frontend)"
        } else {
            ""
        };
        self.arm_close(
            &sid,
            &format!("press ctrl+x k again to close \"{name}\"{extra}"),
        );
    }

    /// `/close-session` without args: picker of live sessions that stays open after each close.
    fn open_close_picker(&mut self) {
        let d = ListDialog::new(Kind::CloseSessions, "Close sessions", Vec::new())
            .with_hint("enter close · tab filter · stays open to close more · esc done");
        self.dialog = Some(Dialog::List(d));
        self.refresh_session_dialog();
        self.req(Request::ListSessions, Pending::LiveList);
    }

    /// Enter / ctrl+d in the close picker.
    fn close_from_picker(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_mut() else {
            return;
        };
        let Some(key) = d.selected().map(|i| i.key.clone()) else {
            return;
        };
        let Some(sid) = key.strip_prefix(LIVE_KEY_PREFIX).map(str::to_string) else {
            return;
        };
        let elsewhere = self.attached_elsewhere(&sid);
        let Some(Dialog::List(d)) = self.dialog.as_mut() else {
            return;
        };
        if elsewhere && d.confirm.as_deref() != Some(key.as_str()) {
            d.confirm = Some(key);
            d.confirm_text = "open in another frontend · enter again to close ".into();
            return;
        }
        d.confirm = None;
        self.close_session(&sid);
    }

    pub fn undo_message(&mut self, sid: &str, msg: &str) {
        let Some(v) = self.views.get(sid) else { return };
        let Some(i) = v.find(msg) else { return };
        let count = v.slots.len() - i;
        let busy = v.is_busy();
        let armed = self
            .undo_armed
            .as_ref()
            .is_some_and(|(m, t)| m == msg && t.elapsed() < CONFIRM_WINDOW);
        if busy && !armed {
            self.undo_armed = Some((msg.to_string(), Instant::now()));
            self.set_hint_for(
                "click ↶ undo again to undo (aborts the running turn)",
                theme::WARNING,
                CONFIRM_WINDOW,
            );
            return;
        }
        self.undo_armed = None;
        self.hint = None;
        self.clear_selection();
        let p = Pending::Undo {
            sid: sid.to_string(),
            count: Some(count),
        };
        self.req(
            Request::Undo {
                session_id: sid.to_string(),
                message_id: Some(msg.to_string()),
            },
            p,
        );
    }

    // -----------------------------------------------------------------------
    // Terminal title / exit screen
    // -----------------------------------------------------------------------

    /// Terminal window title: `R|<session title>` (name when untitled), `R|watch (N)` in split view.
    pub fn terminal_title(&self) -> String {
        if self.mode == Mode::Watch {
            return format!("R|watch ({})", self.tile_ids().len());
        }
        let label = self
            .cur()
            .map(|v| {
                let t = text::one_line(&v.info.title);
                if t.is_empty() { v.info.name.clone() } else { t }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ref".into());
        format!("R|{}", text::truncate(&label, TITLE_MAX_CHARS))
    }

    /// Text printed to stdout after the TUI exits (logo + resume hint), if any.
    pub fn epilogue(&self) -> Option<String> {
        let v = self.cur()?;
        let resumable = !v.is_empty();
        Some(epilogue_text(&v.info, resumable))
    }

    pub fn model_display(&self, m: &ModelRef) -> String {
        find_model(self.models(), m)
            .map(|x| x.name.clone())
            .unwrap_or_else(|| m.model.clone())
    }

    pub fn provider_display(&self, m: &ModelRef) -> String {
        self.config
            .as_ref()
            .and_then(|c| c.models.iter().find(|x| x.provider == m.provider))
            .map(|x| {
                if x.provider_name.is_empty() {
                    x.provider.clone()
                } else {
                    x.provider_name.clone()
                }
            })
            .unwrap_or_else(|| m.provider.clone())
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------
    pub fn run_command(&mut self, name: &str, args: &str) {
        let target = self.active_target_session();
        self.run_command_for(target, name, args);
    }

    pub fn run_command_for(&mut self, target_sid: Option<String>, name: &str, args: &str) {
        let args = args.trim();
        let canonical = find_cmd(name).map_or(name, |c| c.name);
        // Commands needing backend data run once it has arrived (startup race).
        let needs_config = matches!(canonical, cmd::MODELS | cmd::AGENTS | cmd::VARIANT);
        let needs_session = matches!(
            canonical,
            cmd::COMPACT
                | cmd::UNDO
                | cmd::RENAME
                | cmd::EXPORT
                | cmd::COPY
                | cmd::RETRY
                | cmd::PEERS
                | cmd::PEERS_NAME
                | cmd::PEERS_INBOX
                | cmd::PEERS_OUTBOX
        );
        let session_ready = match target_sid.as_deref().and_then(|id| self.views.get(id)) {
            Some(v) if canonical == cmd::COPY => v.loaded,
            Some(_) => true,
            None => false,
        };
        if (needs_config && self.config.is_none()) || (needs_session && !session_ready) {
            if self.connected || !self.ever_connected {
                self.deferred = Some((
                    target_sid,
                    canonical.to_string(),
                    args.to_string(),
                    Instant::now(),
                ));
                return;
            }
            self.toasts.warn(NOT_CONNECTED);
            return;
        }
        match canonical {
            cmd::NEW => {
                self.create_session(None);
            }
            cmd::SESSIONS => {
                if args.is_empty() {
                    self.open_sessions();
                } else {
                    self.switch_session_by_query(args, false);
                }
            }
            cmd::MODELS => {
                if args.is_empty() {
                    self.open_models_for(target_sid, None);
                } else {
                    self.switch_model_by_query(target_sid, args);
                }
            }
            cmd::AGENTS => {
                if args.is_empty() {
                    self.open_agents_for(target_sid, None);
                } else {
                    self.switch_agent_by_query(target_sid, args);
                }
            }
            cmd::VARIANT => {
                if args.is_empty() {
                    self.open_variants_for(target_sid);
                } else {
                    self.set_variant_by_query(target_sid.as_deref(), args);
                }
            }
            cmd::COMPACT => {
                if let Some(sid) = target_sid {
                    self.req(
                        Request::Compact { session_id: sid },
                        Pending::Simple { ok: String::new() },
                    );
                }
            }
            cmd::UNDO => {
                if let Some(sid) = target_sid {
                    let p = Pending::Undo {
                        sid: sid.clone(),
                        count: None,
                    };
                    self.req(
                        Request::Undo {
                            session_id: sid,
                            message_id: None,
                        },
                        p,
                    );
                }
            }
            cmd::RENAME => {
                if args.is_empty() {
                    if self.mode == Mode::Watch
                        && let Some(sid) = target_sid
                    {
                        self.watch
                            .inputs
                            .entry(sid)
                            .or_default()
                            .set_text(&format!("/{} ", cmd::RENAME));
                    } else {
                        self.editor.set_text(&format!("/{} ", cmd::RENAME));
                    }
                } else if let Some(sid) = target_sid {
                    self.req(
                        Request::Rename {
                            session_id: sid,
                            name: args.to_string(),
                        },
                        Pending::Simple {
                            ok: format!("Renamed to \"{args}\""),
                        },
                    );
                }
            }
            cmd::PEERS | cmd::PEERS_NAME | cmd::PEERS_INBOX | cmd::PEERS_OUTBOX => {
                if let Some(sid) = target_sid {
                    let title = match canonical {
                        cmd::PEERS => "Peers",
                        cmd::PEERS_NAME => "Peer name",
                        cmd::PEERS_INBOX => "Peer inbox",
                        _ => "Peer outbox",
                    };
                    self.req(
                        Request::PeerCommand {
                            session_id: sid,
                            command: canonical.to_string(),
                            args: args.to_string(),
                        },
                        Pending::Text {
                            title: title.to_string(),
                        },
                    );
                }
            }
            cmd::CLOSE => match target_sid {
                Some(sid) if self.views.get(&sid).is_some_and(|v| !v.closed) => {
                    self.close_session(&sid)
                }
                Some(sid) => self.leave_closed_session(&sid),
                None => self.toasts.info("No session to close"),
            },
            cmd::DETACH => self.detach_current(),
            cmd::RETRY => {
                if let Some(sid) = target_sid {
                    self.retry_now(&sid);
                }
            }
            cmd::STASH => {
                let name = (!args.is_empty()).then(|| args.to_string());
                self.stash_all(name);
            }
            cmd::STASHED => self.open_stash(),
            cmd::BACKEND => self.open_backend_with_args(args),
            cmd::CD => {
                self.change_dir(target_sid.as_deref(), args);
            }
            cmd::CLOSE_SESSION => self.close_by_name(args),
            cmd::EXPORT => {
                if let Some(sid) = target_sid {
                    let base = self.cwd_of(Some(&sid));
                    let p = if args.is_empty() {
                        format!("ref-session-{sid}.md")
                    } else {
                        args.trim_matches(|c| c == '"' || c == '\'').to_string()
                    };
                    let p = if is_tilde_or_rooted_path(&p) {
                        p
                    } else {
                        std::path::Path::new(&base)
                            .join(&p)
                            .to_string_lossy()
                            .into_owned()
                    };
                    self.req(
                        Request::Export {
                            session_id: sid,
                            path: p,
                        },
                        Pending::Export,
                    );
                }
            }
            cmd::COPY => {
                let raw = target_sid
                    .as_deref()
                    .and_then(|id| self.views.get(id))
                    .and_then(|v| {
                        v.materialized_iter()
                            .rev()
                            .find(|m| {
                                m.role == ref_proto::Role::Assistant && !m.raw_text().is_empty()
                            })
                            .map(|m| m.raw_text())
                    });
                match raw {
                    Some(r) => self.copy_text(&r, "last assistant message"),
                    None => self.toasts.warn("No assistant message yet"),
                }
            }
            cmd::THINKING => self.toggle_thinking(),
            cmd::DETAILS => self.toggle_details(),
            cmd::WATCH => self.toggle_watch(),
            cmd::SIDEBAR => {
                self.sidebar = !self.sidebar;
                if self.sidebar {
                    self.refresh_mcp();
                }
            }
            cmd::RELOAD => {
                let cwd = self.current_cwd();
                self.req(Request::ReloadConfig { cwd }, Pending::ReloadConfig);
            }
            cmd::EDITOR => self.run_editor = true,
            cmd::HELP => self.open_help(),
            cmd::MCP => {
                let select = (!args.is_empty()).then_some(args);
                self.open_mcp(select);
            }
            cmd::EXIT => self.quit = true,
            _ => {}
        }
    }

    pub fn retry_now(&mut self, sid: &str) {
        self.req(
            Request::RetryNow {
                session_id: sid.to_string(),
            },
            Pending::RetryNow,
        );
    }

    pub fn stash_all(&mut self, name: Option<String>) {
        self.req(Request::StashAll { name }, Pending::StashAll);
    }

    pub fn open_stash(&mut self) {
        let mut d = StashDialog::new();
        d.sync(&self.groups);
        self.dialog = Some(Dialog::Stash(d));
        if self.connected {
            self.req(Request::ListGroups, Pending::Groups { ok: None });
        }
    }

    pub fn sync_stash_dialog(&mut self) {
        if let Some(Dialog::Stash(d)) = self.dialog.as_mut() {
            d.sync(&self.groups);
        }
    }

    pub fn handle_stash_command(&mut self, command: StashCommand) {
        match command {
            StashCommand::Close => self.dialog = None,
            StashCommand::Refresh => {
                self.req(Request::ListGroups, Pending::Groups { ok: None });
            }
            StashCommand::StashAll { name } => self.stash_all(name),
            StashCommand::CreateGroup { name } => {
                self.req(
                    Request::CreateGroup {
                        name,
                        session_ids: Vec::new(),
                        stash_live: false,
                    },
                    Pending::CreateGroup,
                );
            }
            StashCommand::RenameGroup { group_id, name } => {
                self.req(
                    Request::RenameGroup { group_id, name },
                    Pending::Groups {
                        ok: Some("Renamed group".into()),
                    },
                );
            }
            StashCommand::SetLocked { group_id, locked } => {
                self.req(
                    Request::SetGroupLocked { group_id, locked },
                    Pending::Groups {
                        ok: Some(if locked {
                            "Locked group".into()
                        } else {
                            "Unlocked group".into()
                        }),
                    },
                );
            }
            StashCommand::DeleteGroup { group_id } => {
                self.req(
                    Request::DeleteGroup { group_id },
                    Pending::Groups {
                        ok: Some("Deleted group".into()),
                    },
                );
            }
            StashCommand::RestoreGroup { group_id } => {
                self.req(Request::RestoreGroup { group_id }, Pending::RestoreGroup);
            }
            StashCommand::RestoreMember {
                group_id,
                session_id,
            } => {
                self.req(
                    Request::RestoreGroupMember {
                        group_id,
                        session_id,
                    },
                    Pending::RestoreGroupMember,
                );
            }
            StashCommand::RemoveMember {
                group_id,
                session_id,
            } => {
                self.req(
                    Request::RemoveFromGroup {
                        group_id,
                        session_id,
                    },
                    Pending::Groups {
                        ok: Some("Removed from group".into()),
                    },
                );
            }
            StashCommand::MoveMember {
                from_group_id,
                to_group_id,
                session_id,
                position,
            } => {
                self.req(
                    Request::MoveGroupMember {
                        from_group_id: Some(from_group_id),
                        to_group_id,
                        session_id,
                        position,
                        stash_live: true,
                    },
                    Pending::Groups {
                        ok: Some("Moved member".into()),
                    },
                );
            }
            StashCommand::MergeGroups {
                source_group_id,
                target_group_id,
            } => {
                self.req(
                    Request::MergeGroups {
                        source_group_id,
                        target_group_id,
                    },
                    Pending::Groups {
                        ok: Some("Merged groups".into()),
                    },
                );
            }
            StashCommand::AddToGroup {
                group_id,
                session_id,
            } => {
                self.req(
                    Request::AddToGroup {
                        group_id,
                        session_id,
                        position: None,
                        stash_live: true,
                    },
                    Pending::Groups {
                        ok: Some("Added session to group".into()),
                    },
                );
            }
        }
    }

    pub fn backend_summary(&self) -> String {
        let src = match self.backend_source {
            BackendSource::Cli => "CLI override",
            BackendSource::Saved => "saved default",
            BackendSource::Auto => "local auto",
            BackendSource::Temporary => "temporary",
        };
        match &self.backend_target {
            BackendTarget::Auto => format!("Local auto-discovery ({src})"),
            BackendTarget::Explicit { url, .. } => format!("{url} ({src})"),
            BackendTarget::Mock => format!("Mock backend ({src})"),
        }
    }

    pub fn open_backend(&mut self) {
        let (url, token) = match &self.backend_target {
            BackendTarget::Explicit { url, token } => (Some(url.as_str()), token.as_deref()),
            _ => (None, None),
        };
        self.dialog = Some(Dialog::Backend(BackendDialog::new(
            url,
            token,
            self.backend_summary(),
        )));
    }

    fn open_backend_with_args(&mut self, args: &str) {
        if args.is_empty() {
            self.open_backend();
            return;
        }
        let tokens = text::split_shell_words(args);
        if tokens.is_empty() {
            self.open_backend();
            return;
        }
        if matches!(tokens[0].as_str(), "auto" | "local") {
            self.switch_backend(BackendTarget::Auto, BackendSource::Temporary);
            return;
        }
        let url = tokens[0].trim().to_string();
        if let Err(e) = ref_client::validate_explicit_url(&url) {
            self.toasts.error(e.to_string());
            return;
        }
        let token = tokens
            .get(1)
            .and_then(|t| crate::prefs::normalize_token(Some(t)));
        self.switch_backend(
            BackendTarget::Explicit { url, token },
            BackendSource::Temporary,
        );
    }

    pub fn handle_backend_action(&mut self, action: BackendAction) {
        match action {
            BackendAction::Close => self.dialog = None,
            BackendAction::ConnectTemporary { url, token } => {
                self.dialog = None;
                self.switch_backend(
                    BackendTarget::Explicit { url, token },
                    BackendSource::Temporary,
                );
            }
            BackendAction::SaveDefault { url, token } => {
                let prefs = TuiPrefs {
                    backend_url: Some(url.clone()),
                    backend_token: token.clone(),
                };
                if let Err(e) = crate::prefs::save_prefs(&prefs) {
                    self.toasts.error(format!("Cannot save preferences: {e}"));
                    return;
                }
                self.dialog = None;
                self.toasts.info("Saved default backend");
                self.switch_backend(BackendTarget::Explicit { url, token }, BackendSource::Saved);
            }
            BackendAction::RestoreLocal => {
                if let Err(e) = crate::prefs::save_prefs(&TuiPrefs::default()) {
                    self.toasts.error(format!("Cannot save preferences: {e}"));
                    return;
                }
                self.dialog = None;
                self.toasts.info("Restored local backend default");
                self.switch_backend(BackendTarget::Auto, BackendSource::Auto);
            }
        }
    }

    pub fn switch_backend(&mut self, target: BackendTarget, source: BackendSource) {
        let label = match &target {
            BackendTarget::Auto => "local backend".to_string(),
            BackendTarget::Explicit { url, .. } => url.clone(),
            BackendTarget::Mock => "mock backend".to_string(),
        };
        self.current = None;
        self.views.clear();
        self.live.clear();
        self.history_list.clear();
        self.history_loaded = false;
        self.history_inflight = false;
        self.history_filtered = None;
        self.sessions_due = None;
        self.attaching.clear();
        self.groups.clear();
        self.groups_loaded = false;
        self.config = None;
        self.pending.clear();
        self.perm = None;
        self.question = None;
        self.mcp.clear();
        self.mcp_loaded = false;
        self.mcp_fallback = false;
        self.mcp_inflight = false;
        self.mcp_poll_at = None;
        self.dir_cache.clear();
        self.dirs_inflight.clear();
        self.bin_cache.clear();
        self.bins_inflight.clear();
        self.shell_complete_cache.clear();
        self.shell_complete_inflight.clear();
        self.shell_complete_errors.clear();
        self.files_inflight = false;
        self.files_want = None;
        self.files_due = None;
        self.deferred = None;
        self.watch = Watch::default();
        self.mode = Mode::Chat;
        self.clear_autocomplete();
        self.exit_shell_mode();
        self.clear_selection();
        self.connected = false;
        self.ever_connected = false;
        self.refused = None;
        self.backend_too_old = None;
        self.net_status = Some(format!("Connecting to {label}…"));
        self.backend_target = target.clone();
        self.backend_source = source;
        self.client.switch_target(target);
    }

    pub fn cycle_watch_filter(&mut self) {
        let parents = self.watch_direct_parents();
        match &self.watch.filter {
            CreatorFilter::All => {
                let focus_parent = self
                    .watch
                    .focus
                    .as_deref()
                    .and_then(|id| self.live.iter().find(|s| s.id == id))
                    .and_then(|s| {
                        crate::ui::split::direct_parent(s, &self.live, &self.history_list)
                    });
                self.watch.pending_parent = focus_parent;
                self.watch.filter = CreatorFilter::UserCreated;
            }
            CreatorFilter::UserCreated => {
                let picked = self
                    .watch
                    .pending_parent
                    .take()
                    .or_else(|| parents.into_iter().next());
                self.watch.filter = match picked {
                    Some((id, label)) => CreatorFilter::ByCreator { id, label },
                    None => CreatorFilter::All,
                };
            }
            CreatorFilter::ByCreator { id, .. } => {
                let pos = parents.iter().position(|(pid, _)| pid == id);
                let next = pos.and_then(|i| parents.get(i + 1).cloned());
                self.watch.filter = match next {
                    Some((pid, label)) => CreatorFilter::ByCreator { id: pid, label },
                    None => CreatorFilter::All,
                };
            }
        }
        self.watch.page = 0;
        self.sync_watch();
    }

    pub fn watch_filter_pill(&self) -> (String, bool) {
        match &self.watch.filter {
            CreatorFilter::All => (" All ".to_string(), false),
            CreatorFilter::UserCreated => (" User-created ".to_string(), true),
            CreatorFilter::ByCreator { label, .. } => {
                (format!(" By {} ", text::truncate(label, 16)), true)
            }
        }
    }

    fn watch_direct_parents(&self) -> Vec<(String, String)> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for s in &self.live {
            if !s.live || s.attached != 0 || self.current.as_deref() == Some(s.id.as_str()) {
                continue;
            }
            if let Some((pid, label)) =
                crate::ui::split::direct_parent(s, &self.live, &self.history_list)
                && seen.insert(pid.clone())
            {
                out.push((pid, label));
            }
        }
        out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    fn apply_model(
        &mut self,
        sid: Option<&str>,
        mut m: ModelRef,
        explicit_variant: Option<String>,
    ) {
        if let Some(v) = explicit_variant {
            m.variant = (!v.is_empty()).then_some(v);
        } else {
            let cur_var = match sid.and_then(|id| self.views.get(id)) {
                Some(v) => v.info.model.variant.clone(),
                None => self
                    .draft
                    .variant
                    .as_deref()
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
                    .or_else(|| self.draft.model.as_ref().and_then(|dm| dm.variant.clone())),
            };
            let supports = find_model(self.models(), &m)
                .is_some_and(|x| cur_var.as_ref().is_some_and(|v| x.variants.contains(v)));
            if supports {
                m.variant = cur_var;
            }
        }
        let name = self.model_display(&m);
        let Some(sid) = sid else {
            self.draft.variant = Some(m.variant.clone().unwrap_or_default());
            self.draft.model = Some(m);
            self.toasts.info(format!("Model: {name}"));
            return;
        };
        if let Some(v) = self.views.get_mut(sid) {
            v.info.model = m.clone();
        }
        self.req(
            Request::SetModel {
                session_id: sid.to_string(),
                model: m,
            },
            Pending::Simple {
                ok: format!("Model: {name}"),
            },
        );
    }

    fn apply_agent(&mut self, sid: Option<&str>, agent: &str, toast: bool) {
        let agent_model = self
            .config
            .as_ref()
            .and_then(|c| c.agents.iter().find(|a| a.name == agent))
            .and_then(|a| a.model.clone());
        let Some(sid) = sid else {
            self.draft.agent = Some(agent.to_string());
            if let Some(m) = agent_model {
                self.draft.model = Some(m);
            }
            if toast {
                self.toasts.info(format!("Agent: {agent}"));
            }
            return;
        };
        if let Some(v) = self.views.get_mut(sid) {
            v.info.agent = agent.to_string();
            if let Some(m) = agent_model {
                v.info.model = m;
            }
        }
        let ok = if toast {
            format!("Agent: {agent}")
        } else {
            String::new()
        };
        self.req(
            Request::SetAgent {
                session_id: sid.to_string(),
                agent: agent.to_string(),
            },
            Pending::Simple { ok },
        );
    }
    fn switch_model_by_query(&mut self, target_sid: Option<String>, raw_args: &str) {
        let (q, explicit_var) = match raw_args.split_once('#') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (raw_args.trim(), None),
        };
        let lq = q.to_lowercase();
        let models = self.models().to_vec();
        let exact_key: Vec<&ref_proto::ModelInfo> = models
            .iter()
            .filter(|m| format!("{}/{}", m.provider, m.id).eq_ignore_ascii_case(q))
            .collect();
        let matched: Vec<&ref_proto::ModelInfo> = if !exact_key.is_empty() {
            exact_key
        } else {
            let exact_id: Vec<&ref_proto::ModelInfo> = models
                .iter()
                .filter(|m| m.id.eq_ignore_ascii_case(q) || m.name.eq_ignore_ascii_case(q))
                .collect();
            if !exact_id.is_empty() {
                exact_id
            } else {
                let sub: Vec<&ref_proto::ModelInfo> = models
                    .iter()
                    .filter(|m| {
                        m.id.to_lowercase().contains(&lq)
                            || m.name.to_lowercase().contains(&lq)
                            || format!("{}/{}", m.provider, m.id)
                                .to_lowercase()
                                .contains(&lq)
                    })
                    .collect();
                if !sub.is_empty() { sub } else { Vec::new() }
            }
        };
        match matched.as_slice() {
            [m] => {
                let var = match explicit_var {
                    Some(v)
                        if v.is_empty()
                            || v.eq_ignore_ascii_case("default")
                            || v.eq_ignore_ascii_case("none") =>
                    {
                        Some(String::new())
                    }
                    Some(v) => match m.variants.iter().find(|mv| mv.eq_ignore_ascii_case(v)) {
                        Some(found) => Some(found.clone()),
                        None => {
                            self.toasts
                                .warn(format!("Model {} has no variant \"{v}\"", m.id));
                            return;
                        }
                    },
                    None => None,
                };
                let mref = ModelRef {
                    provider: m.provider.clone(),
                    model: m.id.clone(),
                    variant: None,
                };
                self.apply_model(target_sid.as_deref(), mref, var);
            }
            [] => self.toasts.warn(format!("No model matching \"{q}\"")),
            _ => self.open_models_for(target_sid, Some(q)),
        }
    }

    fn switch_agent_by_query(&mut self, target_sid: Option<String>, q: &str) {
        let lq = q.trim().to_lowercase();
        let agents = self.agents().to_vec();
        if let Some(a) = agents.iter().find(|a| a.name.eq_ignore_ascii_case(&lq)) {
            self.apply_agent(target_sid.as_deref(), &a.name, true);
            return;
        }
        let pfx: Vec<&ref_proto::AgentInfo> = agents
            .iter()
            .filter(|a| a.name.to_lowercase().starts_with(&lq))
            .collect();
        match pfx.as_slice() {
            [a] => {
                self.apply_agent(target_sid.as_deref(), &a.name, true);
            }
            [] => self.toasts.warn(format!("Unknown agent \"{}\"", q.trim())),
            _ => self.open_agents_for(target_sid, Some(q.trim())),
        }
    }

    fn set_variant_by_query(&mut self, target_sid: Option<&str>, raw: &str) {
        let q = raw.trim();
        let is_default = q.eq_ignore_ascii_case("default") || q.eq_ignore_ascii_case("none");
        let Some(sid) = target_sid else {
            if is_default {
                self.apply_draft_variant(None);
                return;
            }
            let cur_model = self.effective_draft_model();
            let variant = if let Some(m) = cur_model
                .as_ref()
                .and_then(|m| find_model(self.models(), m))
            {
                if m.variants.is_empty() {
                    self.toasts.info("This model has no variants");
                    return;
                }
                match m.variants.iter().find(|v| v.eq_ignore_ascii_case(q)) {
                    Some(v) => v.clone(),
                    None => {
                        self.toasts.warn(format!(
                            "Unknown variant \"{q}\" (available: default, {})",
                            m.variants.join(", ")
                        ));
                        return;
                    }
                }
            } else {
                q.to_string()
            };
            self.apply_draft_variant(Some(variant));
            return;
        };
        if is_default {
            self.req(
                Request::SetVariant {
                    session_id: sid.to_string(),
                    variant: Some(String::new()),
                },
                Pending::Variant,
            );
            return;
        }
        let cur_model = self.views.get(sid).map(|v| v.info.model.clone());
        let variant = if let Some(m) = cur_model
            .as_ref()
            .and_then(|m| find_model(self.models(), m))
        {
            if m.variants.is_empty() {
                self.toasts.info("This model has no variants");
                return;
            }
            match m.variants.iter().find(|v| v.eq_ignore_ascii_case(q)) {
                Some(v) => v.clone(),
                None => {
                    self.toasts.warn(format!(
                        "Unknown variant \"{q}\" (available: default, {})",
                        m.variants.join(", ")
                    ));
                    return;
                }
            }
        } else {
            q.to_string()
        };
        self.req(
            Request::SetVariant {
                session_id: sid.to_string(),
                variant: Some(variant),
            },
            Pending::Variant,
        );
    }

    fn switch_to_session(&mut self, sid: &str) {
        if self.current.as_deref() != Some(sid) {
            self.attach_fg(sid);
        } else if self.mode == Mode::Watch {
            self.resume_current();
        }
    }

    fn switch_session_by_query(&mut self, raw: &str, history_checked: bool) {
        let q = raw.trim().trim_matches(|c| c == '"' || c == '\'');
        let mut all: Vec<SessionInfo> = self.live.iter().filter(|s| s.live).cloned().collect();
        let live_ids: HashSet<String> = all.iter().map(|s| s.id.clone()).collect();
        for h in &self.history_list {
            if !live_ids.contains(&h.id) {
                all.push(h.clone());
            }
        }
        if let Some(s) = all.iter().find(|s| s.id == q || s.name == q) {
            let sid = s.id.clone();
            self.switch_to_session(&sid);
            return;
        }
        let lq = q.to_lowercase();
        let ci_exact: Vec<&SessionInfo> = all
            .iter()
            .filter(|s| s.id.to_lowercase() == lq || s.name.to_lowercase() == lq)
            .collect();
        if let [s] = ci_exact.as_slice() {
            let sid = s.id.clone();
            self.switch_to_session(&sid);
            return;
        }
        if !history_checked && !self.history_loaded && self.connected {
            if let Some(p) = self
                .pending
                .values_mut()
                .find(|p| matches!(p, Pending::HistoryBg | Pending::SwitchSession { .. }))
            {
                *p = Pending::SwitchSession {
                    query: q.to_string(),
                };
                return;
            }
            self.history_inflight = true;
            self.req(
                Request::ListHistory {
                    cwd: None,
                    limit: Some(HISTORY_LIST_LIMIT),
                    creator: None,
                    query: None,
                },
                Pending::SwitchSession {
                    query: q.to_string(),
                },
            );
            return;
        }
        let sub: Vec<&SessionInfo> = if !ci_exact.is_empty() {
            ci_exact
        } else {
            all.iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&lq)
                        || s.id.to_lowercase().contains(&lq)
                        || s.title.to_lowercase().contains(&lq)
                })
                .collect()
        };
        match sub.as_slice() {
            [s] => {
                let sid = s.id.clone();
                self.switch_to_session(&sid);
            }
            [] => self.toasts.warn(format!("No session matching \"{q}\"")),
            _ => self.open_sessions_filtered(Some(q)),
        }
    }

    /// `/close-session [s1 s2 ...]`: picker without args, else closes one or more live sessions.
    fn close_by_name(&mut self, args: &str) {
        if args.is_empty() {
            self.open_close_picker();
            return;
        }
        let tokens = text::split_shell_words(args);
        if tokens.is_empty() {
            self.open_close_picker();
            return;
        }
        if tokens.len() == 1 {
            match match_session(&self.live, &tokens[0]) {
                Ok(s) => {
                    let sid = s.id.clone();
                    self.close_confirmed(&sid, "run the command again");
                }
                Err(e) => self.toasts.warn(e),
            }
            return;
        }
        let mut sids = Vec::new();
        for tok in &tokens {
            match match_session(&self.live, tok) {
                Ok(s) => {
                    if !sids.contains(&s.id) {
                        sids.push(s.id.clone());
                    }
                }
                Err(e) => self.toasts.warn(e),
            }
        }
        for sid in sids {
            self.close_confirmed(&sid, "run the command again");
        }
    }

    pub fn toggle_watch(&mut self) {
        match self.mode {
            Mode::Chat => {
                self.mode = Mode::Watch;
                self.set_watch_focus(None);
                self.watch.drag_tile = None;
                self.clear_autocomplete();
                self.clear_selection();
                // Subscribes to all sessions first, so detaching the current one loses no events.
                self.sync_watch();
                self.background_current();
            }
            Mode::Watch => {
                if self.current.is_some() {
                    self.resume_current();
                } else {
                    self.toasts
                        .info("No foreground session — use ctrl+o to zoom a tile or /new");
                }
            }
        }
    }

    /// Entering split view: the current session becomes background (it stays `current`, so
    /// leaving split view returns to it). Its events keep arriving through `WatchAll`.
    fn background_current(&mut self) {
        let Some(sid) = self.current.clone() else {
            return;
        };
        let Some(v) = self.views.get_mut(&sid).filter(|v| v.foreground) else {
            return;
        };
        v.foreground = false;
        // Optimistic, so its tile shows at once; the backend's session broadcast follows.
        v.info.attached = v.info.attached.saturating_sub(1);
        if let Some(i) = self.live.iter_mut().find(|s| s.id == sid) {
            i.attached = i.attached.saturating_sub(1);
        }
        if self.connected {
            self.client.notify(Request::Detach { session_id: sid });
        }
    }

    /// Leaving split view: back to the current session, attached as foreground again.
    fn resume_current(&mut self) {
        let Some(sid) = self.current.clone() else {
            return;
        };
        self.mode = Mode::Chat;
        self.clear_selection();
        self.clear_autocomplete();
        match self.views.get_mut(&sid) {
            None => self.attach_fg(&sid),
            Some(v) if !v.foreground => {
                v.foreground = true;
                v.info.attached += 1;
                if let Some(i) = self.live.iter_mut().find(|s| s.id == sid) {
                    i.attached += 1;
                }
                self.send_attach(sid, true);
            }
            Some(_) => {}
        }
        self.sync_watch();
    }

    /// Zoom a tile: attach it as foreground.
    pub fn zoom(&mut self, sid: &str) {
        let sid = sid.to_string();
        self.clear_selection();
        self.clear_autocomplete();
        if let Some(v) = self.views.get_mut(&sid) {
            v.foreground = true;
        }
        self.attach_fg(&sid);
        self.sync_watch();
    }

    /// Enter in the main editor.
    pub fn submit(&mut self) {
        let raw = self.editor.text.clone();
        let trimmed = raw.trim();
        if trimmed.is_empty() && self.editor.images.is_empty() {
            return;
        }
        self.clear_autocomplete();
        if self.shell_mode {
            let cmd = trimmed.to_string();
            self.push_history(&format!("!{cmd}"));
            self.editor.clear();
            self.exit_shell_mode();
            match self.current.clone() {
                Some(sid) if self.views.get(&sid).is_some_and(|v| !v.closed) => {
                    self.req(
                        Request::Shell {
                            session_id: sid,
                            command: cmd,
                        },
                        Pending::Simple { ok: String::new() },
                    );
                }
                _ => self.create_session(Some(PostCreate::Shell(cmd))),
            }
            return;
        }
        if matches!(trimmed, "exit" | "quit" | ":q") {
            self.quit = true;
            return;
        }
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let Some(c) = find_cmd(name) {
                self.push_history(trimmed);
                self.editor.clear();
                self.run_command(c.name, args);
                return;
            }
            if let Some(custom) = find_custom_cmd(self.config.as_ref(), name) {
                let cname = custom.name.clone();
                self.push_history(trimmed);
                let sub = self.editor.submit();
                let args = sub
                    .text
                    .trim()
                    .strip_prefix('/')
                    .and_then(|r| r.split_once(char::is_whitespace))
                    .map(|x| x.1.to_string())
                    .unwrap_or_default();
                let sid = self.current.clone();
                self.expand_custom_command(sid, &cname, &args);
                return;
            }
            if self.editor.images.is_empty()
                && !raw.contains('\n')
                && is_single_slash_token(trimmed)
            {
                self.warn_unknown_command(name);
                return;
            }
        }
        self.push_history(&raw);
        let sub = self.editor.submit();
        let images = sub.images.into_iter().map(|(i, _)| i).collect();
        self.submit_prompt_text(sub.text, images);
    }

    /// Submits the input of focused split-view tile `tile`. Returns true if handled as a command/shell/typo.
    pub fn submit_tile_command(&mut self, tile: &str) -> bool {
        let (raw, has_images) = {
            let Some(ed) = self.watch.inputs.get(tile) else {
                return false;
            };
            (ed.text.clone(), !ed.images.is_empty())
        };
        let in_shell = self.watch.shell_modes.contains(tile);
        let trimmed = raw.trim();
        let shell_cmd = if in_shell {
            Some(trimmed)
        } else {
            trimmed.strip_prefix('!').map(str::trim)
        };
        if let Some(cmd_str) = shell_cmd
            && !has_images
        {
            if cmd_str.is_empty() {
                return in_shell;
            }
            let cmd_str = cmd_str.to_string();
            self.push_history(&format!("!{cmd_str}"));
            if let Some(ed) = self.watch.inputs.get_mut(tile) {
                ed.clear();
            }
            self.exit_watch_shell_mode(tile);
            self.clear_autocomplete();
            self.clear_shell_complete_cache();
            self.req(
                Request::Shell {
                    session_id: tile.to_string(),
                    command: cmd_str,
                },
                Pending::Simple { ok: String::new() },
            );
            return true;
        }
        let Some(rest) = trimmed.strip_prefix('/') else {
            return false;
        };
        let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if let Some(c) = find_cmd(name) {
            self.clear_autocomplete();
            self.push_history(trimmed);
            if let Some(ed) = self.watch.inputs.get_mut(tile) {
                ed.clear();
            }
            let args = args.to_string();
            self.run_command_for(Some(tile.to_string()), c.name, &args);
            return true;
        }
        if let Some(custom) = find_custom_cmd(self.config.as_ref(), name) {
            let cname = custom.name.clone();
            self.clear_autocomplete();
            self.push_history(trimmed);
            let args = self
                .watch
                .inputs
                .get_mut(tile)
                .map(|ed| ed.submit().text)
                .unwrap_or_default()
                .trim()
                .strip_prefix('/')
                .and_then(|r| r.split_once(char::is_whitespace))
                .map(|x| x.1.to_string())
                .unwrap_or_default();
            self.expand_custom_command(Some(tile.to_string()), &cname, &args);
            return true;
        }
        if !has_images && !raw.contains('\n') && is_single_slash_token(trimmed) {
            self.warn_unknown_command(name);
            return true;
        }
        false
    }

    fn expand_custom_command(&mut self, sid: Option<String>, name: &str, args: &str) {
        let cwd = self.cwd_of(sid.as_deref());
        self.req(
            Request::ExpandCommand {
                name: name.to_string(),
                args: args.to_string(),
                cwd,
            },
            Pending::Expand { sid },
        );
    }

    fn warn_unknown_command(&mut self, name: &str) {
        match self.suggest_command(name) {
            Some(s) => self
                .toasts
                .warn(format!("Unknown command /{name} — did you mean /{s}?")),
            None => self.toasts.warn(format!("Unknown command /{name}")),
        }
    }

    fn suggest_command(&self, typed: &str) -> Option<String> {
        let tl = typed.to_lowercase();
        let mut candidates: Vec<(String, String)> = Vec::new();
        for c in COMMANDS {
            candidates.push((c.name.to_string(), c.name.to_string()));
            for &al in c.aliases {
                candidates.push((al.to_string(), c.name.to_string()));
            }
        }
        if let Some(cfg) = &self.config {
            for c in &cfg.commands {
                candidates.push((c.name.clone(), c.name.clone()));
            }
        }
        // 1. Prefix match (prefer shortest extra suffix).
        if let Some((_, canon)) = candidates
            .iter()
            .filter(|(cand, _)| cand.to_lowercase().starts_with(&tl))
            .min_by_key(|(cand, _)| cand.len())
        {
            return Some(canon.clone());
        }
        // 2. Damerau-Levenshtein edit distance.
        let max_dist = if tl.chars().count() >= TYPO_LONG_WORD_CHARS {
            TYPO_MAX_DIST_LONG
        } else {
            TYPO_MAX_DIST_SHORT
        };
        candidates
            .into_iter()
            .map(|(cand, canon)| (text::edit_distance(&tl, &cand), cand.len(), canon))
            .filter(|(d, _, _)| *d <= max_dist)
            .min_by_key(|(d, len, _)| (*d, *len))
            .map(|(_, _, canon)| canon)
    }

    pub fn push_history(&mut self, t: &str) {
        if let Err(e) = self.history.push(t)
            && !self.history_warned
        {
            self.history_warned = true;
            self.toasts.warn(format!("Cannot save prompt history: {e}"));
        }
    }
    // -----------------------------------------------------------------------
    // Autocomplete
    // -----------------------------------------------------------------------

    pub fn update_autocomplete(&mut self) {
        self.update_autocomplete_inner(false);
    }

    /// Recomputes autocomplete after a user text edit, clearing any prior manual popup selection.
    pub fn update_autocomplete_on_edit(&mut self) {
        self.shell_tab_requested = false;
        self.shell_complete_errors.clear();
        if let Some(ac) = self.ac.as_mut() {
            ac.explicitly_selected = false;
            ac.sel = 0;
            ac.scroll = 0;
        }
        self.update_autocomplete_inner(false);
    }
    /// Explicit `Tab` press when `self.ac` is currently `None` (e.g. empty `!` shell prompt).
    pub fn trigger_tab_completion(&mut self) -> bool {
        self.suppress_ac = false;
        if self.active_shell_mode() {
            self.shell_tab_requested = true;
        }
        self.update_autocomplete_inner(true);
        self.ac.is_some()
    }

    fn update_autocomplete_inner(&mut self, explicit_tab: bool) {
        if self.suppress_ac {
            self.ac = None;
            self.shell_tab_requested = false;
            self.shell_complete_due = None;
            return;
        }
        if self.mode == Mode::Watch && self.watch.focus.is_none() {
            self.files_due = None;
            self.shell_complete_due = None;
            self.ac = None;
            return;
        }
        if self.active_shell_mode() {
            self.files_due = None;
            self.update_shell_autocomplete(explicit_tab);
            return;
        }
        self.shell_complete_due = None;
        let Some(ed) = self.active_editor() else {
            self.files_due = None;
            self.ac = None;
            return;
        };
        let cursor = ed.cursor.min(ed.text.len());
        let text_len = ed.text.len();
        let before = ed.text[..cursor].to_string();
        let mention = ed.mention();
        if let Some(rest) = before.strip_prefix('/')
            && !before.contains('\n')
        {
            if !rest.contains(char::is_whitespace) {
                if cursor == text_len {
                    let items = self.slash_completions(rest);
                    let prev = self.ac.take();
                    self.ac = Autocomplete::new(
                        AcKind::Slash,
                        " commands ",
                        items,
                        0,
                        false,
                        prev.as_ref(),
                    );
                } else {
                    self.ac = None;
                }
                self.files_due = None;
                return;
            }
            // Cursor is in arguments after `/<cmd> `.
            if self.update_slash_arg_autocomplete(&before) {
                self.files_due = None;
                return;
            }
        }
        if let Some((_, q)) = mention {
            if self.ac.as_ref().is_some_and(|a| a.kind != AcKind::File) {
                self.ac = None;
            }
            self.files_due = Some((Instant::now() + FILE_QUERY_DEBOUNCE, q));
            return;
        }
        self.files_due = None;
        self.ac = None;
    }

    fn update_slash_arg_autocomplete(&mut self, before: &str) -> bool {
        let rest = &before[1..];
        let Some(ws_idx) = rest.find(char::is_whitespace) else {
            return false;
        };
        let cmd_tok = &rest[..ws_idx];
        let Some(cmd_def) = find_cmd(cmd_tok) else {
            // Custom command: return false so `@<file>` mentions in custom command args work!
            return false;
        };
        let after_cmd_byte = 1 + ws_idx;
        let ws_len: usize = before[after_cmd_byte..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum();
        let arg_start = after_cmd_byte + ws_len;
        let arg_raw = &before[arg_start..];
        match cmd_def.name {
            cmd::CD => {
                let items = self.path_completion_items(arg_raw, false, false, false, None);
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " directories ",
                    items,
                    arg_start,
                    true,
                    prev.as_ref(),
                );
                true
            }
            cmd::EXPORT => {
                let default_file = self
                    .active_target_session()
                    .as_ref()
                    .map(|sid| format!("ref-session-{sid}.md"));
                let items = self.path_completion_items(
                    arg_raw,
                    true,
                    false,
                    false,
                    default_file.as_deref(),
                );
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " export path ",
                    items,
                    arg_start,
                    true,
                    prev.as_ref(),
                );
                true
            }
            cmd::VARIANT => {
                let items = self.variant_arg_completions(arg_raw);
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " variants ",
                    items,
                    arg_start,
                    false,
                    prev.as_ref(),
                );
                true
            }
            cmd::MODELS => {
                let items = self.model_arg_completions(arg_raw);
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " models ",
                    items,
                    arg_start,
                    false,
                    prev.as_ref(),
                );
                true
            }
            cmd::AGENTS => {
                let items = self.agent_arg_completions(arg_raw);
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " agents ",
                    items,
                    arg_start,
                    false,
                    prev.as_ref(),
                );
                true
            }
            cmd::SESSIONS => {
                self.ensure_history_loaded();
                let items = self.session_arg_completions(arg_raw);
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " sessions ",
                    items,
                    arg_start,
                    false,
                    prev.as_ref(),
                );
                true
            }
            cmd::CLOSE_SESSION => {
                let (tok_start, tok_raw) = last_arg_token(before, arg_start);
                let prior = text::split_shell_words(&before[arg_start..tok_start]);
                let items = self.close_session_arg_completions(&tok_raw, &prior);
                let freeform = !prior.is_empty() && tok_raw.is_empty();
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " live sessions ",
                    items,
                    tok_start,
                    freeform,
                    prev.as_ref(),
                );
                true
            }
            cmd::MCP => {
                if !self.mcp_loaded {
                    self.refresh_mcp();
                }
                let items = self.mcp_arg_completions(arg_raw);
                let prev = self.ac.take();
                self.ac = Autocomplete::new(
                    AcKind::SlashArg,
                    " mcp servers ",
                    items,
                    arg_start,
                    false,
                    prev.as_ref(),
                );
                true
            }
            _ => false,
        }
    }

    fn update_shell_autocomplete(&mut self, explicit_tab: bool) {
        let show_empty = explicit_tab || self.shell_tab_requested;
        let Some(ed) = self.active_editor() else {
            if show_empty {
                let items = self.shell_cmd_completions("", true);
                self.set_ac(AcKind::ShellCmd, " shell ", items, 0, true);
            } else {
                self.ac = None;
            }
            self.shell_complete_due = None;
            return;
        };
        let cursor = ed.cursor.min(ed.text.len());
        let before = ed.text[..cursor].to_string();
        match shell_token_context(&before) {
            ShellTokenPos::Command {
                start,
                raw,
                at_line_start,
            } => {
                self.shell_complete_due = None;
                if raw.is_empty() && !show_empty {
                    self.ac = None;
                    return;
                }
                let is_path_cmd = raw.contains('/')
                    || (cfg!(windows) && raw.contains('\\'))
                    || raw.starts_with('.')
                    || raw.starts_with('~')
                    || has_drive_prefix(&raw);
                if is_path_cmd {
                    self.show_shell_path_ac(start, &raw, true);
                    return;
                }
                let items = self.shell_cmd_completions(&raw, at_line_start);
                self.set_ac(AcKind::ShellCmd, " shell ", items, start, true);
            }
            ShellTokenPos::Arg { start, raw } => {
                self.update_shell_arg_autocomplete(&before, start, &raw, show_empty, explicit_tab);
            }
        }
    }

    fn set_ac(
        &mut self,
        kind: AcKind,
        title: &str,
        items: Vec<(String, String, String)>,
        start: usize,
        freeform_enter: bool,
    ) {
        let prev = self.ac.take();
        self.ac = Autocomplete::new(kind, title, items, start, freeform_enter, prev.as_ref());
    }

    fn show_shell_path_ac(&mut self, start: usize, raw: &str, show_empty: bool) {
        if raw.is_empty() && !show_empty {
            self.ac = None;
            return;
        }
        let items = self.path_completion_items(raw, true, true, true, None);
        self.set_ac(AcKind::ShellPath, " files ", items, start, true);
    }

    fn update_shell_arg_autocomplete(
        &mut self,
        before: &str,
        start: usize,
        raw: &str,
        show_empty: bool,
        explicit_tab: bool,
    ) {
        let raw_unquoted = raw.trim_start_matches(['"', '\'']);
        let is_flag = raw_unquoted.starts_with('-');
        let is_path = !is_flag
            && (raw.contains('/')
                || (cfg!(windows) && raw.contains('\\'))
                || raw_unquoted.starts_with('.')
                || raw_unquoted.starts_with('~')
                || has_drive_prefix(raw_unquoted));
        if is_path {
            self.shell_complete_due = None;
            self.show_shell_path_ac(start, raw, show_empty);
            return;
        }
        let cwd = self.session_cwd();
        let key = ShellCompleteKey {
            cwd: cwd.clone(),
            line: before.to_string(),
        };
        if let Some(cached) = self.shell_complete_cache.get(&key).map(|s| s.to_vec()) {
            self.shell_complete_due = None;
            if !cached.is_empty() {
                let items = build_shell_arg_items(&cached, raw, false, MAX_COMPLETIONS);
                self.set_ac(AcKind::ShellArg, " shell ", items, start, true);
                return;
            }
            if is_flag {
                self.ac = None;
            } else {
                self.show_shell_path_ac(start, raw, show_empty);
            }
            return;
        }
        let err_key = (cwd.clone(), before.to_string());
        let can_query =
            self.connected && (explicit_tab || !self.shell_complete_errors.contains(&err_key));
        if !can_query {
            self.shell_complete_due = None;
        } else if explicit_tab {
            self.shell_complete_due = None;
            self.shell_complete_errors.remove(&err_key);
            self.request_shell_complete(cwd.clone(), before.to_string());
        } else {
            self.shell_complete_due = Some((
                Instant::now() + SHELL_COMPLETE_DEBOUNCE,
                cwd.clone(),
                before.to_string(),
            ));
        }
        if let Some(prefix_items) = self
            .shell_complete_cache
            .longest_prefix(&cwd, before, start)
            .map(|s| s.to_vec())
        {
            let items = build_shell_arg_items(&prefix_items, raw, true, MAX_COMPLETIONS);
            if !items.is_empty() {
                self.set_ac(AcKind::ShellArg, " shell ", items, start, true);
                return;
            }
        }
        if is_flag {
            self.ac = None;
        } else {
            self.show_shell_path_ac(start, raw, show_empty);
        }
    }

    fn path_completion_items(
        &mut self,
        raw: &str,
        include_files: bool,
        shell_escape: bool,
        trailing_space_on_file: bool,
        extra_file: Option<&str>,
    ) -> Vec<(String, String, String)> {
        let sp = split_dir_query(raw);
        let key = DirKey {
            cwd: self.session_cwd(),
            path: sp.req_dir.clone(),
            include_files,
        };
        let mut out = Vec::new();
        if let Some(def) = extra_file
            && sp.display_prefix.is_empty()
            && (sp.name_prefix.is_empty()
                || def
                    .to_lowercase()
                    .starts_with(&sp.name_prefix.to_lowercase()))
        {
            out.push((def.to_string(), def.to_string(), "default filename".into()));
        }
        if let Some(listing) = self.dir_cache.get(&key).cloned() {
            out.extend(build_dir_items(
                &listing,
                &sp,
                include_files,
                shell_escape,
                trailing_space_on_file,
            ));
        } else {
            self.request_dirs(key);
        }
        out
    }

    fn shell_cmd_completions(
        &mut self,
        query: &str,
        include_history: bool,
    ) -> Vec<(String, String, String)> {
        let ql = query.trim().to_lowercase();
        let mut out: Vec<(String, String, String)> = Vec::new();
        let mut seen = HashSet::new();
        if include_history && !ql.is_empty() {
            for h in self.history.items.iter().rev() {
                if let Some(cmd) = h.strip_prefix('!').map(str::trim)
                    && !cmd.is_empty()
                    && cmd.to_lowercase().starts_with(&ql)
                    && seen.insert(cmd.to_string())
                {
                    out.push((cmd.to_string(), format!("!{cmd}"), "history".into()));
                    if out.len() >= SHELL_HISTORY_MAX {
                        break;
                    }
                }
            }
        }
        if let Some(bins) = self.bin_cache.get(&ql).cloned() {
            for b in bins {
                let ins = format!("{b} ");
                if seen.insert(b.clone()) {
                    out.push((ins, b, String::new()));
                }
            }
        } else {
            self.request_binaries(ql.clone());
            // Serve matching entries from the longest cached prefix while `ql` is in flight (0-flicker).
            if let Some((_, cached)) = self
                .bin_cache
                .iter()
                .filter(|(k, _)| ql.starts_with(k.as_str()))
                .max_by_key(|(k, _)| k.len())
            {
                for b in cached {
                    if text::fuzzy_score(b, &ql).is_some() && seen.insert(b.clone()) {
                        out.push((format!("{b} "), b.clone(), String::new()));
                    }
                }
            }
        }
        out.truncate(MAX_COMPLETIONS);
        out
    }

    fn variant_arg_completions(&self, q: &str) -> Vec<(String, String, String)> {
        let q = q.trim();
        let cur_model = self
            .active_target_session()
            .and_then(|id| self.views.get(&id))
            .map(|v| v.info.model.clone())
            .or_else(|| self.effective_draft_model());
        let variants = cur_model
            .as_ref()
            .and_then(|m| find_model(self.models(), m))
            .map(|m| m.variants.as_slice())
            .unwrap_or(&[]);
        let mut scored = Vec::new();
        if let Some(s) = text::fuzzy_score("default", q) {
            scored.push((
                s + 5,
                (
                    "default".to_string(),
                    "default".to_string(),
                    "model default".to_string(),
                ),
            ));
        }
        for v in variants {
            if let Some(s) = text::fuzzy_score(v, q) {
                scored.push((s + 10, (v.clone(), v.clone(), String::new())));
            }
        }
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        scored.into_iter().map(|x| x.1).collect()
    }

    fn model_arg_completions(&self, q: &str) -> Vec<(String, String, String)> {
        let q = q.trim();
        let mut scored = Vec::new();
        for m in self.models() {
            let key = format!("{}/{}", m.provider, m.id);
            let hay = format!("{} {} {} {}", key, m.name, m.id, m.provider_name);
            let s_id = text::fuzzy_score(&m.id, q).map(|s| s + 20);
            let s_key = text::fuzzy_score(&key, q).map(|s| s + 15);
            let s_all = text::fuzzy_score(&hay, q);
            if let Some(score) = s_id.max(s_key).max(s_all) {
                let provider = if m.provider_name.is_empty() {
                    &m.provider
                } else {
                    &m.provider_name
                };
                scored.push((
                    score,
                    (key.clone(), key, format!("{} · {}", m.name, provider)),
                ));
            }
        }
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        scored
            .into_iter()
            .map(|x| x.1)
            .take(MAX_COMPLETIONS)
            .collect()
    }

    fn agent_arg_completions(&self, q: &str) -> Vec<(String, String, String)> {
        let q = q.trim();
        let mut scored = Vec::new();
        for a in self.agents() {
            if let Some(score) = text::fuzzy_score(&a.name, q) {
                scored.push((
                    score,
                    (
                        a.name.clone(),
                        a.name.clone(),
                        text::one_line(&a.description),
                    ),
                ));
            }
        }
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        scored.into_iter().map(|x| x.1).collect()
    }

    fn session_arg_completions(&self, q: &str) -> Vec<(String, String, String)> {
        let q = q.trim().trim_matches(|c| c == '"' || c == '\'');
        let mut all: Vec<(&SessionInfo, bool)> = self
            .live
            .iter()
            .filter(|s| s.live)
            .map(|s| (s, true))
            .collect();
        let live_ids: HashSet<&str> = all.iter().map(|(s, _)| s.id.as_str()).collect();
        for h in &self.history_list {
            if !live_ids.contains(h.id.as_str()) {
                all.push((h, false));
            }
        }
        let mut name_counts: HashMap<&str, usize> = HashMap::new();
        for (s, _) in &all {
            if !s.name.is_empty() {
                *name_counts.entry(s.name.as_str()).or_default() += 1;
            }
        }
        let mut scored = Vec::new();
        for (s, is_live) in all {
            let label = if s.name.is_empty() {
                s.id.clone()
            } else {
                s.name.clone()
            };
            let val_raw = if !s.name.is_empty() && name_counts.get(s.name.as_str()) == Some(&1) {
                &s.name
            } else {
                &s.id
            };
            let val = if val_raw.contains(char::is_whitespace) {
                format!("\"{val_raw}\"")
            } else {
                val_raw.clone()
            };
            let hay = format!("{} {} {}", s.name, s.id, s.title);
            let s_name = text::fuzzy_score(&label, q).map(|sc| sc + 20);
            let s_all = text::fuzzy_score(&hay, q);
            if let Some(score) = s_name.max(s_all) {
                let kind = if is_live { "live" } else { "history" };
                let title = text::one_line(&s.title);
                let desc = if title.is_empty() {
                    kind.into()
                } else {
                    format!("{kind} · {title}")
                };
                scored.push((score + if is_live { 15 } else { 0 }, (val, label, desc)));
            }
        }
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        scored
            .into_iter()
            .map(|x| x.1)
            .take(MAX_COMPLETIONS)
            .collect()
    }

    fn close_session_arg_completions(
        &self,
        q: &str,
        already: &[String],
    ) -> Vec<(String, String, String)> {
        let q = q.trim().trim_matches(|c| c == '"' || c == '\'');
        let mut scored = Vec::new();
        for s in self.live.iter().filter(|s| s.live) {
            let taken = already.iter().any(|a| {
                a.eq_ignore_ascii_case(&s.id)
                    || (!s.name.is_empty() && a.eq_ignore_ascii_case(&s.name))
            });
            if taken {
                continue;
            }
            let label = if s.name.is_empty() {
                s.id.clone()
            } else {
                s.name.clone()
            };
            let tok = if label.contains(char::is_whitespace) {
                format!("\"{label}\" ")
            } else {
                format!("{label} ")
            };
            let hay = format!("{} {} {}", s.name, s.id, s.title);
            let s_name = text::fuzzy_score(&label, q).map(|sc| sc + 20);
            let s_all = text::fuzzy_score(&hay, q);
            if let Some(score) = s_name.max(s_all) {
                scored.push((
                    score,
                    (
                        tok,
                        label,
                        format!("{} · {}", s.agent, text::one_line(&s.title)),
                    ),
                ));
            }
        }
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        scored
            .into_iter()
            .map(|x| x.1)
            .take(MAX_COMPLETIONS)
            .collect()
    }

    fn mcp_arg_completions(&self, q: &str) -> Vec<(String, String, String)> {
        let q = q.trim();
        let mut scored = Vec::new();
        if !self.mcp.is_empty() {
            for e in &self.mcp {
                if let Some(score) = text::fuzzy_score(&e.name, q) {
                    scored.push((
                        score,
                        (
                            e.name.clone(),
                            e.name.clone(),
                            crate::ui::mcp::status_text(e).0,
                        ),
                    ));
                }
            }
        } else if let Some(cfg) = &self.config {
            for (name, line) in &cfg.mcp {
                if let Some(score) = text::fuzzy_score(name, q) {
                    scored.push((score, (name.clone(), name.clone(), line.clone())));
                }
            }
        }
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        scored.into_iter().map(|x| x.1).collect()
    }

    fn ensure_history_loaded(&mut self) {
        if self.history_loaded || self.history_inflight || !self.connected {
            return;
        }
        self.history_inflight = true;
        self.req(
            Request::ListHistory {
                cwd: None,
                limit: Some(HISTORY_LIST_LIMIT),
                creator: None,
                query: None,
            },
            Pending::HistoryBg,
        );
    }

    fn request_dirs(&mut self, key: DirKey) {
        if !self.connected {
            return;
        }
        let inflight_key = (key.cwd.clone(), key.path.clone(), key.include_files);
        if !self.dirs_inflight.insert(inflight_key) {
            return;
        }
        self.req(
            Request::ListDirs {
                path: key.path.clone(),
                cwd: (!key.cwd.is_empty()).then(|| key.cwd.clone()),
                include_files: key.include_files,
            },
            Pending::Dirs { key },
        );
    }

    fn request_binaries(&mut self, query: String) {
        if !self.connected || !self.bins_inflight.insert(query.clone()) {
            return;
        }
        self.req(
            Request::ListBinaries {
                query: query.clone(),
                limit: Some(MAX_COMPLETIONS),
            },
            Pending::Binaries { query },
        );
    }

    fn request_shell_complete(&mut self, cwd: String, line: String) {
        let inflight_key = (cwd.clone(), line.clone());
        if !self.connected || !self.shell_complete_inflight.insert(inflight_key) {
            return;
        }
        let cursor = Some(line.len());
        self.req(
            Request::CompleteShell {
                cwd: cwd.clone(),
                line: line.clone(),
                cursor,
                limit: Some(MAX_COMPLETIONS),
            },
            Pending::ShellComplete { cwd, line },
        );
    }

    /// Slash commands (built-in and from the backend config) matching `q`, best match first.
    fn slash_completions(&self, q: &str) -> Vec<(String, String, String)> {
        let mut items: Vec<(i64, (String, String, String))> = Vec::new();
        for c in COMMANDS {
            if let Some((score, label)) = best_command_match(c, q) {
                let value = if c.requires_args {
                    format!("/{} ", c.name)
                } else {
                    format!("/{}", c.name)
                };
                items.push((
                    score + CANONICAL_NAME_SCORE_BONUS,
                    (value, label, c.desc.to_string()),
                ));
            }
        }
        if let Some(cfg) = &self.config {
            for c in &cfg.commands {
                if let Some(s) = text::fuzzy_score(&c.name, q) {
                    items.push((
                        s,
                        (
                            format!("/{}", c.name),
                            format!("/{}", c.name),
                            c.description.clone(),
                        ),
                    ));
                }
            }
        }
        items.sort_by_key(|x| std::cmp::Reverse(x.0));
        items
            .into_iter()
            .map(|x| x.1)
            .take(MAX_COMPLETIONS)
            .collect()
    }

    fn request_files(&mut self, q: String) {
        if self.files_inflight {
            self.files_want = Some(q);
            return;
        }
        self.files_inflight = true;
        self.req(
            Request::ListFiles {
                cwd: self.session_cwd(),
                query: q.clone(),
                limit: Some(FILE_QUERY_LIMIT),
            },
            Pending::Files { query: q },
        );
    }

    fn submit_active_slash(&mut self) {
        match self.mode {
            Mode::Chat => self.submit(),
            Mode::Watch => {
                if let Some(tile) = self.watch.focus.clone() {
                    self.submit_tile_command(&tile);
                }
            }
        }
    }

    /// Accepts the selected completion (`run`: pressed Enter or clicked, `false`: pressed Tab).
    /// Returns true if the command was executed directly.
    pub fn accept_completion(&mut self, run: bool) -> bool {
        let Some(ac) = self.ac.take() else {
            return false;
        };
        self.shell_tab_requested = false;
        let Some((value, _, _)) = ac.items.get(ac.sel).cloned() else {
            return false;
        };
        match ac.kind {
            AcKind::Slash => {
                let name = value.trim().trim_start_matches('/').to_string();
                let requires_args = find_cmd(&name).is_some_and(|c| c.requires_args);
                let typed = self
                    .active_editor()
                    .map(|e| e.text.trim().trim_start_matches('/').to_string())
                    .unwrap_or_default();
                let typed_canon = find_cmd(&typed)
                    .map(|c| c.name.to_string())
                    .or_else(|| {
                        find_custom_cmd(self.config.as_ref(), &typed).map(|c| c.name.clone())
                    })
                    .unwrap_or(typed);
                let exact = typed_canon.eq_ignore_ascii_case(&name);
                if run && (!requires_args || exact) {
                    if let Some(ed) = self.active_editor_mut() {
                        ed.set_text(&format!("/{name}"));
                    }
                    self.submit_active_slash();
                    return true;
                }
                if let Some(ed) = self.active_editor_mut() {
                    ed.set_text(&value);
                }
            }
            AcKind::SlashArg => {
                let at_end = if let Some(ed) = self.active_editor_mut() {
                    ed.replace_before_cursor(ac.start, &value);
                    ed.cursor == ed.text.len()
                } else {
                    false
                };
                if run && at_end {
                    self.submit_active_slash();
                    return true;
                }
            }
            AcKind::File | AcKind::ShellCmd | AcKind::ShellPath | AcKind::ShellArg => {
                if let Some(ed) = self.active_editor_mut() {
                    ed.replace_before_cursor(ac.start, &value);
                }
            }
        }
        false
    }
    // -----------------------------------------------------------------------
    // Housekeeping
    // -----------------------------------------------------------------------

    fn shown_session_ids(&self) -> Vec<String> {
        match self.mode {
            Mode::Chat => self.current.iter().cloned().collect(),
            Mode::Watch => self.watch.tiles.iter().map(|(s, _)| s.clone()).collect(),
        }
    }

    /// Schedules a 80ms debounced `Request::Messages` range fetch when placeholders are visible.
    pub fn schedule_history_window(&mut self) {
        let now = Instant::now();
        for sid in self.shown_session_ids() {
            let Some(v) = self.views.get_mut(&sid) else {
                continue;
            };
            let Some((start, end)) = v.desired_range() else {
                v.range_due = None;
                continue;
            };
            let keep_deadline = v.range_due.is_some_and(|(_, s, e)| s == start && e == end);
            if !keep_deadline {
                v.range_due = Some((now + RANGE_DEBOUNCE, start, end));
            }
        }
    }

    /// Requests missing message ranges immediately for shown transcripts whose viewport overlaps
    /// unloaded placeholders (or is within one screen of the top of the materialized window).
    pub fn load_older(&mut self) {
        for sid in self.shown_session_ids() {
            let Some(v) = self.views.get_mut(&sid) else {
                continue;
            };
            let Some((start, end)) = v.desired_range() else {
                continue;
            };
            v.range_due = None;
            let generation = v.history_gen;
            v.range_pending = Some((generation, start, end));
            self.req(
                Request::Messages {
                    session_id: sid.clone(),
                    before: None,
                    start_seq: Some(start as u64),
                    end_seq: Some(end as u64),
                    limit: Some(PAGE_SIZE as u32),
                },
                Pending::MessagesRange {
                    sid,
                    generation,
                    start_seq: start,
                    end_seq: end,
                },
            );
        }
    }

    /// Whether anything needs periodic redraws, and how often.
    pub fn frame_interval(&self) -> Option<Duration> {
        let scroll_animating = self
            .views
            .values()
            .any(|v| v.chat.scrolling() || v.chat.drag_edge != 0);
        let modal_opening = self.dialog.as_ref().is_some_and(|d| d.animating())
            || self
                .perm
                .as_ref()
                .is_some_and(|p| p.opened.elapsed() < OPEN_SETTLE)
            || self
                .question
                .as_ref()
                .is_some_and(|q| q.opened.elapsed() < OPEN_SETTLE);
        if scroll_animating || self.toasts.animating() || modal_opening {
            return Some(FAST_FRAME);
        }
        if self.deferred.is_some() {
            return Some(DEFERRED_POLL);
        }
        if let Some(at) = self
            .views
            .values()
            .filter_map(|v| v.range_due.map(|(at, _, _)| at))
            .min()
        {
            return Some(
                at.saturating_duration_since(Instant::now())
                    .max(MIN_DEADLINE_WAIT),
            );
        }
        if let Some((at, _)) = &self.files_due {
            return Some(
                at.saturating_duration_since(Instant::now())
                    .max(MIN_DEADLINE_WAIT),
            );
        }
        if let Some((at, _, _)) = &self.shell_complete_due {
            return Some(
                at.saturating_duration_since(Instant::now())
                    .max(MIN_DEADLINE_WAIT),
            );
        }
        if let Some(at) = self.sessions_due {
            return Some(
                at.saturating_duration_since(Instant::now())
                    .max(MIN_DEADLINE_WAIT),
            );
        }
        // Spinners for sessions we don't hold a view of: the sidebar's live list, split tiles that
        // are still loading and the session dialogs all draw from `self.live`.
        let live_list_shown = self.mode == Mode::Watch
            || self.sidebar_shown()
            || matches!(&self.dialog, Some(Dialog::List(d)) if matches!(d.kind, Kind::Sessions | Kind::CloseSessions));
        let any_view_animating = self
            .views
            .values()
            .any(|v| v.animating() || matches!(v.info.status, SessionStatus::Retrying { .. }));
        let live_spinner_shown =
            live_list_shown && self.live.iter().any(|s| s.live && !s.status.is_idle());
        let anim =
            any_view_animating || live_spinner_shown || (!self.connected && self.refused.is_none());
        // Pulsing "connecting" dot of MCP servers (sidebar / dialog).
        let mcp_shown = self.sidebar_shown() || matches!(self.dialog, Some(Dialog::Mcp(_)));
        if anim || (mcp_shown && self.mcp.iter().any(|e| e.connecting())) {
            return Some(TICK);
        }
        if let Some(at) = self.mcp_poll_at {
            return Some(
                at.saturating_duration_since(Instant::now())
                    .max(MIN_DEADLINE_WAIT),
            );
        }
        if let Some(d) = self.toasts.next_deadline() {
            return Some(d.max(FAST_FRAME));
        }
        let key_timer_armed = self.hint.is_some()
            || self.leader.is_some()
            || self.esc_armed.is_some()
            || self.ctrlc_armed.is_some();
        if key_timer_armed {
            return Some(IDLE_TIMER_FRAME);
        }
        None
    }

    /// Whether the chat-mode sidebar is drawn at the current screen size.
    pub fn sidebar_shown(&self) -> bool {
        self.mode == Mode::Chat
            && self.sidebar
            && self.screen.width >= crate::render::SIDEBAR_MIN_SCREEN
            && (self.current.is_some()
                || (self.connected && (!self.live.is_empty() || !self.mcp.is_empty())))
    }

    /// Expires timers (hints, leader, armed keys) and fires debounced range loads.
    pub fn housekeeping(&mut self) {
        if let Some((target_sid, name, args, at)) = self.deferred.take() {
            let target = target_sid.or_else(|| self.active_target_session());
            let session_ok = target
                .as_deref()
                .and_then(|id| self.views.get(id))
                .is_some_and(|v| v.loaded);
            let ready = self.config.is_some() && session_ok;
            if ready {
                self.run_command_for(target, &name, &args);
                self.dirty = true;
            } else if at.elapsed() < DEFERRED_COMMAND_TTL {
                self.deferred = Some((target, name, args, at));
            }
        }
        if self.hint.as_ref().is_some_and(|h| Instant::now() >= h.2) {
            self.hint = None;
            self.dirty = true;
        }
        if self.leader.is_some_and(|l| l.elapsed() > LEADER_TIMEOUT) {
            self.leader = None;
            self.dirty = true;
        }
        if self
            .esc_armed
            .is_some_and(|l| l.elapsed() > ARMED_KEY_TIMEOUT)
        {
            self.esc_armed = None;
        }
        if self
            .ctrlc_armed
            .is_some_and(|l| l.elapsed() > ARMED_KEY_TIMEOUT)
        {
            self.ctrlc_armed = None;
        }
        let now = Instant::now();
        let range_ready = self.shown_session_ids().into_iter().any(|sid| {
            self.views
                .get(&sid)
                .is_some_and(|v| v.range_due.is_some_and(|(at, _, _)| now >= at))
        });
        if range_ready {
            self.load_older();
        }
        if let Some((at, _)) = &self.files_due
            && Instant::now() >= *at
            && let Some((_, q)) = self.files_due.take()
        {
            self.request_files(q);
        }
        if let Some((at, _, _)) = &self.shell_complete_due
            && Instant::now() >= *at
            && let Some((_, cwd, line)) = self.shell_complete_due.take()
        {
            self.request_shell_complete(cwd, line);
        }
        if self.sessions_due.is_some_and(|at| Instant::now() >= at) {
            self.sessions_due = None;
            if matches!(&self.dialog, Some(Dialog::List(d)) if d.kind == Kind::Sessions) {
                self.request_sessions_history();
            }
        }
        if !matches!(&self.dialog, Some(Dialog::List(d)) if d.kind == Kind::Sessions) {
            self.sessions_due = None;
            self.history_filtered = None;
        }
        if self.mcp_poll_at.is_some_and(|at| Instant::now() >= at) {
            self.refresh_mcp();
        }
        let shown = self.sidebar_shown();
        if shown && !self.sidebar_was_shown {
            self.refresh_mcp();
        }
        self.sidebar_was_shown = shown;
        self.toasts.gc();
        for v in self.views.values_mut() {
            if v.chat.drag_tick() {
                self.dirty = true;
            }
        }
    }
}

/// Finds a live session by exact name / id, else a unique case-insensitive name / id match.
pub fn match_session<'a>(live: &'a [SessionInfo], q: &str) -> Result<&'a SessionInfo, String> {
    let q = q.trim().trim_matches('"');
    let live: Vec<&SessionInfo> = live.iter().filter(|s| s.live).collect();
    if let Some(s) = live.iter().find(|s| s.id == q || s.name == q) {
        return Ok(s);
    }
    let lq = q.to_lowercase();
    let m: Vec<&&SessionInfo> = live
        .iter()
        .filter(|s| s.name.to_lowercase() == lq || s.id.to_lowercase() == lq)
        .collect();
    match m.as_slice() {
        [s] => Ok(s),
        [] => Err(format!("No live session named \"{q}\"")),
        _ => Err(format!(
            "\"{q}\" matches {} sessions — use the exact name or id",
            m.len()
        )),
    }
}

/// True when `trimmed` looks like a single-token `/command` (`^/[A-Za-z][A-Za-z0-9_-]*$`).
fn is_single_slash_token(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Block-letter logo shared by the welcome screen and the exit screen.
pub const LOGO: [&str; 3] = ["█▀▀▄ █▀▀▀ █▀▀▀", "█▄▄▀ █▀▀  █▀▀ ", "█  █ █▄▄▄ █   "];

/// Exit screen (opencode style): logo, session line, resume command. ANSI truecolor.
fn epilogue_text(info: &SessionInfo, resumable: bool) -> String {
    let ansi = |c: Color| match c {
        Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        _ => String::new(),
    };
    let (reset, bold) = ("\x1b[0m", "\x1b[1m");
    let (primary, muted) = (ansi(theme::PRIMARY), ansi(theme::MUTED));
    let mut out = String::new();
    for l in LOGO {
        // Left two letters in primary, the last one muted (like opencode's two-tone wordmark).
        let split = l
            .char_indices()
            .nth(LOGO_PRIMARY_CHARS)
            .map(|x| x.0)
            .expect("logo lines are longer than LOGO_PRIMARY_CHARS");
        let (a, b) = l.split_at(split);
        out.push_str(&format!(
            "  {primary}{a}{reset}{muted}{}{reset}\n",
            b.trim_end()
        ));
    }
    out.push('\n');
    let row = |label: &str, value: &str| {
        format!("  {muted}{label:<EPILOGUE_LABEL_WIDTH$}{reset}{bold}{value}{reset}")
    };
    if resumable {
        let title = text::one_line(&info.title);
        let session = match (title.is_empty(), info.name.is_empty()) {
            (false, false) => format!("{title}{reset}{muted}  ({}){reset}", info.name),
            (false, true) => title,
            (true, _) => info.name.clone(),
        };
        out.push_str(&row("Session", &session));
        out.push('\n');
        out.push_str(&row("Continue", &format!("ref -s {}", info.id)));
        out.push_str("\n\n");
    }
    out
}

/// Images of a restored prompt: those in the response, else the queued row's own.
fn restored_images(
    result: &Value,
    queued_images: Vec<ImageData>,
) -> Vec<(ImageData, Option<String>)> {
    let images = response_images(result);
    if !images.is_empty() {
        return images;
    }
    queued_images
        .into_iter()
        .filter(|i| !i.is_stub())
        .map(|i| (i, None))
        .collect()
}

/// `images` of an `Undo` / `RemoveQueued` response (to put back into the editor).
fn response_images(result: &Value) -> Vec<(ImageData, Option<String>)> {
    let list: Vec<ref_proto::NamedImage> = result
        .get("images")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    list.into_iter()
        .filter(|i| !i.image.is_stub())
        .map(|i| (i.image, i.name))
        .collect()
}

/// Best fuzzy score of a command's name or aliases against `q`, with the label to show.
/// The canonical name wins ties (aliases are only shown when they match better).
fn best_command_match(c: &Cmd, q: &str) -> Option<(i64, String)> {
    let mut best: Option<(i64, &str)> = None;
    for n in std::iter::once(c.name).chain(c.aliases.iter().copied()) {
        if let Some(s) = text::fuzzy_score(n, q)
            && best.is_none_or(|b| s > b.0)
        {
            best = Some((s, n));
        }
    }
    let (score, n) = best?;
    let label = if n == c.name {
        format!("/{}", c.name)
    } else {
        format!("/{n} → /{}", c.name)
    };
    Some((score, label))
}

fn live_key(sid: &str) -> String {
    format!("{LIVE_KEY_PREFIX}{sid}")
}

fn find_model<'a>(
    models: &'a [ref_proto::ModelInfo],
    m: &ModelRef,
) -> Option<&'a ref_proto::ModelInfo> {
    models
        .iter()
        .find(|x| x.provider == m.provider && x.id == m.model)
}

/// Dialog marker: a filled dot for the selected row, blank otherwise.
fn selected_marker(on: bool) -> (String, Color) {
    let glyph = if on { SELECTED_GLYPH } else { " " };
    (glyph.into(), theme::PRIMARY)
}

fn config_agents(config: &Option<ServerConfigInfo>) -> &[ref_proto::AgentInfo] {
    config.as_ref().map(|c| c.agents.as_slice()).unwrap_or(&[])
}

fn config_models(config: &Option<ServerConfigInfo>) -> &[ref_proto::ModelInfo] {
    config.as_ref().map(|c| c.models.as_slice()).unwrap_or(&[])
}

fn parse_field<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Result<T, String> {
    let field = v.get(key).ok_or_else(|| format!("missing `{key}`"))?;
    serde_json::from_value(field.clone()).map_err(|e| format!("invalid `{key}`: {e}"))
}

fn session_info_of(result: &Value) -> Result<SessionInfo, String> {
    parse_field(result, "session")
}

fn text_of(result: &Value) -> Result<&str, String> {
    result
        .get("text")
        .ok_or_else(|| "missing `text`".to_string())?
        .as_str()
        .ok_or_else(|| "invalid `text`".to_string())
}

fn flag_of(result: &Value, key: &str) -> bool {
    result.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn sessions_of(v: &Value) -> Result<Vec<SessionInfo>, String> {
    parse_field(v, "sessions")
}

fn format_cd_dir(path: &str) -> String {
    let mut display = text::tilde(path).replace('\\', "/");
    if is_bare_drive(&display) || !display.ends_with('/') {
        display.push('/');
    }
    display
}

fn normalize_cd_target(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let sep = if p.ends_with('\\') { '\\' } else { '/' };
    let trimmed = p.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return sep.to_string();
    }
    if is_bare_drive(trimmed) {
        return format!("{trimmed}{sep}");
    }
    trimmed.to_string()
}

fn is_fs_root(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if path == "/" || path == "\\" {
        return true;
    }
    let trimmed = path.trim_end_matches(['/', '\\']);
    if is_bare_drive(trimmed) {
        return true;
    }
    std::path::Path::new(path).parent().is_none()
}

fn resolved_parent_dir(resolved_dir: &str) -> Option<String> {
    if resolved_dir.is_empty() || is_fs_root(resolved_dir) {
        return None;
    }
    if has_drive_prefix(resolved_dir) {
        let norm = resolved_dir.replace('\\', "/");
        let trimmed = norm.trim_end_matches('/');
        let idx = trimmed.rfind('/')?;
        let parent = &trimmed[..idx];
        if is_bare_drive(parent) {
            return Some(format!("{parent}/"));
        }
        #[cfg(windows)]
        return Some(parent.replace('/', "\\"));
        #[cfg(not(windows))]
        return Some(parent.to_string());
    }
    let parent = std::path::Path::new(resolved_dir).parent()?;
    let s = parent.to_string_lossy();
    (!s.is_empty()).then(|| s.into_owned())
}

fn cd_parent_nav(raw: &str, resolved_dir: &str) -> String {
    if let Some(parent) = resolved_parent_dir(resolved_dir) {
        return format_cd_dir(&parent);
    }
    if !resolved_dir.is_empty() && is_fs_root(resolved_dir) {
        return raw.to_string();
    }
    let sp = split_dir_query(raw.trim());
    let sep = if sp.display_prefix.ends_with('\\') {
        '\\'
    } else {
        '/'
    };
    if sp.display_prefix.is_empty() || sp.display_prefix == "./" || sp.display_prefix == ".\\" {
        format!("..{sep}")
    } else {
        format!("{}..{sep}", sp.display_prefix)
    }
}

fn is_tilde_or_rooted_path(p: &str) -> bool {
    std::path::Path::new(p).is_absolute()
        || p == "~"
        || p.starts_with("~/")
        || p.starts_with("~\\")
        || has_drive_root_prefix(p)
}
