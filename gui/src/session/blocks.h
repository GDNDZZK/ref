#pragma once

#include "proto/types.h"
#include "session/fence_splitter.h"

#include <QJsonObject>
#include <QList>
#include <QString>

namespace refgui::session
{

// One row of the conversation: the unit the QML delegates draw. A message turns into
// one or more blocks; the identity fields tell which message and part a block came from.
struct Block
{
	enum class Kind
	{
		User,
		Reasoning,
		Markdown,
		Code,
		Tool,
		Error,
		Image,
		Meta, // the line under a message: time, agent, tokens, actions
	};

	// An image the server holds: its bytes come from GetImage with `id`.
	struct ImageRef
	{
		QString id{};
		QString mime{};
		qint64 size = 0; // bytes
		QString name{};

		bool operator==(const ImageRef&) const = default;
	};

	Kind kind = Kind::Markdown;
	QString content{};
	QString language{};
	QString output{};
	bool streaming = false;
	bool failed = false;

	QString messageId{};
	qsizetype partIndex = 0;
	QString source{}; // user blocks: "human", "peer", "spawn", "reminder", "compaction" or "shell"
	QString sender{}; // peer and spawn: the name of the session that sent it
	qint64 time = 0;

	// Tool blocks.
	QString toolName{};
	QString purpose{};
	QString toolState{};
	QString inputJson{};
	QString rawInput{}; // only while the arguments are not valid JSON yet
	QString callId{};
	qint64 timeEnd = 0; // 0 while running
	qint64 runAt = 0;   // Delayed: when execution starts
	QJsonObject meta{}; // display extras of the tool (diff, exit code, ...), without the live output
	QString liveOutput{};

	// Image blocks have one entry, tool blocks all the images of the result.
	QList<ImageRef> images{};

	// Meta blocks, which also use `time` and `timeEnd` of the message.
	QString agent{};
	QString modelId{};
	QString variant{};
	quint64 tokensIn = 0; // input including cache reads and writes
	quint64 tokensOut = 0;
	bool interrupted = false; // the user stopped the turn
	bool errored = false;
	bool human = false; // typed by the user, so it can be undone

	// Set by the model from the position of the last compaction, never by the builders.
	bool stale = false;        // before the last compaction: no longer sent to the model
	bool contextStart = false; // the last compaction, with older messages above it

	bool operator==(const Block&) const = default;
};

QString kindName(Block::Kind kind);

// A message without its parts: what the blocks of its parts have in common.
using MessageHeader = proto::Message;

// True once nothing more will be streamed into the message.
bool isEnded(const MessageHeader& header);

// Whether the part is still receiving content. Of the text and reasoning parts, only the last one streams.
bool isPartStreaming(const MessageHeader& header, const proto::Part& part, bool isLast);

// All blocks of an assistant part. `splitter` is fresh and left in the state that
// follows the part's text, so that more text can be fed with `feedText`.
QList<Block> assistantPartBlocks(const MessageHeader& header, qsizetype index, const proto::Part& part, bool streaming, FenceSplitter& splitter);

// Adds streamed text to the blocks of a part that is still streaming, as built by
// `assistantPartBlocks`.
void feedText(QList<Block>& blocks, FenceSplitter& splitter, const MessageHeader& header, qsizetype index, QStringView delta);

// A user message is one block of its text, plus one per image.
QList<Block> userBlocks(const MessageHeader& header, const QList<proto::Part>& parts);

// The error that ended an assistant message, if it is one worth showing.
QList<Block> errorBlocks(const MessageHeader& header);

// The line under a message. A compaction summary has none: its divider says it all.
QList<Block> metaBlocks(const MessageHeader& header);

// The `purpose` value in tool arguments that are still streaming, as far as it has arrived.
QString partialPurpose(const QString& rawInput);

} // namespace refgui::session
