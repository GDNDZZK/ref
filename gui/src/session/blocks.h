#pragma once

#include "proto/types.h"
#include "session/fence_splitter.h"

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

// The `purpose` value in tool arguments that are still streaming, as far as it has arrived.
QString partialPurpose(const QString& rawInput);

} // namespace refgui::session
