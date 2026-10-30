#pragma once

#include "llm/message.h"
#include "llm/openai_client.h"
#include "ui/driver.h"
#include "ui/stream_pacer.h"

#include <QCoroTask>
#include <QList>
#include <QString>
#include <QTimer>

#include <optional>
#include <stop_token>

namespace qutie::ui
{

class ConversationModel;

// Replies with the model configured in the file at `configPath`, and keeps the
// conversation's history for it. The history matches what the user sees: a reply
// that failed or was stopped stays in it as far as it got.
//
// The configuration is read at the first message, and again at each next one for as
// long as reading it fails. Any failure ends the turn with an error block.
class AgentDriver : public Driver
{
	Q_OBJECT

public:
	AgentDriver(ConversationModel& model, QString configPath, QObject* parent = nullptr);
	~AgentDriver() override;

	void respond(QStringView prompt) override;
	void stop() override;
	bool isRunning() const override;

private:
	QCoro::Task<> reply();
	void tick();
	void show(const QList<llm::StreamEvent>& events);
	void endTurn(const std::optional<QString>& notice);

	ConversationModel& m_model;
	QString m_configPath;
	std::optional<llm::OpenAiClient> m_client; // once the configuration has been read

	QList<llm::Message> m_history;
	bool m_running = false;
	bool m_destroying = false;
	// Fresh for each reply. Requesting a stop runs the rest of the reply before it
	// returns (see llm::OpenAiClient::complete()).
	std::stop_source m_stop;

	// Deltas go through the pacer, which the ticker drains once a frame. The turn ends
	// when the stream is over and the pacer has shown all of it; stopping shows the
	// rest at once.
	StreamPacer m_pacer;
	QTimer m_ticker;
	bool m_streamEnded = false;
	std::optional<QString> m_notice; // how the reply ended, for the user; once the stream ended
};

} // namespace qutie::ui
