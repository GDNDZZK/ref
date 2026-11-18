#include "composer/composer_model.h"

#include "client/result.h"
#include "compose/image_file.h"
#include "workspace/command_registry.h"
#include "workspace/session_directory.h"
#include "workspace/toast_text.h"

#include <QBuffer>
#include <QClipboard>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QLoggingCategory>
#include <QMimeData>
#include <QPointer>
#include <QSet>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::composer
{
namespace
{

Q_LOGGING_CATEGORY(lcComposer, "refgui.composer")

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

struct SplitPath
{
	QString dirToRequest;
	QString typedPrefix;
	QString leafPrefix;
};

SplitPath splitPathQuery(const QString& query)
{
	if(query == u"~"_s)
	{
		return {.dirToRequest = u"~"_s, .typedPrefix = u"~/"_s, .leafPrefix = {}};
	}
	const qsizetype slash = query.lastIndexOf(u'/');
	if(slash < 0)
	{
		return {.dirToRequest = u"."_s, .typedPrefix = {}, .leafPrefix = query};
	}
	const QString prefix = query.left(slash + 1);
	QString dir = prefix;
	if(prefix == u"./"_s)
	{
		dir = u"."_s;
	}
	else if(prefix == u"~/"_s)
	{
		dir = u"~"_s;
	}
	return {.dirToRequest = dir, .typedPrefix = prefix, .leafPrefix = query.mid(slash + 1)};
}

QString sessionTokenValue(const proto::SessionInfo& info)
{
	if(!info.name.isEmpty() && !std::ranges::any_of(info.name, [](QChar c) { return c.isSpace(); }))
	{
		return info.name;
	}
	return info.id;
}

QString sessionDetail(const proto::SessionInfo& info)
{
	if(!info.title.isEmpty())
	{
		return info.title.simplified();
	}
	return info.cwd;
}

} // namespace

ComposerModel::ComposerModel(const workspace::BackendConfig& config,
                             session::SessionController& controller,
                             compose::PromptHistory& history,
                             QObject* parent,
                             const workspace::SessionDirectory* directory)
	: QObject(parent)
	, m_config(config)
	, m_controller(controller)
	, m_history(history)
	, m_browser(history)
{
	m_fileTimer.setSingleShot(true);
	m_fileTimer.setInterval(FileSearchDelay);
	connect(&m_fileTimer, &QTimer::timeout, this, [this] { findFiles(m_target.query, m_fileSerial); });

	m_dirTimer.setSingleShot(true);
	connect(&m_dirTimer, &QTimer::timeout, this, [this] {
		findDirs(m_controller.info().cwd, m_pendingDir, m_pendingIncludeFiles, m_dirSerial);
	});

	m_binTimer.setSingleShot(true);
	connect(&m_binTimer, &QTimer::timeout, this, [this] { findBinaries(m_target.query, m_binSerial); });

	m_interruptTimer.setSingleShot(true);
	m_interruptTimer.setInterval(DefaultInterruptWindow);
	connect(&m_interruptTimer, &QTimer::timeout, this, &ComposerModel::interruptArmedChanged);

	connect(&m_controller, &session::SessionController::queueChanged, this, [this] { m_queue.setMessages(m_controller.queued()); });
	connect(&m_controller, &session::SessionController::infoChanged, this, [this] {
		emit labelsChanged();
		if(m_target.kind != compose::CompletionTarget::Kind::None)
		{
			updateCompletion();
		}
	});
	connect(&m_controller, &session::SessionController::turnInProgressChanged, this, &ComposerModel::disarmInterrupt);
	connect(&m_controller, &session::SessionController::connectionChanged, this, &ComposerModel::readyChanged);
	connect(&m_config, &workspace::BackendConfig::changed, this, [this] {
		emit labelsChanged();
		if(m_target.kind == compose::CompletionTarget::Kind::CommandArg)
		{
			updateCompletion();
		}
	});
	setSessionDirectory(directory);
	m_queue.setMessages(m_controller.queued());
}

void ComposerModel::setSessionDirectory(const workspace::SessionDirectory* directory)
{
	if(m_directory == directory)
	{
		return;
	}
	if(m_directory)
	{
		disconnect(m_directory, nullptr, this, nullptr);
	}
	m_directory = directory;
	if(m_directory)
	{
		const auto refreshIfSessions = [this] {
			if(m_target.kind == compose::CompletionTarget::Kind::CommandArg
			   && (m_target.command == u"sessions"_s || m_target.command == u"close-session"_s))
			{
				updateCompletion();
			}
		};
		connect(m_directory, &workspace::SessionDirectory::liveChanged, this, refreshIfSessions);
		connect(m_directory, &workspace::SessionDirectory::historyLoadedChanged, this, refreshIfSessions);
	}
}

bool ComposerModel::isReady() const
{
	return m_controller.isConnected() && m_controller.isAttached();
}

bool ComposerModel::isCompletionOpen() const
{
	return !m_completionDismissed && m_target.kind != compose::CompletionTarget::Kind::None;
}

bool ComposerModel::hasVisibleCompletion() const
{
	if(!isCompletionOpen())
	{
		return false;
	}
	using Kind = compose::CompletionTarget::Kind;
	switch(m_target.kind)
	{
	case Kind::None:
		return false;
	case Kind::Command:
	case Kind::File:
		return true;
	case Kind::CommandArg:
	case Kind::ShellCommand:
	case Kind::ShellPath:
		return m_completions.rowCount() > 0;
	}
	Q_UNREACHABLE_RETURN(false);
}

QString ComposerModel::completionKind() const
{
	using Kind = compose::CompletionTarget::Kind;
	switch(isCompletionOpen() ? m_target.kind : Kind::None)
	{
	case Kind::None:
		return u"none"_s;
	case Kind::Command:
		return u"command"_s;
	case Kind::CommandArg:
		return u"argument"_s;
	case Kind::File:
		return u"file"_s;
	case Kind::ShellCommand:
		return u"shellCommand"_s;
	case Kind::ShellPath:
		return u"shellPath"_s;
	}
	Q_UNREACHABLE_RETURN(u"none"_s);
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
	const auto found = std::ranges::find(m_config.config().agents, m_controller.info().agent, &proto::AgentInfo::name);
	return found == m_config.config().agents.end() ? nullptr : &*found;
}

const proto::ModelInfo* ComposerModel::modelInfo() const
{
	const proto::ModelRef& ref = m_controller.info().model;
	const auto found = std::ranges::find_if(m_config.config().models, [&](const proto::ModelInfo& model) { return model.provider == ref.provider && model.id == ref.model; });
	return found == m_config.config().models.end() ? nullptr : &*found;
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
	return agent ? static_cast<int>(agent - m_config.config().agents.constData()) : NoAgentIndex;
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
		m_forceShellEmpty = false;
		m_completionDismissed = false;
		m_browser.reset();
	}
	m_draft.setText(text, cursor);
	draftEdited();
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

void ComposerModel::draftEdited()
{
	m_attachments.setAttachments(m_draft.attachments());
	updateCompletion();
	emit draftChanged();
}

void ComposerModel::changeDraft()
{
	draftEdited();
	emit draftReplaced();
}

void ComposerModel::cancelFileSearch()
{
	m_fileTimer.stop();
	++m_fileSerial;
	m_fileQuery.reset();
	m_files.setPaths({});
}

void ComposerModel::cancelDirSearch()
{
	m_dirTimer.stop();
	++m_dirSerial;
	m_dirKey.reset();
}

void ComposerModel::cancelBinarySearch()
{
	m_binTimer.stop();
	++m_binSerial;
	m_binQuery.reset();
}

void ComposerModel::triggerCompletion()
{
	m_completionDismissed = false;
	if(m_shellMode && m_draft.text().trimmed().isEmpty())
	{
		m_forceShellEmpty = true;
	}
	updateCompletion();
}

void ComposerModel::updateCompletion()
{
	using Kind = compose::CompletionTarget::Kind;
	const compose::CompletionTarget target = compose::completionTarget(m_draft.text(), m_draft.cursor(), m_shellMode, m_forceShellEmpty);
	m_target = target;

	if(m_completionDismissed || target.kind == Kind::None)
	{
		cancelFileSearch();
		cancelDirSearch();
		cancelBinarySearch();
		m_completions.setEntries({});
		emit completionChanged();
		return;
	}

	if(target.kind != Kind::File)
	{
		cancelFileSearch();
	}
	const bool isPathKind = target.kind == Kind::ShellPath
	                        || (target.kind == Kind::CommandArg && (target.command == u"cd"_s || target.command == u"export"_s));
	if(!isPathKind)
	{
		cancelDirSearch();
	}
	if(target.kind != Kind::ShellCommand)
	{
		cancelBinarySearch();
	}

	switch(target.kind)
	{
	case Kind::None:
		break;
	case Kind::Command:
		m_completions.setEntries({});
		break;
	case Kind::File:
		m_completions.setEntries({});
		if(m_fileQuery != target.query)
		{
			m_fileQuery = target.query;
			++m_fileSerial;
			m_fileTimer.start();
		}
		break;
	case Kind::CommandArg:
		updateArgCompletion();
		break;
	case Kind::ShellCommand:
		updateShellCommandCompletion();
		break;
	case Kind::ShellPath:
		updatePathCompletion(true, false);
		break;
	}

	emit completionChanged();
}

void ComposerModel::updateArgCompletion()
{
	const QString& cmd = m_target.command;
	const QString query = m_target.query.trimmed();

	if(cmd == u"cd"_s)
	{
		updatePathCompletion(false, false);
		return;
	}
	if(cmd == u"export"_s)
	{
		updatePathCompletion(true, true);
		return;
	}

	QList<CompletionEntry> prefixHits;
	QList<CompletionEntry> subHits;

	if(cmd == u"variant"_s)
	{
		const proto::ModelInfo* model = modelInfo();
		const QStringList rawVariants = model ? model->variants : QStringList();
		const QString cur = variant();
		const QString defaultLabel = tr("default");
		if(query.isEmpty() || u"default"_s.startsWith(query, Qt::CaseInsensitive) || defaultLabel.startsWith(query, Qt::CaseInsensitive))
		{
			prefixHits.append({.value = u"default"_s,
			                   .label = defaultLabel,
			                   .detail = cur.isEmpty() ? tr("active") : QString(),
			                   .icon = u"variant"_s,
			                   .mono = true});
		}
		for(const QString& v : rawVariants)
		{
			const CompletionEntry entry{.value = v,
			                            .label = v,
			                            .detail = cur == v ? tr("active") : QString(),
			                            .icon = u"variant"_s,
			                            .mono = true};
			if(v.startsWith(query, Qt::CaseInsensitive))
			{
				prefixHits.append(entry);
			}
			else if(v.contains(query, Qt::CaseInsensitive))
			{
				subHits.append(entry);
			}
		}
		m_completions.setEntries(prefixHits + subHits);
		return;
	}

	if(cmd == u"models"_s)
	{
		const qsizetype hash = query.lastIndexOf(u'#');
		const QString modelFilter = hash >= 0 ? query.left(hash) : query;
		const std::optional<QString> variantFilter = hash >= 0 ? std::optional<QString>(query.mid(hash + 1)) : std::nullopt;
		const proto::ModelRef& curRef = m_controller.info().model;

		for(const proto::ModelInfo& m : m_config.config().models)
		{
			const QString fullId = m.provider + u"/"_s + m.id;
			const bool isPrefix = fullId.startsWith(modelFilter, Qt::CaseInsensitive)
			                      || m.id.startsWith(modelFilter, Qt::CaseInsensitive)
			                      || m.name.startsWith(modelFilter, Qt::CaseInsensitive);
			const bool isSub = isPrefix
			                   || fullId.contains(modelFilter, Qt::CaseInsensitive)
			                   || m.name.contains(modelFilter, Qt::CaseInsensitive)
			                   || m.providerName.contains(modelFilter, Qt::CaseInsensitive);
			if(!isSub)
			{
				continue;
			}
			const QString detailBase = m.providerName + (m.name.isEmpty() || m.name == m.id ? QString() : u" · "_s + m.name);
			if(variantFilter)
			{
				for(const QString& v : m.variants)
				{
					if(v.startsWith(*variantFilter, Qt::CaseInsensitive) || v.contains(*variantFilter, Qt::CaseInsensitive))
					{
						const QString spec = fullId + u"#"_s + v;
						(v.startsWith(*variantFilter, Qt::CaseInsensitive) ? prefixHits : subHits)
							.append({.value = spec, .label = spec, .detail = detailBase, .icon = u"model"_s, .mono = true});
					}
				}
			}
			else
			{
				const bool active = curRef.provider == m.provider && curRef.model == m.id;
				(isPrefix ? prefixHits : subHits)
					.append({.value = fullId,
					         .label = fullId,
					         .detail = active ? detailBase + u" · "_s + tr("active") : detailBase,
					         .icon = u"model"_s,
					         .mono = true});
			}
		}
		m_completions.setEntries(prefixHits + subHits);
		return;
	}

	if(cmd == u"agents"_s)
	{
		const QString cur = agentName();
		for(const proto::AgentInfo& a : m_config.config().agents)
		{
			const bool isPrefix = a.name.startsWith(query, Qt::CaseInsensitive);
			const bool isSub = isPrefix || a.name.contains(query, Qt::CaseInsensitive) || a.description.contains(query, Qt::CaseInsensitive);
			if(!isSub)
			{
				continue;
			}
			QString detail = a.description;
			if(a.name == cur)
			{
				detail = detail.isEmpty() ? tr("active") : detail + u" · "_s + tr("active");
			}
			(isPrefix ? prefixHits : subHits)
				.append({.value = a.name, .label = a.name, .detail = detail, .icon = u"agent"_s, .mono = true});
		}
		m_completions.setEntries(prefixHits + subHits);
		return;
	}

	if(cmd == u"sessions"_s || cmd == u"close-session"_s)
	{
		if(!m_directory)
		{
			m_completions.setEntries({});
			return;
		}
		const bool closeMode = cmd == u"close-session"_s;
		QSet<QString> alreadyPicked;
		if(closeMode)
		{
			const QString& fullText = m_draft.text();
			qsizetype spaceIdx = 0;
			while(spaceIdx < fullText.size() && fullText[spaceIdx].isSpace())
			{
				++spaceIdx;
			}
			while(spaceIdx < fullText.size() && !fullText[spaceIdx].isSpace())
			{
				++spaceIdx;
			}
			const QString earlier = fullText.mid(spaceIdx, m_target.start - spaceIdx);
			for(const QString& tok : earlier.split(u' ', Qt::SkipEmptyParts))
			{
				alreadyPicked.insert(tok.toLower());
			}
		}

		const QList<proto::SessionInfo> list = closeMode ? m_directory->liveSessions() : m_directory->allSessions();
		for(const proto::SessionInfo& s : list)
		{
			if(closeMode && (alreadyPicked.contains(s.id.toLower()) || (!s.name.isEmpty() && alreadyPicked.contains(s.name.toLower()))))
			{
				continue;
			}
			const QString label = !s.name.isEmpty() ? s.name : s.id;
			const bool isPrefix = label.startsWith(query, Qt::CaseInsensitive) || s.id.startsWith(query, Qt::CaseInsensitive);
			const bool isSub = isPrefix
			                   || label.contains(query, Qt::CaseInsensitive)
			                   || s.id.contains(query, Qt::CaseInsensitive)
			                   || s.title.contains(query, Qt::CaseInsensitive);
			if(!isSub)
			{
				continue;
			}
			QString detail = sessionDetail(s);
			if(!closeMode)
			{
				const QString stateTag = s.live ? tr("live") : tr("history");
				detail = detail.isEmpty() ? stateTag : detail + u" · "_s + stateTag;
			}
			(isPrefix ? prefixHits : subHits)
				.append({.value = sessionTokenValue(s),
				         .label = label,
				         .detail = detail,
				         .icon = u"chat"_s,
				         .mono = true,
				         .trailingSpace = closeMode});
		}
		m_completions.setEntries(prefixHits + subHits);
		return;
	}

	m_completions.setEntries({});
}

void ComposerModel::updatePathCompletion(bool includeFiles, bool isExport)
{
	const SplitPath split = splitPathQuery(m_target.query);
	const QString& cwd = m_controller.info().cwd;

	if(m_dirCache.valid
	   && m_dirCache.cwd == cwd
	   && m_dirCache.dir == split.dirToRequest
	   && (m_dirCache.includeFiles == includeFiles || (!includeFiles && m_dirCache.includeFiles)))
	{
		cancelDirSearch();
		m_completions.setEntries(buildPathEntries(split.typedPrefix, split.leafPrefix, m_dirCache.dirs, m_dirCache.files, includeFiles, isExport));
		return;
	}

	if(isExport)
	{
		m_completions.setEntries(buildPathEntries(split.typedPrefix, split.leafPrefix, {}, {}, includeFiles, true));
	}
	else if(!m_dirCache.valid || m_dirCache.dir != split.dirToRequest)
	{
		m_completions.setEntries({});
	}

	const QString key = cwd + u'\0' + split.dirToRequest + u'\0' + (includeFiles ? u"1"_s : u"0"_s);
	if(m_dirKey != key)
	{
		m_dirKey = key;
		m_pendingDir = split.dirToRequest;
		m_pendingIncludeFiles = includeFiles;
		++m_dirSerial;
		m_dirTimer.setInterval(split.leafPrefix.isEmpty() ? std::chrono::milliseconds{0} : DirSearchDelay);
		m_dirTimer.start();
	}
}

QList<CompletionEntry> ComposerModel::buildPathEntries(const QString& typedPrefix,
                                                       const QString& leafPrefix,
                                                       const QStringList& dirs,
                                                       const QStringList& files,
                                                       bool includeFiles,
                                                       bool isExport) const
{
	QList<CompletionEntry> result;
	QString defaultExportName;
	if(isExport && !m_controller.sessionId().isEmpty())
	{
		defaultExportName = u"ref-session-"_s + m_controller.sessionId() + u".md"_s;
		if(defaultExportName.startsWith(leafPrefix, Qt::CaseInsensitive))
		{
			result.append({.value = typedPrefix + defaultExportName,
			               .label = defaultExportName,
			               .prefixLabel = typedPrefix,
			               .detail = tr("default"),
			               .icon = u"file-text"_s,
			               .isDirectory = false,
			               .mono = true,
			               .trailingSpace = false});
		}
	}

	for(const QString& dir : dirs)
	{
		if(!dir.startsWith(leafPrefix, Qt::CaseInsensitive))
		{
			continue;
		}
		// Skip dotfiles unless the user explicitly started typing `.`.
		if(dir.startsWith(u'.') && !leafPrefix.startsWith(u'.'))
		{
			continue;
		}
		result.append({.value = typedPrefix + dir + u"/"_s,
		               .label = dir,
		               .prefixLabel = typedPrefix,
		               .detail = {},
		               .icon = u"folder"_s,
		               .isDirectory = true,
		               .mono = true,
		               .trailingSpace = false});
	}

	if(includeFiles)
	{
		const bool addTrailingSpace = m_target.kind == compose::CompletionTarget::Kind::ShellPath;
		for(const QString& file : files)
		{
			if(!file.startsWith(leafPrefix, Qt::CaseInsensitive))
			{
				continue;
			}
			if(file.startsWith(u'.') && !leafPrefix.startsWith(u'.'))
			{
				continue;
			}
			if(isExport && file == defaultExportName)
			{
				continue;
			}
			result.append({.value = typedPrefix + file,
			               .label = file,
			               .prefixLabel = typedPrefix,
			               .detail = {},
			               .icon = u"file"_s,
			               .isDirectory = false,
			               .mono = true,
			               .trailingSpace = addTrailingSpace});
		}
	}

	return result;
}

void ComposerModel::updateShellCommandCompletion()
{
	const QString& query = m_target.query;
	if(m_binCache.valid && m_binCache.query == query)
	{
		cancelBinarySearch();
		m_completions.setEntries(buildShellCommandEntries(query, m_binCache.binaries));
		return;
	}

	QStringList interimBinaries;
	if(m_binCache.valid && !m_binCache.query.isEmpty() && query.startsWith(m_binCache.query, Qt::CaseInsensitive))
	{
		for(const QString& bin : m_binCache.binaries)
		{
			if(bin.contains(query, Qt::CaseInsensitive))
			{
				interimBinaries.append(bin);
			}
		}
	}
	m_completions.setEntries(buildShellCommandEntries(query, interimBinaries));

	if(m_binQuery != query)
	{
		m_binQuery = query;
		++m_binSerial;
		m_binTimer.setInterval(m_forceShellEmpty ? std::chrono::milliseconds{0} : BinarySearchDelay);
		m_binTimer.start();
	}
}

QList<CompletionEntry> ComposerModel::buildShellCommandEntries(const QString& query, const QStringList& binaries) const
{
	QList<CompletionEntry> historyPrefix;
	QList<CompletionEntry> historySub;
	QSet<QString> seenHistory;
	const QStringList& allHistory = m_history.items();
	for(qsizetype idx = allHistory.size() - 1; idx >= 0; --idx)
	{
		const QString& raw = allHistory.at(idx);
		if(!raw.startsWith(ShellPrefix))
		{
			continue;
		}
		const QString cmdLine = raw.mid(ShellPrefix.size()).trimmed();
		if(cmdLine.isEmpty() || seenHistory.contains(cmdLine))
		{
			continue;
		}
		seenHistory.insert(cmdLine);
		if(query.isEmpty() || cmdLine.startsWith(query, Qt::CaseInsensitive))
		{
			historyPrefix.append({.value = cmdLine,
			                      .label = cmdLine,
			                      .detail = tr("history"),
			                      .icon = u"history"_s,
			                      .mono = true,
			                      .trailingSpace = false});
		}
		else if(cmdLine.contains(query, Qt::CaseInsensitive))
		{
			historySub.append({.value = cmdLine,
			                   .label = cmdLine,
			                   .detail = tr("history"),
			                   .icon = u"history"_s,
			                   .mono = true,
			                   .trailingSpace = false});
		}
	}

	QList<CompletionEntry> result = historyPrefix + historySub;
	if(result.size() > MaxShellHistoryMatches)
	{
		result.resize(MaxShellHistoryMatches);
	}

	for(const QString& bin : binaries)
	{
		if(seenHistory.contains(bin))
		{
			continue;
		}
		result.append({.value = bin,
		               .label = bin,
		               .detail = {},
		               .icon = u"terminal"_s,
		               .mono = true,
		               .trailingSpace = true});
	}
	return result;
}

QCoro::Task<> ComposerModel::findFiles(QString query, quint64 serial)
{
	const QPointer<ComposerModel> self(this); // the model can be gone when the answer arrives
	const auto result = co_await m_controller.listFiles(std::move(query), FileLimit);
	if(!self || serial != m_fileSerial)
	{
		co_return;
	}
	if(!result)
	{
		// No toast: this runs while typing. The list of the previous query would be wrong now.
		if(!client::isConnectionLost(result.error()))
		{
			qCWarning(lcComposer) << "file search failed:" << result.error();
		}
		m_files.setPaths({});
		co_return;
	}
	m_files.setPaths(*result);
}

QCoro::Task<> ComposerModel::findDirs(QString cwd, QString dir, bool includeFiles, quint64 serial)
{
	const QPointer<ComposerModel> self(this);
	const auto result = co_await m_controller.listDirs(dir, includeFiles);
	if(!self || serial != m_dirSerial)
	{
		co_return;
	}
	m_dirKey.reset();
	const bool isExport = m_target.kind == compose::CompletionTarget::Kind::CommandArg && m_target.command == u"export"_s;
	const SplitPath split = splitPathQuery(m_target.query);
	if(!result)
	{
		m_dirCache = {};
		m_completions.setEntries(isExport ? buildPathEntries(split.typedPrefix, split.leafPrefix, {}, {}, includeFiles, true) : QList<CompletionEntry>{});
		co_return;
	}
	m_dirCache = {.cwd = std::move(cwd),
	              .dir = std::move(dir),
	              .includeFiles = includeFiles,
	              .dirs = result->dirs,
	              .files = result->files,
	              .valid = true};
	if(split.dirToRequest == m_dirCache.dir)
	{
		m_completions.setEntries(buildPathEntries(split.typedPrefix, split.leafPrefix, m_dirCache.dirs, m_dirCache.files, includeFiles, isExport));
	}
}

QCoro::Task<> ComposerModel::findBinaries(QString query, quint64 serial)
{
	const QPointer<ComposerModel> self(this);
	const auto result = co_await m_controller.listBinaries(query, BinaryLimit);
	if(!self || serial != m_binSerial)
	{
		co_return;
	}
	m_binQuery.reset();
	if(!result)
	{
		co_return;
	}
	m_binCache = {.query = query, .binaries = *result, .valid = true};
	if(m_target.kind == compose::CompletionTarget::Kind::ShellCommand && m_target.query == query)
	{
		m_completions.setEntries(buildShellCommandEntries(query, m_binCache.binaries));
	}
}

void ComposerModel::setShellMode(bool shellMode)
{
	if(shellMode == m_shellMode)
	{
		return;
	}
	m_shellMode = shellMode;
	if(!m_shellMode)
	{
		m_forceShellEmpty = false;
	}
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
	if(!m_shellMode && m_draft.attachments().isEmpty() && workspace::CommandRegistry::isSingleSlashToken(m_draft.text()))
	{
		const QString typed = m_draft.text().trimmed().mid(1);
		const QString suggestion = workspace::CommandRegistry::suggestSlashFrom(typed, m_config.config().commands);
		const QString message = suggestion.isEmpty()
			? tr("Unknown command /%1").arg(typed)
			: tr("Unknown command /%1 — did you mean /%2?").arg(typed, suggestion);
		emit toastRequested(workspace::ToastLevel::Warn, message);
		return false;
	}
	if(!isReady())
	{
		emit failed(tr("Not connected to the ref backend."));
		return false;
	}
	m_forceShellEmpty = false;
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
	m_forceShellEmpty = false;
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

template<typename T>
QCoro::Task<bool> ComposerModel::awaitAndReport(QCoro::Task<client::Result<T>> request, std::function<void()> onFailure)
{
	const QPointer<ComposerModel> self(this); // the model can be gone when the answer arrives
	const auto result = co_await std::move(request);
	if(!self)
	{
		co_return false;
	}
	if(result)
	{
		co_return true;
	}
	if(onFailure)
	{
		onFailure();
	}
	emit failed(result.error());
	co_return false;
}

QCoro::Task<> ComposerModel::sendPrompt(QString text, QList<proto::NamedImage> images, bool restoreOnFailure)
{
	const auto restore = [&] {
		if(restoreOnFailure)
		{
			restoreDraft(text, images);
		}
	};
	if(co_await awaitAndReport(m_controller.send(text, imageData(images)), restore))
	{
		emit sent();
	}
}

QCoro::Task<> ComposerModel::sendShell(QString command)
{
	// Back into the box as a shell line, so nothing typed is lost.
	const auto restore = [&] {
		restoreDraft(command, {});
		setShellMode(true);
	};
	if(co_await awaitAndReport(m_controller.runShell(command), restore))
	{
		emit sent();
	}
}

QCoro::Task<> ComposerModel::expandAndSend(QString name, QString args)
{
	const QPointer<ComposerModel> self(this); // the model can be gone when the answer arrives
	const auto expanded = co_await m_controller.expandCommand(std::move(name), std::move(args));
	if(!self)
	{
		co_return;
	}
	if(!expanded)
	{
		emit failed(expanded.error());
		co_return;
	}
	if(expanded->trimmed().isEmpty())
	{
		emit toastRequested(workspace::ToastLevel::Warn, tr("The command expanded to nothing"));
		co_return;
	}
	co_await sendPrompt(*expanded, {}, false);
}

bool ComposerModel::historyPrevious()
{
	const bool atStart = m_draft.text().isEmpty() || m_draft.cursor() == 0 || (m_shellMode && !m_draft.text().contains(u'\n'));
	if(!m_browser.browsing() && !atStart)
	{
		return false;
	}
	const QString current = m_shellMode ? ShellPrefix + m_draft.text() : m_draft.text();
	if(const auto entry = m_browser.previous(current, m_shellMode))
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
	m_forceShellEmpty = false;
	setShellMode(isShell);
	m_draft.setText(text, text.size());
	m_completionDismissed = isShell; // avoid popping binary completions while walking shell history
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
	const QPointer<ComposerModel> self(this); // the model can be gone when the answer arrives
	const auto result = co_await m_controller.removeQueued(messageId);
	if(!self)
	{
		co_return;
	}
	m_queue.setRemoving(messageId, false);
	if(!result)
	{
		emit failed(result.error());
		co_return;
	}
	if(!result->removed)
	{
		emit toastRequested(workspace::ToastLevel::Info, tr("Already sent; the prompt can no longer be removed"));
		co_return;
	}
	restoreDraft(result->text, result->images);
}

void ComposerModel::restoreDraft(const QString& text, const QList<proto::NamedImage>& images)
{
	m_forceShellEmpty = false;
	setShellMode(false);
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
	m_forceShellEmpty = false;
	setShellMode(false);
	m_draft.clear();
	m_browser.reset();
	changeDraft();
}

bool ComposerModel::escape()
{
	if(hasVisibleCompletion())
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
	if(m_completionDismissed && !m_forceShellEmpty)
	{
		return;
	}
	m_forceShellEmpty = false;
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

bool ComposerModel::acceptCompletionItem(int row, bool viaEnter)
{
	const CompletionEntry* found = m_completions.entryAt(row);
	if(!found)
	{
		return false;
	}
	const CompletionEntry entry = *found;
	const compose::CompletionTarget target = m_target;
	const bool isCloseSession = target.kind == compose::CompletionTarget::Kind::CommandArg && target.command == u"close-session"_s;

	QString replacement = entry.value;
	const bool addSpace = !entry.isDirectory && entry.trailingSpace && !(isCloseSession && viaEnter);
	if(addSpace && !replacement.endsWith(u' '))
	{
		replacement += u' ';
	}

	const QString& current = m_draft.text();
	qsizetype restStart = target.end;
	if(addSpace && current.mid(restStart).startsWith(u' '))
	{
		++restStart;
	}

	const bool shouldSubmit = viaEnter && !entry.isDirectory && target.kind == compose::CompletionTarget::Kind::CommandArg;
	const bool keepOpen = entry.isDirectory
	                      || (target.kind == compose::CompletionTarget::Kind::ShellCommand && addSpace)
	                      || (isCloseSession && !viaEnter);

	m_forceShellEmpty = false;
	m_completionDismissed = !keepOpen;
	m_draft.setText(current.left(target.start) + replacement + current.mid(restStart), target.start + replacement.size());
	changeDraft();
	return shouldSubmit;
}

bool ComposerModel::pasteFromClipboard()
{
	const QMimeData* data = QGuiApplication::clipboard()->mimeData();
	if(!data)
	{
		return false;
	}
	if(data->hasUrls() && attachImageUrls(data->urls(), false) > 0)
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
		emit failed(tr("The image is larger than %1 MiB.").arg(compose::MaxImageMiB));
		return true;
	}
	attachImage(compose::imageFromBytes(bytes, PngMime, PastedImageName));
	return true;
}

int ComposerModel::attachUrls(const QList<QUrl>& urls)
{
	return attachImageUrls(urls, true);
}

int ComposerModel::attachImageUrls(const QList<QUrl>& urls, bool reportOthers)
{
	int handled = 0;
	bool pathInserted = false;
	for(const QUrl& url : urls)
	{
		const auto file = compose::imageFromUrl(url);
		if(!file)
		{
			emit failed(file.error());
			continue;
		}
		if(*file)
		{
			attachImage(**file);
			++handled;
			continue;
		}
		if(url.isLocalFile())
		{
			const QString localPath = url.toLocalFile();
			if(QFileInfo::exists(localPath))
			{
				m_draft.insertPath(localPath);
				pathInserted = true;
				++handled;
				continue;
			}
			if(reportOthers)
			{
				emit toastRequested(workspace::ToastLevel::Warn, tr("File not found: %1").arg(localPath));
			}
			continue;
		}
		if(reportOthers)
		{
			emit toastRequested(workspace::ToastLevel::Warn, tr("%1 is not a local file").arg(url.toDisplayString()));
		}
	}
	if(pathInserted)
	{
		changeDraft();
	}
	return handled;
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
