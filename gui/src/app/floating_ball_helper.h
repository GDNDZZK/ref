#pragma once

#include <QList>
#include <QObject>
#include <QPoint>
#include <QRect>
#include <QSize>
#include <QString>
#include <QVariantMap>

class QScreen;
class QWindow;

// Computes screen-clamped positions and inward expansion directions for the floating ball,
// and watches screen hotplug / availableGeometry changes so QML can re-clamp immediately.
class FloatingBallHelper : public QObject
{
	Q_OBJECT

public:
	struct ScreenGeometry
	{
		QString name;
		QRect availableGeometry;
	};

	struct PanelPlacement
	{
		QRect rect;
		QString screenName;
		bool expandLeft = false;
		bool expandUp = false;
	};

	explicit FloatingBallHelper(QObject* parent = nullptr);

	static QPoint clampBallPosition(QPoint pos, QSize ballSize, const QRect& available);
	static QPoint resolveBallPosition(bool hasSaved,
	                                  const QString& savedScreen,
	                                  QPoint savedPos,
	                                  QSize ballSize,
	                                  const QList<ScreenGeometry>& screens,
	                                  QString* resolvedScreen = nullptr);
	static PanelPlacement computePanelPlacement(QRect ballRect,
	                                            QSize panelSize,
	                                            const QList<ScreenGeometry>& screens,
	                                            const QString& preferredScreen = {});
	static QRect clampWindowRect(QRect rect, const QList<ScreenGeometry>& screens, const QString& preferredScreen = {});
	static QString pickScreenName(QRect rect, const QList<ScreenGeometry>& screens, const QString& preferredScreen = {});

	Q_INVOKABLE QVariantMap resolveBallPosition(bool hasSaved,
	                                            const QString& savedScreen,
	                                            int savedX,
	                                            int savedY,
	                                            int ballWidth,
	                                            int ballHeight) const;
	Q_INVOKABLE QVariantMap computePanelGeometry(int ballX,
	                                             int ballY,
	                                             int ballWidth,
	                                             int ballHeight,
	                                             int panelWidth,
	                                             int panelHeight,
	                                             const QString& screenName = {}) const;
	Q_INVOKABLE QVariantMap clampWindowRect(int x, int y, int width, int height, const QString& screenName = {}) const;
	Q_INVOKABLE QPoint cursorPos() const;
	Q_INVOKABLE void ensureAlwaysOnTop(QWindow* window) const;

signals:
	void screenGeometryChanged();

private:
	void watchScreen(QScreen* screen);
	static QList<ScreenGeometry> currentScreens();
};
