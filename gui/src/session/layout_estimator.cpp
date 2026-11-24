#include "session/layout_estimator.h"

#include "session/blocks.h"
#include "session/fence_splitter.h"
#include "session/table_splitter.h"

#include <QVariantMap>

#include <algorithm>
#include <cmath>
#include <limits>

using namespace Qt::StringLiterals;

namespace refgui::session
{
namespace
{

constexpr double MinValidColumnWidth = 1.0;
constexpr double MinProseCharsPerLine = 16.0;
constexpr double MinUserCharsPerLine = 12.0;
constexpr double MinTextLines = 1.0;
constexpr double MinReasoningLines = 1.0;
constexpr double MaxReasoningLines = 16.0;
constexpr double MinToolLines = 2.0;
constexpr double MaxTodoLines = 10.0;
constexpr double MaxToolOutputLines = 16.0;
constexpr double BlockSpacing = 14.0;
constexpr double TurnGap = 18.0;
constexpr double TurnDividerHeight = 1.0;
constexpr double MetaLineHeight = 24.0;
constexpr double SourceLabelHeight = 18.0;
constexpr double SourceLabelGap = 4.0;
constexpr double UserBubblePaddingY = 18.0;
constexpr double UserBubblePaddingX = 28.0;
constexpr double ProseLineHeight = 22.0;
constexpr double CharWidthProse = 8.2;
constexpr double CharWidthUser = 8.5;
constexpr double ReasoningHeaderHeight = 32.0;
constexpr double ReasoningLineHeight = 20.0;
constexpr double ReasoningBodyPad = 16.0;
constexpr double ReasoningBodyPadX = 24.0;
constexpr double CodeHeaderHeight = 36.0;
constexpr double CodeLineHeight = 19.0;
constexpr double TableOverhead = 48.0;
constexpr double ToolPillHeight = 38.0;
constexpr double ToolBodyPadding = 28.0;
constexpr double TodoRowHeight = 24.0;
constexpr double ToolOutputLineHeight = 20.0;
constexpr double ImageThumbHeight = 72.0;
constexpr double MaxInlineImageWidth = 320.0;
constexpr double MaxInlineImageHeight = 360.0;
constexpr double ErrorCardHeight = 48.0;

quint32 countLines(QStringView text)
{
	if(text.isEmpty())
	{
		return 0;
	}
	const QStringView trimmed = text.endsWith(u'\n') ? text.chopped(1) : text;
	if(trimmed.isEmpty())
	{
		return 1;
	}
	return static_cast<quint32>(trimmed.count(u'\n')) + 1;
}

proto::PartLayoutMeta summarizeTextPart(const QString& text)
{
	proto::PartLayoutMeta meta;
	meta.kind = u"text"_s;
	meta.chars = static_cast<quint32>(text.size());
	meta.lines = countLines(text);

	FenceSplitter fence(false);
	TableSplitter table;
	bool inCode = false;
	const auto handleEvents = [&](const QList<FenceSplitter::Event>& events) {
		for(const auto& ev : events)
		{
			if(std::holds_alternative<FenceSplitter::CodeOpened>(ev))
			{
				inCode = true;
				++meta.codeBlocks;
			}
			else if(std::holds_alternative<FenceSplitter::CodeClosed>(ev))
			{
				inCode = false;
			}
			else if(const auto* appended = std::get_if<FenceSplitter::TextAppended>(&ev))
			{
				if(inCode)
				{
					meta.codeLines += countLines(appended->text);
				}
				else
				{
					const auto tableEvents = table.feed(appended->text);
					for(const auto& tev : tableEvents)
					{
						if(std::holds_alternative<TableSplitter::TableBlock>(tev))
						{
							++meta.tables;
						}
					}
				}
			}
		}
	};
	handleEvents(fence.feed(text));
	handleEvents(fence.finish());
	const auto trailing = table.finish();
	for(const auto& tev : trailing)
	{
		if(std::holds_alternative<TableSplitter::TableBlock>(tev))
		{
			++meta.tables;
		}
	}
	return meta;
}

double estimatePartHeight(const proto::PartLayoutMeta& part, double width, bool thinkingExpanded, bool detailsExpanded)
{
	if(part.kind == "reasoning"_L1)
	{
		if(!thinkingExpanded)
		{
			return ReasoningHeaderHeight;
		}
		const double charsPerLine = std::max(MinProseCharsPerLine, std::floor((width - ReasoningBodyPadX) / CharWidthProse));
		const double wrapped = std::ceil(static_cast<double>(part.chars) / charsPerLine);
		const double lineCount = std::clamp(std::max<double>(part.lines, wrapped), MinReasoningLines, MaxReasoningLines);
		return ReasoningHeaderHeight + lineCount * ReasoningLineHeight + ReasoningBodyPad;
	}
	if(part.kind == "tool"_L1)
	{
		const bool isTodo = part.tool && *part.tool == "todo"_L1;
		if(!detailsExpanded && !isTodo)
		{
			return ToolPillHeight;
		}
		const double rows = std::clamp(static_cast<double>(part.toolLines), MinToolLines, isTodo ? MaxTodoLines : MaxToolOutputLines);
		const double rowHeight = isTodo ? TodoRowHeight : ToolOutputLineHeight;
		return ToolPillHeight + ToolBodyPadding + rows * rowHeight;
	}
	if(part.kind == "image"_L1)
	{
		if(part.width && part.height && *part.width > 0 && *part.height > 0)
		{
			const double maxW = std::min(width, MaxInlineImageWidth);
			const double scale = std::min(1.0, maxW / static_cast<double>(*part.width));
			const double scaledH = static_cast<double>(*part.height) * scale;
			return std::clamp(scaledH, ImageThumbHeight, MaxInlineImageHeight);
		}
		return ImageThumbHeight;
	}

	// Text part
	const double charsPerLine = std::max(MinProseCharsPerLine, std::floor(width / CharWidthProse));
	const double nonCodeChars = static_cast<double>(part.chars);
	const double wrapped = std::ceil(nonCodeChars / charsPerLine);
	const double textLines = std::max<double>(MinTextLines, std::max<double>(part.lines, wrapped));
	const double codeHeight = static_cast<double>(part.codeBlocks) * CodeHeaderHeight + static_cast<double>(part.codeLines) * CodeLineHeight;
	const double tableHeight = static_cast<double>(part.tables) * TableOverhead;
	return std::max(ProseLineHeight, textLines * ProseLineHeight + codeHeight + tableHeight);
}

} // namespace

proto::MessageLayoutMeta LayoutEstimator::computeMeta(quint64 seq, const proto::Message& message)
{
	proto::MessageLayoutMeta meta;
	meta.seq = seq;
	meta.id = message.id;
	meta.role = message.role;
	meta.sourceKind = sourceName(message.source.kind);
	meta.hasError = message.error.has_value() && !message.error->isEmpty() && message.error->compare(u"aborted"_s, Qt::CaseInsensitive) != 0;

	for(const proto::Part& part : message.parts)
	{
		if(const auto* text = std::get_if<proto::TextPart>(&part))
		{
			meta.parts.append(summarizeTextPart(text->text));
		}
		else if(const auto* reasoning = std::get_if<proto::ReasoningPart>(&part))
		{
			proto::PartLayoutMeta p;
			p.kind = u"reasoning"_s;
			p.chars = static_cast<quint32>(reasoning->text.size());
			p.lines = std::max<quint32>(1, countLines(reasoning->text));
			meta.parts.append(std::move(p));
		}
		else if(std::holds_alternative<proto::ImagePart>(part))
		{
			proto::PartLayoutMeta p;
			p.kind = u"image"_s;
			meta.parts.append(std::move(p));
		}
		else if(const auto* tool = std::get_if<proto::ToolPart>(&part))
		{
			proto::PartLayoutMeta p;
			p.kind = u"tool"_s;
			p.tool = tool->name;
			if(tool->output)
			{
				p.toolLines = static_cast<quint16>(std::min<quint32>(std::numeric_limits<quint16>::max(), countLines(*tool->output)));
			}
			meta.parts.append(std::move(p));
		}
	}
	return meta;
}

double LayoutEstimator::estimateMessageHeight(const proto::MessageLayoutMeta& meta,
                                            double columnWidth,
                                            bool thinkingExpanded,
                                            bool detailsExpanded)
{
	const double width = columnWidth > MinValidColumnWidth ? std::max(MinColumnWidth, columnWidth) : DefaultColumnWidth;
	double total = 0.0;
	int blocks = 0;

	if(meta.role == proto::Role::User)
	{
		if(meta.seq > 0 && isTurnSourceKindName(meta.sourceKind))
		{
			total += TurnGap + TurnDividerHeight;
		}
		if(!meta.sourceKind.isEmpty() && meta.sourceKind != "human"_L1)
		{
			total += SourceLabelHeight + SourceLabelGap;
		}
		quint32 textChars = 0;
		quint32 textLines = 0;
		bool hasImage = false;
		for(const proto::PartLayoutMeta& part : meta.parts)
		{
			if(part.kind == "image"_L1)
			{
				hasImage = true;
			}
			else
			{
				textChars += part.chars;
				textLines += part.lines;
			}
		}
		const double charsPerLine = std::max(MinUserCharsPerLine, std::floor((width - UserBubblePaddingX) / CharWidthUser));
		const double lines = std::max<double>(MinTextLines, std::max<double>(textLines, std::ceil(static_cast<double>(textChars) / charsPerLine)));
		total += UserBubblePaddingY + lines * ProseLineHeight;
		++blocks;
		if(hasImage)
		{
			total += ImageThumbHeight;
			++blocks;
		}
		if(meta.sourceKind != "compaction"_L1)
		{
			total += MetaLineHeight;
			++blocks;
		}
		if(blocks > 1)
		{
			total += static_cast<double>(blocks - 1) * BlockSpacing;
		}
		return std::max(MinMessageHeight, std::round(total));
	}

	for(const proto::PartLayoutMeta& part : meta.parts)
	{
		total += estimatePartHeight(part, width, thinkingExpanded, detailsExpanded);
		++blocks;
	}
	if(meta.hasError)
	{
		total += ErrorCardHeight;
		++blocks;
	}
	total += MetaLineHeight;
	++blocks;
	if(blocks > 1)
	{
		total += static_cast<double>(blocks - 1) * BlockSpacing;
	}
	return std::max(MinMessageHeight, std::round(total));
}

QVariantList LayoutEstimator::skeletonParts(const proto::MessageLayoutMeta& meta)
{
	QVariantList list;
	list.reserve(meta.parts.size());
	for(const proto::PartLayoutMeta& part : meta.parts)
	{
		QVariantMap entry;
		entry.insert(u"kind"_s, part.kind);
		entry.insert(u"chars"_s, static_cast<int>(part.chars));
		entry.insert(u"lines"_s, static_cast<int>(part.lines));
		entry.insert(u"codeBlocks"_s, static_cast<int>(part.codeBlocks));
		entry.insert(u"codeLines"_s, static_cast<int>(part.codeLines));
		entry.insert(u"tables"_s, static_cast<int>(part.tables));
		entry.insert(u"tool"_s, part.tool.value_or(QString()));
		entry.insert(u"toolLines"_s, static_cast<int>(part.toolLines));
		entry.insert(u"width"_s, part.width ? static_cast<int>(*part.width) : 0);
		entry.insert(u"height"_s, part.height ? static_cast<int>(*part.height) : 0);
		list.append(std::move(entry));
	}
	return list;
}

} // namespace refgui::session
