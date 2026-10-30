#pragma once

#include "ui/fence_splitter.h"

#include <QAbstractListModel>

#include <optional>

namespace qutie::ui
{

// The conversation as a flat list of blocks in display order.
//
//   user        "Fix the build"
//   reasoning   "The error says ..."
//   markdown    "The QML module is missing a file:"
//   code        "qt_add_qml_module(...)"        language = "cmake"
//   tool        "bash ctest"                    output, failed
//   error       "Connection failed: ..."
//
// A message is split into blocks because each kind has its own delegate (a code
// block needs a dedicated highlighted TextEdit) and animates independently.
// At most one block is streaming: the one currently receiving content.
// Rows are only ever appended or updated, never reset, so view transitions stay intact.
class ConversationModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		KindRole = Qt::UserRole + 1, // "user", "reasoning", "markdown", "code", "tool" or "error"
		ContentRole,                 // text; for a tool block, its title
		LanguageRole,                // code blocks: the fence's language, may be empty
		StreamingRole,               // still receiving content; for a tool block, still running
		OutputRole,                  // tool blocks
		FailedRole,                  // tool blocks
	};

	using QAbstractListModel::QAbstractListModel;

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

	// Precondition: no assistant turn is in progress.
	void appendUserMessage(const QString& text);

	// Assistant turn. Reasoning, text and tool calls may interleave; each switch between
	// them ends the current text segment, so a code fence never spans a tool call.
	void appendReasoning(QStringView delta);
	void appendAssistantText(QStringView delta);
	void beginToolCall(const QString& title);
	void endToolCall(bool succeeded, const QString& output); // completes the running tool call
	void endAssistantTurn();

	// Why a reply failed or was cut short, for the user. Precondition: no assistant
	// turn is in progress.
	void appendError(const QString& message);

private:
	enum class Kind
	{
		User,
		Reasoning,
		Markdown,
		Code,
		Tool,
		Error,
	};

	// Default initializers keep Clang's -Wmissing-field-initializers quiet when
	// designated initialization skips fields.
	struct Block
	{
		Kind kind;
		QString content{};
		QString language{};
		QString output{};
		bool failed = false;
	};

	static QString kindName(Kind kind);

	void applySplitEvent(const FenceSplitter::Event& event);
	void endTextSegment();

	void insertBlock(Block block, bool streaming);
	void startStreamingBlock(Block block);
	void appendToStreamingBlock(QStringView text);
	void finishStreamingBlock();
	bool isStreaming(Kind kind) const;
	void notifyChanged(qsizetype row, const QList<int>& roles);

	QList<Block> m_blocks;
	std::optional<qsizetype> m_streamingRow;
	FenceSplitter m_splitter; // splits the current text segment
};

} // namespace qutie::ui
