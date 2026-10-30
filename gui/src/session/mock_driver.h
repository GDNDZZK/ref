#pragma once

#include "session/driver.h"

#include <QDeadlineTimer>
#include <QList>
#include <QString>
#include <QTimer>

#include <chrono>
#include <variant>

namespace refgui::session
{

class ConversationModel;

// Stand-in for the agent while the UI is built: plays canned replies into the model
// at a token-like pace, so streaming, code blocks and tool calls can be exercised.
class MockDriver : public Driver
{
	Q_OBJECT

public:
	explicit MockDriver(ConversationModel& model, QObject* parent = nullptr);

	// Plays the script named `prompt` ("code", "tools", "zh", "slow", "long", "error"), or else
	// the next one in rotation.
	void respond(QStringView prompt) override;
	// Ends the reply early, as a user cancel would.
	void stop() override;
	bool isRunning() const override;

private:
	struct Pause
	{
		std::chrono::milliseconds duration;
	};
	struct Reasoning
	{
		QString text;
	};
	struct Text
	{
		QString text;
	};
	struct ToolCall
	{
		QString title;
		QString output;
		bool succeeded;
		std::chrono::milliseconds duration;
	};
	// Ends the reply with an error, as a failed request would. Last step of a script.
	struct Failure
	{
		QString message;
	};
	using Step = std::variant<Pause, Reasoning, Text, ToolCall, Failure>;

	// One chunk of 1 ≤ length < maxChunkLength characters per tick.
	struct Pace
	{
		std::chrono::milliseconds tickInterval;
		int maxChunkLength;
	};
	// One tick per frame, 1–7 characters per tick: ~250 characters (~60 tokens) per second.
	static constexpr Pace NormalPace{.tickInterval = std::chrono::milliseconds(16), .maxChunkLength = 8};
	// A slow local model: 1–4 characters every 150 ms, ~17 characters (~4 tokens) per second.
	static constexpr Pace SlowPace{.tickInterval = std::chrono::milliseconds(150), .maxChunkLength = 5};

	struct Script
	{
		QString name;
		Pace pace;
		QList<Step> steps;
	};

	static QList<Script> scripts();
	static Script codeScript();
	static Script toolsScript();
	static Script chineseScript();
	static Script slowScript();
	static Script longScript();
	static Script errorScript();

	void advance();
	bool streamChunk(const QString& text, void (ConversationModel::*append)(QStringView));
	bool waitFor(std::chrono::milliseconds duration);
	void finish();

	ConversationModel& m_model;
	QTimer m_ticker;
	QList<Script> m_scripts;
	qsizetype m_nextScript = 0;

	// Progress through the playing script.
	QList<Step> m_steps;
	int m_maxChunkLength = 0;
	qsizetype m_stepIndex = 0;
	qsizetype m_charIndex = 0;     // Reasoning and Text steps
	bool m_stepStarted = false;    // Pause and ToolCall steps
	QDeadlineTimer m_stepDeadline; // Pause and ToolCall steps
};

} // namespace refgui::session
