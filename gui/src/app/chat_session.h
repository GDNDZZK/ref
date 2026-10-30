// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include "session/conversation_model.h"
#include "session/driver.h"

#include <QObject>
#include <QtQmlIntegration>

#include <memory>

// The one object QML talks to: the conversation and the actions on it.
// Created in C++ and handed to the window, which cannot create one itself.
class ChatSession : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_UNCREATABLE("Created in C++ and passed to Main as `session`.")
	Q_PROPERTY(QAbstractItemModel* blocks READ blocks CONSTANT)
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyChanged)

public:
	enum class Replies
	{
		FromModel, // the model in the configuration file
		FromMock,  // canned scripts, for working on the UI without a server
	};

	explicit ChatSession(Replies replies, QObject* parent = nullptr);
	~ChatSession() override;

	QAbstractItemModel* blocks();
	bool isBusy() const;

	// Precondition: !busy and `text` is not blank.
	Q_INVOKABLE void send(const QString& text);
	// Precondition: busy.
	Q_INVOKABLE void stop();

signals:
	void busyChanged();

private:
	refgui::session::ConversationModel m_model;
	std::unique_ptr<refgui::session::Driver> m_driver; // writes to m_model, so declared after it
};
