#pragma once

#include <QJsonObject>
#include <QJsonValue>
#include <QList>
#include <QString>

#include <optional>
#include <variant>

// The ref wire protocol (crates/proto/src/lib.rs) as plain data. Timestamps are unix
// epoch milliseconds. Field names follow the JSON ones in camelCase.
namespace refgui::proto
{

// Sent in Hello; the backend answers with its own version.
constexpr int ProtocolVersion = 2;

// Written by ref-server to `<state dir>/instances/<pid>.json`.
struct InstanceInfo
{
	quint32 pid = 0;
	quint16 port = 0;
	QString bind;       // address the server listens on, e.g. 127.0.0.1 or 0.0.0.0
	qint64 startedAt = 0;
	QString token;      // clients must present it
};

struct ModelRef
{
	QString provider;
	QString model;
	std::optional<QString> variant{};
};

enum class TodoStatus
{
	Pending,
	InProgress,
	Completed,
	Cancelled,
};

struct Todo
{
	QString content;
	TodoStatus status;
};

struct ContextUsage
{
	quint64 used = 0;
	quint64 max = 0;
	quint64 recommend = 0;
	quint64 autoCompact = 0; // JSON: "auto"
};

struct Usage
{
	quint64 input = 0; // uncached
	quint64 output = 0;
	quint64 cacheRead = 0;
	quint64 cacheWrite = 0;
	quint64 reasoning = 0;
};

struct SessionStatus
{
	enum class State
	{
		Idle,
		Busy,              // waiting for or streaming an LLM response
		Running,           // executing tools
		WaitingPermission, // blocked on the user
		Retrying,          // the LLM request failed and will be repeated at `nextAt`
		Compacting,
	};

	State state = State::Idle;
	// Retrying only.
	quint32 attempt = 0;
	qint64 nextAt = 0;
	QString error{};
};

struct SessionInfo
{
	QString id;
	QString name;
	QString title;
	QString agent;
	ModelRef model;
	QString cwd;
	qint64 createdAt = 0;
	qint64 updatedAt = 0;
	SessionStatus status;
	quint32 attached = 0; // frontends directly attached
	bool live = false;    // loaded in the backend, not only on disk
	std::optional<QString> parent{};
	bool mustReport = false;
	ContextUsage context{};
	QList<Todo> todos{};
	std::optional<qint64> busySince{}; // when the current run started
	std::optional<QString> lastError{}; // the last run ended with this error
};

// Image bytes are left out of messages: `data` is empty and `id` names the image for GetImage.
struct ImageData
{
	QString mime;
	QString data; // base64
	std::optional<QString> id{};
	std::optional<quint64> size{};
};

// An image with its file name that was taken back: the removed prompt's images in the
// answer to RemoveQueued (and Undo), with their bytes.
struct NamedImage
{
	QString mime;
	QString data; // base64
	std::optional<QString> name{};
};

enum class ToolState
{
	Streaming,          // arguments still streaming from the model
	AwaitingPermission,
	Delayed,            // waiting out the configured delay
	Running,
	Done,
	Error,
	Denied,
	Cancelled,
};

bool isFinal(ToolState state);

struct TextPart
{
	QString text;
	qint64 time = 0;
};

struct ReasoningPart
{
	QString text;
	qint64 time = 0;
	std::optional<qint64> timeEnd{};
	QJsonValue providerData{};
};

struct ImagePart
{
	ImageData image;
	std::optional<QString> name{};
};

struct ToolPart
{
	QString callId;
	QString name;
	QString purpose{};
	QJsonValue input{}; // parsed arguments without `purpose`; null while streaming
	QString rawInput{}; // the argument JSON as streamed
	ToolState state = ToolState::Streaming;
	std::optional<QString> output{};
	QJsonValue meta{}; // structured extras for display, e.g. diff or exit code
	QList<ImageData> images{};
	qint64 time = 0;
	std::optional<qint64> timeEnd{};
	std::optional<qint64> runAt{}; // Delayed: when execution starts
	QJsonValue providerData{};
};

// A part type this client does not know. Kept so part indexes stay aligned.
struct UnknownPart
{
	QString type;
};

using Part = std::variant<TextPart, ReasoningPart, ImagePart, ToolPart, UnknownPart>;

enum class Role
{
	User,
	Assistant,
};

// Where a user-role message came from.
struct MessageSource
{
	enum class Kind
	{
		Human,
		Peer,       // another session's send_message
		Spawn,      // the prompt of the session that created this one
		Reminder,   // injected by the harness
		Compaction, // the summary that replaced older context
		Shell,      // output of a `!command`
	};

	Kind kind = Kind::Human;
	// Peer and Spawn.
	QString fromId{};
	QString fromName{};
	QString msgId{}; // Peer only
};

struct Message
{
	QString id;
	Role role = Role::User;
	qint64 time = 0;
	std::optional<qint64> timeEnd{}; // assistant: when it finished
	MessageSource source{};
	QList<Part> parts{};
	std::optional<ModelRef> model{};
	std::optional<QString> agent{};
	std::optional<Usage> usage{};
	std::optional<QString> error{}; // the turn ended with this error or "aborted"
};

struct PermissionRequest
{
	QString sessionId;
	QString messageId;
	QString callId;
	QString tool;
	QString purpose;
	QJsonValue input;
	qint64 time = 0;
};

struct SessionSnapshot
{
	SessionInfo info;
	QList<Message> messages; // the newest page
	bool moreBefore = false;
	QList<PermissionRequest> pendingPermissions{};
	QList<Message> queued{};
};

struct Delta
{
	enum class Kind
	{
		Text,
		Reasoning,
		ToolInput,
	};

	Kind kind = Kind::Text;
	QString text;
};

// ---- Session events -----------------------------------------------------------------

struct InfoEvent
{
	SessionInfo info;
};
struct MessageEvent // a new message, or the whole replacement of one with the same id
{
	Message message;
};
struct PartEvent // part `index` of the message appended or replaced
{
	QString messageId;
	qsizetype index = 0;
	Part part;
};
struct DeltaEvent
{
	QString messageId;
	qsizetype index = 0;
	Delta delta;
};
struct RemovedEvent
{
	QString messageId;
};
struct ResetEvent // the whole history replaced; only the newest page
{
	QList<Message> messages;
	bool moreBefore = false;
};
struct PermissionEvent
{
	PermissionRequest request;
};
struct PermissionResolvedEvent
{
	QString callId;
};
struct QueueEvent // every message still waiting for the next safe point
{
	QList<Message> messages;
};
struct ErrorEvent
{
	QString text;
};
struct ClosedEvent
{
};

using SessionEvent = std::variant<InfoEvent,
                                  MessageEvent,
                                  PartEvent,
                                  DeltaEvent,
                                  RemovedEvent,
                                  ResetEvent,
                                  PermissionEvent,
                                  PermissionResolvedEvent,
                                  QueueEvent,
                                  ErrorEvent,
                                  ClosedEvent>;

// ---- Server frames ------------------------------------------------------------------

struct ResponseFrame
{
	quint64 id = 0;
	bool ok = false;
	QJsonObject result{}; // empty when the request failed
	QString error{};      // set when it failed
};
struct EventFrame
{
	QString sessionId;
	SessionEvent event;
};
struct SessionsFrame // the live session list changed
{
	QList<SessionInfo> sessions;
};
struct ToastFrame
{
	enum class Level
	{
		Info,
		Warn,
		Error,
	};

	Level level = Level::Info;
	QString text;
};
struct NotifyFrame
{
	enum class Kind
	{
		Permission,
		TurnDone,
		TurnError,
	};

	QString sessionId;
	Kind kind = Kind::Permission;
	QString sessionTitle;
	std::optional<QString> detail{};
};

using ServerFrame = std::variant<ResponseFrame, EventFrame, SessionsFrame, ToastFrame, NotifyFrame>;

// ---- Results of requests --------------------------------------------------------------

struct HelloResult
{
	QString serverVersion;
	int protocol = 1; // older backends do not report it
};

struct PromptResult
{
	bool queued = false; // the session was busy: the message waits for the next safe point
	QString messageId;
};

// RemoveQueued: `removed` is false when the message was delivered already.
struct RemovedPrompt
{
	bool removed = false;
	QString text{};
	QList<NamedImage> images{};
};

struct MessagesPage
{
	QList<Message> messages;
	bool moreBefore = false;
};

} // namespace refgui::proto
