#include "icons/icon_provider.h"

#include <QDir>
#include <QGuiApplication>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::icons;

class TstIconProvider : public QObject
{
	Q_OBJECT

private slots:
	void parsesNameGridAndColor_data()
	{
		QTest::addColumn<QString>("id");
		QTest::addColumn<bool>("valid");
		QTest::addColumn<int>("grid");
		QTest::addColumn<QColor>("color");

		QTest::newRow("plain") << u"check-16"_s << true << 16 << QColor(Qt::black);
		QTest::newRow("hex without hash") << u"check-20?color=d23f45"_s << true << 20 << QColor(u"#d23f45"_s);
		QTest::newRow("encoded hash") << u"copy-16?color=%23112233"_s << true << 16 << QColor(u"#112233"_s);
		QTest::newRow("alpha") << u"copy-16?color=80112233"_s << true << 16 << QColor::fromString(u"#80112233"_s);
		QTest::newRow("bad colour keeps black") << u"copy-16?color=zzz"_s << true << 16 << QColor(Qt::black);
		QTest::newRow("no grid") << u"check"_s << false << 0 << QColor(Qt::black);
		QTest::newRow("grid not a number") << u"check-big"_s << false << 0 << QColor(Qt::black);
	}

	void parsesNameGridAndColor()
	{
		QFETCH(QString, id);
		QFETCH(bool, valid);
		QFETCH(int, grid);
		QFETCH(QColor, color);

		IconRequest request;
		QCOMPARE(parseIconRequest(id, request), valid);
		if(valid)
		{
			QCOMPARE(request.grid, grid);
			QCOMPARE(request.color, color);
		}
	}

	void replacesEveryCurrentColor()
	{
		const QByteArray svg = R"(<svg stroke="currentColor"><path stroke="currentColor"/></svg>)";
		const QByteArray result = colouredSvg(svg, QColor(u"#010203"_s));
		QVERIFY(!result.contains("currentColor"));
		QCOMPARE(result.count("#010203"), 2);
	}

	void everyIconRendersInTheRequestedColor_data()
	{
		QTest::addColumn<QString>("name");
		const QStringList files = QDir(u":/icons"_s).entryList({u"*.svg"_s});
		QVERIFY(files.size() > 50);
		for(const QString& file : files)
		{
			QTest::newRow(qPrintable(file)) << file.chopped(4);
		}
	}

	void everyIconRendersInTheRequestedColor()
	{
		QFETCH(QString, name);
		IconProvider provider;
		QSize size;
		const QImage image = provider.requestImage(name + u"?color=ff0000"_s, &size, QSize(32, 32));
		QCOMPARE(image.size(), QSize(32, 32));
		QCOMPARE(size, QSize(32, 32));

		bool inked = false;
		for(int y = 0; y < image.height(); ++y)
		{
			for(int x = 0; x < image.width(); ++x)
			{
				const QColor pixel = image.pixelColor(x, y);
				if(pixel.alpha() == 0)
				{
					continue;
				}
				inked = true;
				QCOMPARE(pixel.red(), 255);
				QCOMPARE(pixel.green(), 0);
				QCOMPARE(pixel.blue(), 0);
			}
		}
		QVERIFY(inked);
	}

	void rendersAtGridSizeWithoutARequestedSize()
	{
		IconProvider provider;
		QSize size;
		const QImage image = provider.requestImage(u"close-20"_s, &size, QSize());
		QCOMPARE(image.size(), QSize(20, 20));
	}

	void alphaScalesTheStrokes()
	{
		IconProvider provider;
		QSize size;
		const QImage opaque = provider.requestImage(u"close-20?color=000000"_s, &size, QSize(20, 20));
		const QImage faded = provider.requestImage(u"close-20?color=80000000"_s, &size, QSize(20, 20));
		int maxOpaque = 0;
		int maxFaded = 0;
		for(int y = 0; y < 20; ++y)
		{
			for(int x = 0; x < 20; ++x)
			{
				maxOpaque = std::max(maxOpaque, opaque.pixelColor(x, y).alpha());
				maxFaded = std::max(maxFaded, faded.pixelColor(x, y).alpha());
			}
		}
		QCOMPARE(maxOpaque, 255);
		QVERIFY(maxFaded > 120 && maxFaded < 136);
	}

	void unknownIconGivesAnEmptyImage()
	{
		IconProvider provider;
		QSize size;
		QVERIFY(provider.requestImage(u"nope-16"_s, &size, QSize(16, 16)).isNull());
		QVERIFY(provider.requestImage(u"nope"_s, &size, QSize(16, 16)).isNull());
	}
};

QTEST_MAIN(TstIconProvider)
#include "tst_icon_provider.moc"
