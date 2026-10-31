// A Qt generic plugin that lets a script drive an unmodified ref-gui and photograph it.
//
//   QT_QPA_GENERIC_PLUGINS=refpromo QT_PLUGIN_PATH=<build>/plugins ref-gui ...
//
// Qt loads the plugin into the ref-gui process at start-up. It reads one JSON command per line
// from stdin and answers each with one JSON line on stdout (see capture/lib/guiSession.ts).
//
//   {"cmd":"window"}                      -> {"ok":true,"width":W,"height":H} (null size until a window shows)
//   {"cmd":"resize","width":W,"height":H}
//   {"cmd":"texts"}                       -> {"ok":true,"texts":[{"text":..,"x":..,"y":..,"width":..,"height":..}]}
//   {"cmd":"grab","file":"a.png"}
//   {"cmd":"click","x":X,"y":Y}   {"cmd":"clickText","text":"New session"}   {"cmd":"move","x":X,"y":Y}
//   {"cmd":"key","sequence":"Ctrl+Shift+N"}   {"cmd":"type","text":"hello"}
//   {"cmd":"wheel","x":X,"y":Y,"delta":-120}
//   {"cmd":"record","dir":"d","fps":15}  photographs the window every 1000/fps ms into d/f00000.jpg ...
//   {"cmd":"stop"}                      -> {"ok":true,"frames":N,"times":[ms since record, ...]}
//   {"cmd":"quit"}
#include <QtCore/QCoreApplication>
#include <QtCore/QDir>
#include <QtCore/QElapsedTimer>
#include <QtCore/QThreadPool>
#include <QtCore/QTimer>
#include <QtCore/QJsonArray>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QSocketNotifier>
#include <QtGui/QGuiApplication>
#include <QtGui/QKeySequence>
#include <QtGui/QWheelEvent>
#include <QtGui/qgenericplugin.h>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>
#include <QtTest/QTest>
#include <unistd.h>

using namespace Qt::StringLiterals;

namespace
{

constexpr int StdinFd = 0;
constexpr size_t ReadChunkBytes = 65536;
constexpr int ClickPauseMs = 30;
constexpr int DefaultRecordFps = 15;
constexpr int RecordJpegQuality = 90;

QString errorReply(const QString& message)
{
	return QString::fromUtf8(QJsonDocument(QJsonObject{{"ok", false}, {"error", message}}).toJson(QJsonDocument::Compact));
}

// The biggest visible Qt Quick window: the application window, not a helper.
QQuickWindow* mainWindow()
{
	QQuickWindow* best = nullptr;
	for(QWindow* window : QGuiApplication::topLevelWindows())
	{
		auto* quick = qobject_cast<QQuickWindow*>(window);
		if(!quick || !quick->isVisible())
		{
			continue;
		}
		if(!best || quick->width() * quick->height() > best->width() * best->height())
		{
			best = quick;
		}
	}
	return best;
}

bool effectivelyVisible(const QQuickItem* item)
{
	for(const QQuickItem* current = item; current; current = current->parentItem())
	{
		if(!current->isVisible() || current->opacity() <= 0.01)
		{
			return false;
		}
	}
	return true;
}

struct TextItem
{
	QString text;
	QRectF rect;
};

void collectTexts(QQuickItem* item, const QRectF& windowRect, QList<TextItem>& out)
{
	if(!item->isVisible() || item->opacity() <= 0.01)
	{
		return;
	}
	QString text = item->property("text").toString();
	if(text.isEmpty())
	{
		text = item->property("placeholderText").toString();
	}
	if(!text.isEmpty() && item->width() > 0 && item->height() > 0 && effectivelyVisible(item))
	{
		const QRectF rect(item->mapToScene(QPointF(0, 0)), QSizeF(item->width(), item->height()));
		if(windowRect.intersects(rect))
		{
			out.append({text, rect});
		}
	}
	for(QQuickItem* child : item->childItems())
	{
		collectTexts(child, windowRect, out);
	}
}

QList<TextItem> visibleTexts(QQuickWindow* window)
{
	QList<TextItem> texts;
	collectTexts(window->contentItem(), QRectF(0, 0, window->width(), window->height()), texts);
	return texts;
}

class Driver : public QObject
{
public:
	Driver()
	{
		m_stdin = new QSocketNotifier(StdinFd, QSocketNotifier::Read, this);
		connect(m_stdin, &QSocketNotifier::activated, this, &Driver::readStdin);
	}

private:
	void readStdin()
	{
		char chunk[ReadChunkBytes];
		const ssize_t count = ::read(StdinFd, chunk, sizeof chunk);
		if(count <= 0)
		{
			// The script ended or died: ref-gui goes with it.
			QCoreApplication::quit();
			return;
		}
		m_pending.append(chunk, static_cast<qsizetype>(count));
		for(qsizetype end; (end = m_pending.indexOf('\n')) >= 0;)
		{
			const QByteArray line = m_pending.left(end);
			m_pending.remove(0, end + 1);
			reply(run(QJsonDocument::fromJson(line).object()));
		}
	}

	void reply(const QString& json)
	{
		QByteArray bytes = json.toUtf8() + '\n';
		for(qsizetype done = 0; done < bytes.size();)
		{
			const ssize_t written = ::write(1, bytes.constData() + done, bytes.size() - done);
			if(written <= 0)
			{
				QCoreApplication::quit();
				return;
			}
			done += written;
		}
	}

	static QString ok(QJsonObject fields = {})
	{
		fields.insert("ok", true);
		return QString::fromUtf8(QJsonDocument(fields).toJson(QJsonDocument::Compact));
	}

	QString run(const QJsonObject& command)
	{
		const QString name = command.value("cmd").toString();
		if(name == "quit")
		{
			QMetaObject::invokeMethod(qApp, &QCoreApplication::quit, Qt::QueuedConnection);
			return ok();
		}
		QQuickWindow* window = mainWindow();
		if(name == "window")
		{
			return window ? ok({{"width", window->width()}, {"height", window->height()}}) : ok();
		}
		if(!window)
		{
			return errorReply(u"no window yet"_s);
		}
		if(name == "resize")
		{
			window->resize(command.value("width").toInt(), command.value("height").toInt());
			return ok();
		}
		if(name == "texts")
		{
			QJsonArray array;
			for(const TextItem& item : visibleTexts(window))
			{
				array.append(QJsonObject{{"text", item.text}, {"x", item.rect.x()}, {"y", item.rect.y()}, {"width", item.rect.width()}, {"height", item.rect.height()}});
			}
			return ok({{"texts", array}});
		}
		if(name == "grab")
		{
			const QImage image = window->grabWindow();
			return image.save(command.value("file").toString()) ? ok() : errorReply(u"cannot save the image"_s);
		}
		if(name == "click" || name == "move")
		{
			return pointer(window, name, QPoint(command.value("x").toInt(), command.value("y").toInt()));
		}
		if(name == "clickText")
		{
			const QString wanted = command.value("text").toString();
			for(const TextItem& item : visibleTexts(window))
			{
				if(item.text.contains(wanted))
				{
					return pointer(window, u"click"_s, item.rect.center().toPoint());
				}
			}
			return errorReply(u"no visible text \"%1\""_s.arg(wanted));
		}
		if(name == "key")
		{
			return key(window, command.value("sequence").toString());
		}
		if(name == "type")
		{
			for(const QChar character : command.value("text").toString())
			{
				const Qt::Key code = character.unicode() < 128 ? Qt::Key(character.toUpper().unicode()) : Qt::Key_unknown;
				QTest::sendKeyEvent(QTest::Click, window, code, QString(character), Qt::NoModifier);
			}
			return ok();
		}
		if(name == "record")
		{
			m_recordDir = command.value("dir").toString();
			QDir().mkpath(m_recordDir);
			m_recordFrames = 0;
			m_recordTimes = {};
			m_recordClock.start();
			if(!m_recordTimer)
			{
				m_recordTimer = new QTimer(this);
				m_recordTimer->setTimerType(Qt::PreciseTimer);
				connect(m_recordTimer, &QTimer::timeout, this, &Driver::recordFrame);
			}
			m_recordTimer->start(1000 / command.value("fps").toInt(DefaultRecordFps));
			return ok();
		}
		if(name == "stop")
		{
			if(m_recordTimer)
			{
				m_recordTimer->stop();
			}
			QThreadPool::globalInstance()->waitForDone();
			return ok({{"frames", m_recordFrames}, {"times", m_recordTimes}});
		}
		if(name == "wheel")
		{
			const QPoint position(command.value("x").toInt(), command.value("y").toInt());
			QWheelEvent event(position, window->mapToGlobal(position), QPoint(), QPoint(0, command.value("delta").toInt()), Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
			QGuiApplication::sendEvent(window, &event);
			return ok();
		}
		return errorReply(u"unknown command \"%1\""_s.arg(name));
	}

	static QString pointer(QQuickWindow* window, const QString& kind, const QPoint& position)
	{
		QTest::mouseMove(window, position);
		if(kind == "click")
		{
			QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, position, ClickPauseMs);
		}
		return ok();
	}

	static QString key(QQuickWindow* window, const QString& sequence)
	{
		const QKeySequence parsed = QKeySequence::fromString(sequence);
		if(parsed.isEmpty())
		{
			return errorReply(u"cannot parse key sequence \"%1\""_s.arg(sequence));
		}
		const QKeyCombination combination = parsed[0];
		QTest::keyClick(window, combination.key(), combination.keyboardModifiers());
		return ok();
	}

	// One frame of the recording: the window is read back here, the JPEG is written on a worker thread.
	void recordFrame()
	{
		QQuickWindow* window = mainWindow();
		if(!window)
		{
			return;
		}
		const QImage image = window->grabWindow();
		m_recordTimes.append(static_cast<qint64>(m_recordClock.elapsed()));
		const QString path = u"%1/f%2.jpg"_s.arg(m_recordDir).arg(m_recordFrames++, 5, 10, QChar(u'0'));
		QThreadPool::globalInstance()->start(QRunnable::create([image, path] { image.save(path, "JPG", RecordJpegQuality); }));
	}

	QSocketNotifier* m_stdin = nullptr;
	QTimer* m_recordTimer = nullptr;
	QElapsedTimer m_recordClock;
	QString m_recordDir;
	int m_recordFrames = 0;
	QJsonArray m_recordTimes;
	QByteArray m_pending;
};

}

class DriverPlugin : public QGenericPlugin
{
	Q_OBJECT
	Q_PLUGIN_METADATA(IID QGenericPluginFactoryInterface_iid FILE "refpromo.json")

public:
	QObject* create(const QString& name, const QString&) override
	{
		return name.compare(u"refpromo"_s, Qt::CaseInsensitive) == 0 ? new Driver : nullptr;
	}
};

#include "driver.moc"
