#pragma once

#include <QObject>
#include <QStringView>

namespace refgui::session
{

// Produces the assistant's side of a conversation in a ConversationModel.
class Driver : public QObject
{
	Q_OBJECT

public:
	using QObject::QObject;

	// Replies to `prompt`, the user message just appended to the model.
	// Precondition: !isRunning().
	virtual void respond(QStringView prompt) = 0;
	// Ends the reply early. Precondition: isRunning().
	virtual void stop() = 0;
	virtual bool isRunning() const = 0;

signals:
	void runningChanged();
};

} // namespace refgui::session
