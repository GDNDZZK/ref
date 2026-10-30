#include "session/blocks.h"

#include <QJsonArray>
#include <QJsonDocument>

#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::session
{
namespace
{

// Same word as the server uses for a turn the user cancelled; such a turn is not an error to show.
constexpr QLatin1StringView AbortedError = "aborted"_L1;
// Where the server puts the tail of the output of a running tool in the tool's meta.
constexpr qsizetype NotAPart = -1; // partIndex of a block that belongs to the message, not to a part
constexpr QLatin1StringView LiveOutputKey = "live_output"_L1;

QString sourceName(proto::MessageSource::Kind kind)
{
	using Kind = proto::MessageSource::Kind;
	switch(kind)
	{
	case Kind::Human:
		return u"human"_s;
	case Kind::Peer:
		return u"peer"_s;
	case Kind::Spawn:
		return u"spawn"_s;
	case Kind::Reminder:
		return u"reminder"_s;
	case Kind::Compaction:
		return u"compaction"_s;
	case Kind::Shell:
		return u"shell"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

QString toolStateName(proto::ToolState state)
{
	using State = proto::ToolState;
	switch(state)
	{
	case State::Streaming:
		return u"streaming"_s;
	case State::AwaitingPermission:
		return u"awaiting_permission"_s;
	case State::Delayed:
		return u"delayed"_s;
	case State::Running:
		return u"running"_s;
	case State::Done:
		return u"done"_s;
	case State::Error:
		return u"error"_s;
	case State::Denied:
		return u"denied"_s;
	case State::Cancelled:
		return u"cancelled"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

// A block with the identity fields filled in.
Block makeBlock(Block::Kind kind, const MessageHeader& header, qsizetype partIndex)
{
	const bool fromUser = header.role == proto::Role::User;
	const bool namesSender = header.source.kind == proto::MessageSource::Kind::Peer || header.source.kind == proto::MessageSource::Kind::Spawn;
	return {
		.kind = kind,
		.messageId = header.id,
		.partIndex = partIndex,
		.source = fromUser ? sourceName(header.source.kind) : QString(),
		.sender = namesSender ? header.source.fromName : QString(),
		.time = header.time,
	};
}

void startBlock(QList<Block>& blocks, Block block)
{
	if(!blocks.isEmpty())
	{
		blocks.last().streaming = false; // only the newest block receives content
	}
	block.streaming = true;
	blocks.append(std::move(block));
}

void applySplitEvent(QList<Block>& blocks, const FenceSplitter::Event& event, const Block& identity)
{
	if(const auto* opened = std::get_if<FenceSplitter::CodeOpened>(&event))
	{
		Block code = identity;
		code.kind = Block::Kind::Code;
		code.language = opened->language;
		startBlock(blocks, std::move(code));
		return;
	}
	if(std::holds_alternative<FenceSplitter::CodeClosed>(event))
	{
		blocks.last().streaming = false;
		return;
	}

	// Prose after a closed fence starts a new Markdown block.
	const bool continuesLast = !blocks.isEmpty() && (blocks.last().kind == Block::Kind::Markdown || blocks.last().streaming);
	if(!continuesLast)
	{
		Block markdown = identity;
		markdown.kind = Block::Kind::Markdown;
		startBlock(blocks, std::move(markdown));
	}
	blocks.last().content += std::get<FenceSplitter::TextAppended>(event).text;
}

Block::ImageRef imageRef(const proto::ImageData& image, const QString& name = {})
{
	return {.id = image.id.value_or(QString()), .mime = image.mime, .size = static_cast<qint64>(image.size.value_or(0)), .name = name};
}

QList<Block> toolBlocks(const MessageHeader& header, qsizetype index, const proto::ToolPart& tool)
{
	Block block = makeBlock(Block::Kind::Tool, header, index);
	block.content = tool.purpose.isEmpty() ? tool.name : tool.name + u' ' + tool.purpose;
	block.toolName = tool.name;
	block.purpose = tool.purpose;
	block.toolState = toolStateName(tool.state);
	block.callId = tool.callId;
	block.output = tool.output.value_or(QString());
	block.streaming = !proto::isFinal(tool.state);
	block.failed = tool.state == proto::ToolState::Error || tool.state == proto::ToolState::Denied;
	block.timeEnd = tool.timeEnd.value_or(0);
	block.runAt = tool.runAt.value_or(0);
	block.meta = tool.meta.toObject();
	block.liveOutput = block.meta.take(LiveOutputKey).toString();
	for(const proto::ImageData& image : tool.images)
	{
		block.images.append(imageRef(image));
	}
	if(tool.input.isObject())
	{
		block.inputJson = QString::fromUtf8(QJsonDocument(tool.input.toObject()).toJson(QJsonDocument::Indented));
	}
	else if(tool.input.isArray())
	{
		block.inputJson = QString::fromUtf8(QJsonDocument(tool.input.toArray()).toJson(QJsonDocument::Indented));
	}
	else
	{
		block.rawInput = tool.rawInput; // still streaming
	}
	return {block};
}

QList<Block> textBlocks(const MessageHeader& header, qsizetype index, const QString& text, bool streaming, FenceSplitter& splitter)
{
	QList<Block> blocks;
	const Block identity = makeBlock(Block::Kind::Markdown, header, index);
	for(const FenceSplitter::Event& event : splitter.feed(text))
	{
		applySplitEvent(blocks, event, identity);
	}
	if(!streaming)
	{
		for(const FenceSplitter::Event& event : splitter.finish())
		{
			applySplitEvent(blocks, event, identity);
		}
		if(!blocks.isEmpty())
		{
			blocks.last().streaming = false;
		}
	}
	return blocks;
}

QList<Block> reasoningBlocks(const MessageHeader& header, qsizetype index, const proto::ReasoningPart& reasoning, bool streaming)
{
	const bool hasContent = !reasoning.text.isEmpty() || streaming;
	if(!hasContent)
	{
		return {}; // e.g. a part that only carries provider data
	}
	Block block = makeBlock(Block::Kind::Reasoning, header, index);
	block.content = reasoning.text;
	block.streaming = streaming;
	return {block};
}

} // namespace

QString kindName(Block::Kind kind)
{
	switch(kind)
	{
	case Block::Kind::User:
		return u"user"_s;
	case Block::Kind::Reasoning:
		return u"reasoning"_s;
	case Block::Kind::Markdown:
		return u"markdown"_s;
	case Block::Kind::Code:
		return u"code"_s;
	case Block::Kind::Tool:
		return u"tool"_s;
	case Block::Kind::Error:
		return u"error"_s;
	case Block::Kind::Image:
		return u"image"_s;
	case Block::Kind::Meta:
		return u"meta"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

bool isEnded(const MessageHeader& header)
{
	return header.timeEnd.has_value() || header.error.has_value();
}

bool isPartStreaming(const MessageHeader& header, const proto::Part& part, bool isLast)
{
	if(const auto* tool = std::get_if<proto::ToolPart>(&part))
	{
		return !proto::isFinal(tool->state);
	}
	if(isEnded(header))
	{
		return false;
	}
	if(const auto* reasoning = std::get_if<proto::ReasoningPart>(&part))
	{
		return isLast && !reasoning->timeEnd.has_value();
	}
	return std::holds_alternative<proto::TextPart>(part) && isLast;
}

QList<Block> assistantPartBlocks(const MessageHeader& header, qsizetype index, const proto::Part& part, bool streaming, FenceSplitter& splitter)
{
	if(const auto* text = std::get_if<proto::TextPart>(&part))
	{
		return textBlocks(header, index, text->text, streaming, splitter);
	}
	if(const auto* reasoning = std::get_if<proto::ReasoningPart>(&part))
	{
		return reasoningBlocks(header, index, *reasoning, streaming);
	}
	if(const auto* tool = std::get_if<proto::ToolPart>(&part))
	{
		return toolBlocks(header, index, *tool);
	}
	return {}; // images and unknown parts have no place in an assistant turn
}

void feedText(QList<Block>& blocks, FenceSplitter& splitter, const MessageHeader& header, qsizetype index, QStringView delta)
{
	const Block identity = makeBlock(Block::Kind::Markdown, header, index);
	for(const FenceSplitter::Event& event : splitter.feed(delta))
	{
		applySplitEvent(blocks, event, identity);
	}
}

QList<Block> userBlocks(const MessageHeader& header, const QList<proto::Part>& parts)
{
	QStringList texts;
	QList<Block> images;
	for(qsizetype index = 0; index < parts.size(); ++index)
	{
		if(const auto* text = std::get_if<proto::TextPart>(&parts.at(index)))
		{
			texts.append(text->text);
		}
		else if(const auto* image = std::get_if<proto::ImagePart>(&parts.at(index)))
		{
			Block block = makeBlock(Block::Kind::Image, header, index);
			block.content = image->name.value_or(image->image.mime);
			block.images = {imageRef(image->image, image->name.value_or(QString()))};
			images.append(block);
		}
	}

	QList<Block> blocks;
	if(!texts.isEmpty() || images.isEmpty())
	{
		Block user = makeBlock(Block::Kind::User, header, 0);
		user.content = texts.join(u"\n\n"_s);
		blocks.append(user);
	}
	blocks.append(images);
	return blocks;
}

QList<Block> errorBlocks(const MessageHeader& header)
{
	const bool showsError = header.error && header.error->compare(AbortedError, Qt::CaseInsensitive) != 0;
	if(!showsError)
	{
		return {};
	}
	Block block = makeBlock(Block::Kind::Error, header, NotAPart);
	block.content = *header.error;
	return {block};
}

QList<Block> metaBlocks(const MessageHeader& header)
{
	if(header.source.kind == proto::MessageSource::Kind::Compaction)
	{
		return {};
	}
	Block block = makeBlock(Block::Kind::Meta, header, NotAPart);
	block.timeEnd = header.timeEnd.value_or(0);
	block.agent = header.agent.value_or(QString());
	if(header.model)
	{
		block.modelId = header.model->model;
		block.variant = header.model->variant.value_or(QString());
	}
	if(header.usage)
	{
		block.tokensIn = header.usage->input + header.usage->cacheRead + header.usage->cacheWrite;
		block.tokensOut = header.usage->output;
	}
	const bool aborted = header.error && header.error->compare(AbortedError, Qt::CaseInsensitive) == 0;
	block.interrupted = aborted;
	block.errored = header.error && !aborted;
	block.human = header.role == proto::Role::User && header.source.kind == proto::MessageSource::Kind::Human;
	return {block};
}

QString partialPurpose(const QString& rawInput)
{
	static const QLatin1StringView Key = "\"purpose\""_L1;
	const qsizetype key = rawInput.indexOf(Key);
	if(key < 0)
	{
		return {};
	}

	QStringView rest = QStringView(rawInput).mid(key + Key.size()).trimmed();
	if(!rest.startsWith(u':'))
	{
		return {};
	}
	rest = rest.mid(1).trimmed();
	if(!rest.startsWith(u'"'))
	{
		return {};
	}

	// Line breaks and tabs would break the one-line title.
	QString purpose;
	bool escaped = false;
	for(const QChar ch : rest.mid(1))
	{
		if(escaped)
		{
			purpose += (ch == u'n' || ch == u't') ? u' ' : ch;
			escaped = false;
		}
		else if(ch == u'\\')
		{
			escaped = true;
		}
		else if(ch == u'"')
		{
			break;
		}
		else
		{
			purpose += ch;
		}
	}
	return purpose;
}

} // namespace refgui::session
