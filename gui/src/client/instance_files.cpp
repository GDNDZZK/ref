#include "client/instance_files.h"

#include "client/process_alive.h"
#include "proto/parse.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::client
{
namespace
{

QString homeDirectory()
{
	const QString home = qEnvironmentVariable("HOME");
	return home.isEmpty() ? QDir::homePath() : home;
}

std::optional<proto::InstanceInfo> readInstanceFile(const QString& path)
{
	QFile file(path);
	if(!file.open(QIODevice::ReadOnly))
	{
		return std::nullopt;
	}
	const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
	if(!document.isObject())
	{
		return std::nullopt;
	}
	const proto::Parsed<proto::InstanceInfo> parsed = proto::parseInstanceInfo(document.object());
	if(!parsed)
	{
		return std::nullopt;
	}
	return *parsed;
}

} // namespace

QString stateDirectory()
{
	if(const QString explicitDirectory = qEnvironmentVariable("REF_STATE_DIR"); !explicitDirectory.isEmpty())
	{
		return explicitDirectory;
	}
	if(const QString xdg = qEnvironmentVariable("XDG_STATE_HOME"); !xdg.isEmpty())
	{
		return xdg + u"/ref"_s;
	}
	return homeDirectory() + u"/.local/state/ref"_s;
}

QString instancesDirectory()
{
	return stateDirectory() + u"/instances"_s;
}

QList<proto::InstanceInfo> liveInstances(const QString& directory)
{
	QList<proto::InstanceInfo> live;
	const QFileInfoList files = QDir(directory).entryInfoList({u"*.json"_s}, QDir::Files);
	for(const QFileInfo& file : files)
	{
		const std::optional<proto::InstanceInfo> instance = readInstanceFile(file.absoluteFilePath());
		if(!instance)
		{
			continue;
		}
		if(!processIsAlive(instance->pid))
		{
			QFile::remove(file.absoluteFilePath());
			continue;
		}
		live.append(*instance);
	}
	std::ranges::sort(live, std::greater{}, &proto::InstanceInfo::startedAt);
	return live;
}

Endpoint endpointFor(const proto::InstanceInfo& instance)
{
	const bool wildcard = instance.bind.isEmpty() || instance.bind == u"0.0.0.0" || instance.bind == u"::" || instance.bind == u"[::]";
	const bool bareIpv6 = instance.bind.contains(u':') && !instance.bind.startsWith(u'[');

	QString host = instance.bind;
	if(wildcard)
	{
		host = u"127.0.0.1"_s;
	}
	else if(bareIpv6)
	{
		host = u'[' + instance.bind + u']';
	}
	return {.url = QUrl(u"ws://%1:%2/"_s.arg(host).arg(instance.port)), .token = instance.token};
}

} // namespace refgui::client
