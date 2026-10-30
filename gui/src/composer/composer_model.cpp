#include "composer/composer_model.h"

#include "compose/image_file.h"

#include <QBuffer>
#include <QClipboard>
#include <QGuiApplication>
#include <QImage>
#include <QMimeData>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::composer
{
namespace
{

const QString ShellPrefix = u"!"_s;
const QString PngMime = u"image/png"_s;
const QString PastedImageName = u"clipboard.png"_s;

QList<proto::ImageData> imageData(const QList<proto::NamedImage>& images)
{
	QList<proto::ImageData> result;
	for(const proto::NamedImage& image : images)
	{
		result.append({.mime = image.mime, .data = image.data});
	}
	return result;
}

} // namespace

ComposerModel::ComposerModel(client::Backend& backend, session::SessionController& controller, compose::PromptHistory& history, QObject* parent)
	: QObject(parent)
	, m_backend(backend)
	, m_controller(controller)
	, m_history(history)
	, m_browser(history)
{
	m_fileTimer.setSingleShot(true);
	m_fileTimer.setInterval(FileSearchDelay);
	connect(&m_fileTimer, &QTimer::timeout, this, [this] { findFiles(m_target.query, m_fileSerial); });
	m_interruptTimer.setSingleShot(true);
	m_interruptTimer.setInterval(DefaultInterruptWindow);
	connect(&m_interruptTimer, &QTimer::timeout, this, &ComposerModel::interruptArmedChanged);

	connect(&m_controller, &session::SessionController::queueChanged, this, [this] { m_queue.setMessages(m_controller.queued()); });
	connect(&m_controller, &session::SessionController::infoChanged, this, &ComposerModel::labelsChanged);
	connect(&m_controller, &session::SessionController::turnInProgressChanged, this, &ComposerModel::disarmInterrupt);
	connect(&m_backend, &client::Backend::stateChanged, this, &ComposerModel::readyChanged);
	connect(&m_backend, &client::Backend::connected, this, [this] { loadConfig(); });
	m_queue.setMessages(m_controller.queued());
	if(m_backend.state() == client::Backend::State::Connected)
	{
		loadConfig();
	}
}

bool ComposerModel::isReady() const
{
	return m_backend.state() == client::Backend::State::Connected && m_controller.isAttached();
}

QString ComposerModel::completionKind() const
{
	using Kind = compose::CompletionTarget::Kind;
	if(m_completionDismissed)
	{
		return u"none"_s;
	}
	switch(m_target.kind)
	{
	case Kind::Command:
		return u"command"_s;
	case Kind::File:
		return u"file"_s;
	case Kind::None:
		break;
	}
	return u"none"_s;
}

void ComposerModel::setInterruptWindow(int milliseconds)
{
	if(milliseconds == interruptWindow())
	{
		return;
	}
	m_interruptTimer.setInterval(milliseconds);
	emit interruptWindowChanged();
}

const proto::AgentInfo* ComposerModel::agentInfo() const
{
	const auto found = std::ranges::find(m_config.agents, m_controller.info().agent, &proto::AgentInfo::name);
	return found == m_config.agents.end() ? nullptr : &*found;
}

const proto::ModelInfo* ComposerModel::modelInfo() const
{
	const proto::ModelRef& ref = m_controller.info().model;
	const auto found = std::ranges::find_if(m_config.models, [&](const proto::ModelInfo& model) { return model.provider == ref.provider && model.id == ref.model; });
	return found == m_config.models.end() ? nullptr : &*found;
}

QString ComposerModel::agentName() const
{
	return m_controller.info().agent;
}

QString ComposerModel::agentColor() const
{
	const proto::AgentInfo* agent = agentInfo();
	return agent ? agent->color.value_or(QString()) : QString();
}

int ComposerModel::agentIndex() const
{
	const proto::AgentInfo* agent = agentInfo();
	return agent ? static_cast<int>(agent - m_config.agents.constData()) : 0;
}

QString ComposerModel::modelName() const
{
	const proto::ModelInfo* model = modelInfo();
	return model ? model->name : m_controller.info().model.model;
}

QString ComposerModel::modelDescription() const
{
	const proto::ModelInfo* model = modelInfo();
	if(!model)
	{
		return {};
	}
	return model->providerName + u" / "_s + model->id;
}

QString ComposerModel::variant() const
{
	return m_controller.info().model.variant.value_or(QString());
}

bool ComposerModel::variantAvailable() const
{
	const proto::ModelInfo* model = modelInfo();
	return model && !model->variants.isEmpty();
}

QCoro::Task<> ComposerModel::loadConfig()
{
	const auto config = co_await m_backend.client().getConfig();
	if(!config)
	{
		co_return;
	}
	m_config = *config;
	emit labelsChanged();
}

void ComposerModel::edit(const QString& text, int cursor)
{
	const bool typedBang = !m_shellMode && text == ShellPrefix && m_draft.text().isEmpty();
	if(typedBang)
	{
		setShellMode(true);
		changeDraft(); // the text area drops the `!`
		return;
	}
	if(text == m_draft.text() && cursor == m_draft.cursor())
	{
		return;
	}
	if(text != m_draft.text())
	{
		m_completionDismissed = false;
		m_browser.reset();
	}
	m_draft.setText(text, cursor);
	changeDraft();
}

void ComposerModel::moveCursor(int cursor)
{
	if(cursor == m_draft.cursor())
	{
		return;
	}
	m_draft.setCursor(cursor);
	updateCompletion();
	emit draftChanged();
}

void ComposerModel::changeDraft()
{
	m_attachments.setAttachments(m_draft.attachments());
	updateCompletion();
	emit draftChanged();
}

void ComposerModel::updateCompletion()
{
	const compose::CompletionTarget target = compose::completionTarget(m_draft.text(), m_draft.cursor(), m_shellMode);
	const bool wantsFiles = target.kind == compose::CompletionTarget::Kind::File && !m_completionDismissed;
	const bool queryChanged = target.kind != m_target.kind || target.query != m_target.query;
	m_target = target;
	if(!wantsFiles)
	{
		m_fileTimer.stop();
		++m_fileSerial; // answers still on their way are stale
		m_files.setPaths({});
	}
	else if(queryChanged)
	{
		++m_fileSerial;
		m_fileTimer.start();
	}
	emit completionChanged();
}

QCoro::Task<> ComposerModel::findFiles(QString query, quint64 serial)
{
	const auto result = co_await m_backend.client().listFiles(m_controller.info().cwd, std::move(query), FileLimit);
	if(serial != m_fileSerial || !result)
	{
		co_return;
	}
	m_files.setPaths(*result);
}

void ComposerModel::setShellMode(bool shellMode)
{
	if(shellMode == m_shellMode)
	{
		return;
	}
	m_shellMode = shellMode;
	emit shellModeChanged();
}

void ComposerModel::leaveShell()
{
	if(!m_shellMode)
	{
		return;
	}
	setShellMode(false);
	m_draft.clear();
	changeDraft();
}

bool ComposerModel::submit()
{
	if(m_draft.isEmpty())
	{
		return false;
	}
	if(!isReady())
	{
		emit failed(tr("Not connected to the ref backend."));
		return false;
	}
	m_browser.reset();
	disarmInterrupt();

	if(m_shellMode)
	{
		const QString command = m_draft.text().trimmed();
		m_history.push(ShellPrefix + command);
		setShellMode(false);
		m_draft.clear();
		changeDraft();
		sendShell(command);
		return true;
	}

	m_history.push(m_draft.text());
	QList<proto::NamedImage> images;
	for(const auto& attachment : m_draft.attachments())
	{
		images.append(attachment.image);
	}
	const QString text = m_draft.take().text.trimmed();
	changeDraft();
	sendPrompt(text, std::move(images), true);
	return true;
}

void ComposerModel::finishCommand()
{
	m_browser.reset();
	m_history.push(m_draft.text());
	m_draft.clear();
	changeDraft();
}

void ComposerModel::runCustomCommand(const QString& name, const QString& args)
{
	if(!isReady())
	{
		emit failed(tr("Not connected to the ref backend."));
		return;
	}
	expandAndSend(name, args);
}

QCoro::Task<> ComposerModel::sendPrompt(QString text, QList<proto::NamedImage> images, bool restoreOnFailure)
{
	const auto result = co_await m_backend.client().prompt(m_controller.sessionId(), text, imageData(images));
	if(result)
	{
		emit sent();
		co_return;
	}
	if(restoreOnFailure)
	{
		restoreDraft(text, images);
	}
	emit failed(result.error());
}

QCoro::Task<> ComposerModel::sendShell(QString command)
{
	const auto result = co_await m_backend.client().shell(m_controller.sessionId(), command);
	if(result)
	{
		emit sent();
		co_return;
	}
	emit failed(result.error());
}

QCoro::Task<> ComposerModel::expandAndSend(QString name, QString args)
{
	const auto expanded = co_await m_backend.client().expandCommand(std::move(name), std::move(args), m_controller.info().cwd);
	if(!expanded)
	{
		emit failed(expanded.error());
		co_return;
	}
	co_await sendPrompt(*expanded, {}, false);
}

bool ComposerModel::historyPrevious()
{
	const bool atStart = m_draft.text().isEmpty() || m_draft.cursor() == 0;
	if(!m_browser.browsing() && !atStart)
	{
		return false;
	}
	const QString current = m_shellMode ? ShellPrefix + m_draft.text() : m_draft.text();
	if(const auto entry = m_browser.previous(current))
	{
		fillFromHistory(*entry);
	}
	return m_browser.browsing();
}

bool ComposerModel::historyNext()
{
	const bool onLastLine = m_draft.text().indexOf(u'\n', m_draft.cursor()) < 0;
	if(!m_browser.browsing() || !onLastLine)
	{
		return false;
	}
	const auto entry = m_browser.next();
	if(!entry)
	{
		return false;
	}
	fillFromHistory(*entry);
	return true;
}

void ComposerModel::fillFromHistory(const QString& entry)
{
	const bool isShell = entry.startsWith(ShellPrefix);
	const QString text = isShell ? entry.mid(ShellPrefix.size()) : entry;
	setShellMode(isShell);
	m_draft.setText(text, text.size());
	m_completionDismissed = false;
	changeDraft();
}

void ComposerModel::withdraw(int row)
{
	const QString id = m_queue.idAt(row);
	if(id.isEmpty())
	{
		return;
	}
	m_queue.setRemoving(id, true);
	removeQueued(id);
}

QCoro::Task<> ComposerModel::removeQueued(QString messageId)
{
	const auto result = co_await m_backend.client().removeQueued(m_controller.sessionId(), messageId);
	m_queue.setRemoving(messageId, false);
	if(!result)
	{
		emit failed(result.error());
		co_return;
	}
	if(!result->removed)
	{
		emit toastRequested(u"info"_s, tr("Already sent; the prompt can no longer be removed"));
		co_return;
	}
	restoreDraft(result->text, result->images);
}

void ComposerModel::restoreDraft(const QString& text, const QList<proto::NamedImage>& images)
{
	if(m_shellMode)
	{
		setShellMode(false);
	}
	m_draft.restore(text, images);
	m_browser.reset();
	changeDraft();
}

void ComposerModel::clearDraft()
{
	if(m_draft.text().size() >= DiscardKeepLength)
	{
		m_history.push(m_shellMode ? ShellPrefix + m_draft.text() : m_draft.text());
	}
	setShellMode(false);
	m_draft.clear();
	m_browser.reset();
	changeDraft();
}

bool ComposerModel::escape()
{
	if(completionKind() != u"none"_s)
	{
		dismissCompletion();
		return true;
	}
	if(m_shellMode)
	{
		leaveShell();
		return true;
	}
	if(!m_controller.isTurnInProgress())
	{
		return false;
	}
	if(m_interruptTimer.isActive())
	{
		disarmInterrupt();
		emit interruptRequested();
		return true;
	}
	m_interruptTimer.start();
	emit interruptArmedChanged();
	return true;
}

void ComposerModel::disarmInterrupt()
{
	if(!m_interruptTimer.isActive())
	{
		return;
	}
	m_interruptTimer.stop();
	emit interruptArmedChanged();
}

void ComposerModel::dismissCompletion()
{
	if(m_completionDismissed)
	{
		return;
	}
	m_completionDismissed = true;
	updateCompletion();
}

void ComposerModel::acceptFile(int row)
{
	const QString path = m_files.pathAt(row);
	if(path.isEmpty() || m_target.kind != compose::CompletionTarget::Kind::File)
	{
		return;
	}
	const QString& text = m_draft.text();
	const QString mention = u"@"_s + path + u" "_s;
	qsizetype restStart = m_draft.cursor();
	if(text.mid(restStart).startsWith(u' '))
	{
		++restStart; // the space after the mention is already there
	}
	m_draft.setText(text.left(m_target.start) + mention + text.mid(restStart), m_target.start + mention.size());
	changeDraft();
}

void ComposerModel::completeCommand(const QString& name)
{
	const QString text = u"/"_s + name + u" "_s;
	m_draft.setText(text, text.size());
	m_completionDismissed = false;
	changeDraft();
}

bool ComposerModel::pasteFromClipboard()
{
	const QMimeData* data = QGuiApplication::clipboard()->mimeData();
	if(!data)
	{
		return false;
	}
	if(data->hasUrls() && attachUrls(data->urls()) > 0)
	{
		return true;
	}
	if(data->hasText())
	{
		const auto file = compose::imageFromPath(data->text());
		if(!file)
		{
			emit failed(file.error());
			return true;
		}
		if(!*file)
		{
			return false; // plain text
		}
		attachImage(**file);
		return true;
	}
	if(!data->hasImage())
	{
		return false;
	}
	const QImage image = qvariant_cast<QImage>(data->imageData());
	QByteArray bytes;
	QBuffer buffer(&bytes);
	if(image.isNull() || !image.save(&buffer, "PNG"))
	{
		return false;
	}
	if(bytes.size() > compose::MaxImageBytes)
	{
		emit failed(tr("The image is larger than 20 MiB."));
		return true;
	}
	attachImage(compose::imageFromBytes(bytes, PngMime, PastedImageName));
	return true;
}

int ComposerModel::attachUrls(const QList<QUrl>& urls)
{
	int attached = 0;
	for(const QUrl& url : urls)
	{
		const auto file = compose::imageFromPath(url.toString(QUrl::FullyEncoded));
		if(!file)
		{
			emit failed(file.error());
			continue;
		}
		if(*file)
		{
			attachImage(**file);
			++attached;
		}
	}
	return attached;
}

void ComposerModel::attachImage(proto::NamedImage image)
{
	m_draft.attach(std::move(image));
	changeDraft();
}

void ComposerModel::removeAttachment(int row)
{
	if(row < 0 || row >= m_draft.attachments().size())
	{
		return;
	}
	m_draft.removeAttachment(row);
	changeDraft();
}

} // namespace refgui::composer
