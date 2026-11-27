#include "app/floating_ball_helper.h"

#include <QCursor>
#include <QGuiApplication>
#include <QScreen>
#include <QWindow>

#ifdef Q_OS_WIN
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#elif QT_CONFIG(xcb)
#include <QLibrary>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#endif
#include <algorithm>

using namespace Qt::StringLiterals;

namespace
{

constexpr int DefaultEdgeMargin = 24;

qint64 intersectionArea(const QRect& a, const QRect& b)
{
	const QRect clipped = a.intersected(b);
	if(clipped.isEmpty())
	{
		return 0;
	}
	return static_cast<qint64>(clipped.width()) * static_cast<qint64>(clipped.height());
}

const FloatingBallHelper::ScreenGeometry* findScreenForRect(const QRect& rect,
                                                            const QList<FloatingBallHelper::ScreenGeometry>& screens,
                                                            const QString& preferredScreen)
{
	if(screens.isEmpty())
	{
		return nullptr;
	}
	if(!preferredScreen.isEmpty())
	{
		for(const auto& screen : screens)
		{
			if(screen.name == preferredScreen)
			{
				return &screen;
			}
		}
	}
	const QPoint center = rect.center();
	for(const auto& screen : screens)
	{
		if(screen.availableGeometry.contains(center))
		{
			return &screen;
		}
	}
	const FloatingBallHelper::ScreenGeometry* best = &screens.first();
	qint64 bestArea = intersectionArea(rect, best->availableGeometry);
	for(qsizetype i = 1; i < screens.size(); ++i)
	{
		const qint64 area = intersectionArea(rect, screens.at(i).availableGeometry);
		if(area > bestArea)
		{
			best = &screens.at(i);
			bestArea = area;
		}
	}
	return best;
}

#if !defined(Q_OS_WIN) && QT_CONFIG(xcb)

constexpr std::uint8_t XcbClientMessage = 33;
constexpr std::uint8_t XcbFormat32 = 32;
constexpr std::uint32_t XcbEventMaskSubstructureNotify = 0x00080000U;
constexpr std::uint32_t XcbEventMaskSubstructureRedirect = 0x00100000U;
constexpr std::uint32_t NetWmStateAdd = 1U;
constexpr std::uint32_t NetWmSourceApplication = 1U;

struct XcbCookie
{
	std::uint32_t sequence;
};

struct XcbInternAtomReply
{
	std::uint8_t responseType;
	std::uint8_t pad0;
	std::uint16_t sequence;
	std::uint32_t length;
	std::uint32_t atom;
};

struct XcbGetGeometryReply
{
	std::uint8_t responseType;
	std::uint8_t depth;
	std::uint16_t sequence;
	std::uint32_t length;
	std::uint32_t root;
	std::int16_t x;
	std::int16_t y;
	std::uint16_t width;
	std::uint16_t height;
	std::uint16_t borderWidth;
	std::uint8_t pad0[2];
};

struct XcbClientMessageEvent
{
	std::uint8_t responseType;
	std::uint8_t format;
	std::uint16_t sequence;
	std::uint32_t window;
	std::uint32_t type;
	std::uint32_t data32[5];
};
static_assert(sizeof(XcbClientMessageEvent) == 32);

struct XcbApi
{
	using InternAtomFn = XcbCookie (*)(xcb_connection_t*, std::uint8_t, std::uint16_t, const char*);
	using InternAtomReplyFn = XcbInternAtomReply* (*)(xcb_connection_t*, XcbCookie, void**);
	using GetGeometryFn = XcbCookie (*)(xcb_connection_t*, std::uint32_t);
	using GetGeometryReplyFn = XcbGetGeometryReply* (*)(xcb_connection_t*, XcbCookie, void**);
	using SendEventFn = XcbCookie (*)(xcb_connection_t*, std::uint8_t, std::uint32_t, std::uint32_t, const char*);
	using FlushFn = int (*)(xcb_connection_t*);

	InternAtomFn internAtom = nullptr;
	InternAtomReplyFn internAtomReply = nullptr;
	GetGeometryFn getGeometry = nullptr;
	GetGeometryReplyFn getGeometryReply = nullptr;
	SendEventFn sendEvent = nullptr;
	FlushFn flush = nullptr;

	bool valid() const
	{
		return internAtom != nullptr && internAtomReply != nullptr && getGeometry != nullptr && getGeometryReply != nullptr && sendEvent != nullptr && flush != nullptr;
	}
};

template<typename Fn>
bool resolveXcbSymbol(QLibrary& lib, const char* symbol, Fn& out)
{
	const QFunctionPointer raw = lib.resolve(symbol);
	if(raw == nullptr)
	{
		return false;
	}
	static_assert(sizeof(Fn) == sizeof(QFunctionPointer));
	std::memcpy(&out, &raw, sizeof(out));
	return true;
}

const XcbApi& xcbApi()
{
	static const XcbApi api = [] {
		static QLibrary lib(u"xcb"_s, 1);
		XcbApi loaded;
		if(!lib.load())
		{
			return loaded;
		}
		const bool ok = resolveXcbSymbol(lib, "xcb_intern_atom", loaded.internAtom)
		                && resolveXcbSymbol(lib, "xcb_intern_atom_reply", loaded.internAtomReply)
		                && resolveXcbSymbol(lib, "xcb_get_geometry", loaded.getGeometry)
		                && resolveXcbSymbol(lib, "xcb_get_geometry_reply", loaded.getGeometryReply)
		                && resolveXcbSymbol(lib, "xcb_send_event", loaded.sendEvent)
		                && resolveXcbSymbol(lib, "xcb_flush", loaded.flush);
		return ok ? loaded : XcbApi{};
	}();
	return api;
}

struct XcbAtoms
{
	xcb_connection_t* conn = nullptr;
	std::uint32_t netWmState = 0;
	std::uint32_t netWmStateAbove = 0;
};

XcbAtoms resolveXcbAtoms(const XcbApi& api, xcb_connection_t* conn)
{
	static XcbAtoms cached;
	if(cached.conn == conn && cached.netWmState != 0 && cached.netWmStateAbove != 0)
	{
		return cached;
	}
	constexpr const char* stateName = "_NET_WM_STATE";
	constexpr const char* aboveName = "_NET_WM_STATE_ABOVE";
	const XcbCookie stateCookie = api.internAtom(conn, 0, static_cast<std::uint16_t>(std::strlen(stateName)), stateName);
	const XcbCookie aboveCookie = api.internAtom(conn, 0, static_cast<std::uint16_t>(std::strlen(aboveName)), aboveName);
	XcbInternAtomReply* stateReply = api.internAtomReply(conn, stateCookie, nullptr);
	XcbInternAtomReply* aboveReply = api.internAtomReply(conn, aboveCookie, nullptr);
	XcbAtoms result{.conn = conn, .netWmState = 0, .netWmStateAbove = 0};
	if(stateReply != nullptr)
	{
		result.netWmState = stateReply->atom;
		std::free(stateReply);
	}
	if(aboveReply != nullptr)
	{
		result.netWmStateAbove = aboveReply->atom;
		std::free(aboveReply);
	}
	if(result.netWmState != 0 && result.netWmStateAbove != 0)
	{
		cached = result;
	}
	return result;
}

void sendXcbAlwaysOnTop(QWindow* window)
{
	if(qGuiApp == nullptr)
	{
		return;
	}
	auto* x11 = qGuiApp->nativeInterface<QNativeInterface::QX11Application>();
	if(x11 == nullptr)
	{
		return;
	}
	xcb_connection_t* conn = x11->connection();
	if(conn == nullptr)
	{
		return;
	}
	const XcbApi& api = xcbApi();
	if(!api.valid())
	{
		return;
	}
	const auto xid = static_cast<std::uint32_t>(window->winId());
	if(xid == 0)
	{
		return;
	}
	const XcbCookie geomCookie = api.getGeometry(conn, xid);
	XcbGetGeometryReply* geom = api.getGeometryReply(conn, geomCookie, nullptr);
	if(geom == nullptr)
	{
		return;
	}
	const std::uint32_t root = geom->root;
	std::free(geom);
	if(root == 0)
	{
		return;
	}
	const XcbAtoms atoms = resolveXcbAtoms(api, conn);
	if(atoms.netWmState == 0 || atoms.netWmStateAbove == 0)
	{
		return;
	}
	XcbClientMessageEvent ev{};
	ev.responseType = XcbClientMessage;
	ev.format = XcbFormat32;
	ev.window = xid;
	ev.type = atoms.netWmState;
	ev.data32[0] = NetWmStateAdd;
	ev.data32[1] = atoms.netWmStateAbove;
	ev.data32[3] = NetWmSourceApplication;
	api.sendEvent(conn, 0, root, XcbEventMaskSubstructureNotify | XcbEventMaskSubstructureRedirect, reinterpret_cast<const char*>(&ev));
	api.flush(conn);
}

#endif

} // namespace

FloatingBallHelper::FloatingBallHelper(QObject* parent)
	: QObject(parent)
{
	if(qGuiApp != nullptr)
	{
		for(QScreen* screen : QGuiApplication::screens())
		{
			watchScreen(screen);
		}
		connect(qGuiApp, &QGuiApplication::screenAdded, this, [this](QScreen* screen) {
			watchScreen(screen);
			emit screenGeometryChanged();
		});
		connect(qGuiApp, &QGuiApplication::screenRemoved, this, [this](QScreen*) {
			emit screenGeometryChanged();
		});
		connect(qGuiApp, &QGuiApplication::primaryScreenChanged, this, [this](QScreen*) {
			emit screenGeometryChanged();
		});
	}
}

void FloatingBallHelper::watchScreen(QScreen* screen)
{
	if(screen == nullptr)
	{
		return;
	}
	connect(screen, &QScreen::availableGeometryChanged, this, &FloatingBallHelper::screenGeometryChanged, Qt::UniqueConnection);
	connect(screen, &QScreen::geometryChanged, this, &FloatingBallHelper::screenGeometryChanged, Qt::UniqueConnection);
}

QList<FloatingBallHelper::ScreenGeometry> FloatingBallHelper::currentScreens()
{
	QList<ScreenGeometry> result;
	if(qGuiApp == nullptr)
	{
		return result;
	}
	const QScreen* primary = QGuiApplication::primaryScreen();
	const QList<QScreen*> screens = QGuiApplication::screens();
	result.reserve(screens.size());
	if(primary != nullptr)
	{
		result.append({.name = primary->name(), .availableGeometry = primary->availableGeometry()});
	}
	for(const QScreen* screen : screens)
	{
		if(screen != nullptr && screen != primary)
		{
			result.append({.name = screen->name(), .availableGeometry = screen->availableGeometry()});
		}
	}
	return result;
}

QPoint FloatingBallHelper::clampBallPosition(QPoint pos, QSize ballSize, const QRect& available)
{
	if(!available.isValid() || available.width() <= 0 || available.height() <= 0)
	{
		return pos;
	}
	const int w = std::max(1, ballSize.width());
	const int h = std::max(1, ballSize.height());
	const int minX = available.x();
	const int maxX = available.x() + std::max(0, available.width() - w);
	const int minY = available.y();
	const int maxY = available.y() + std::max(0, available.height() - h);
	return {std::clamp(pos.x(), minX, maxX), std::clamp(pos.y(), minY, maxY)};
}

QPoint FloatingBallHelper::resolveBallPosition(bool hasSaved,
                                                 const QString& savedScreen,
                                                 QPoint savedPos,
                                                 QSize ballSize,
                                                 const QList<ScreenGeometry>& screens,
                                                 QString* resolvedScreen)
{
	if(screens.isEmpty())
	{
		if(resolvedScreen != nullptr)
		{
			*resolvedScreen = savedScreen;
		}
		return hasSaved ? savedPos : QPoint(DefaultEdgeMargin, DefaultEdgeMargin);
	}

	const ScreenGeometry* target = nullptr;
	if(hasSaved)
	{
		target = findScreenForRect(QRect(savedPos, ballSize), screens, savedScreen);
	}
	if(target == nullptr)
	{
		target = &screens.first();
	}
	if(resolvedScreen != nullptr)
	{
		*resolvedScreen = target->name;
	}

	const QRect avail = target->availableGeometry;
	if(!hasSaved)
	{
		const QPoint defaultPos(avail.x() + avail.width() - ballSize.width() - DefaultEdgeMargin,
		                        avail.y() + avail.height() - ballSize.height() - DefaultEdgeMargin);
		return clampBallPosition(defaultPos, ballSize, avail);
	}
	return clampBallPosition(savedPos, ballSize, avail);
}

FloatingBallHelper::PanelPlacement FloatingBallHelper::computePanelPlacement(QRect ballRect,
                                                                             QSize panelSize,
                                                                             const QList<ScreenGeometry>& screens,
                                                                             const QString& preferredScreen)
{
	const ScreenGeometry* target = findScreenForRect(ballRect, screens, preferredScreen);
	if(target == nullptr || !target->availableGeometry.isValid())
	{
		return {.rect = QRect(ballRect.topLeft(), panelSize), .screenName = preferredScreen, .expandLeft = false, .expandUp = false};
	}

	const QRect avail = target->availableGeometry;
	const int panelW = std::clamp(panelSize.width(), 1, std::max(1, avail.width()));
	const int panelH = std::clamp(panelSize.height(), 1, std::max(1, avail.height()));
	const int spaceRight = (avail.x() + avail.width()) - ballRect.x();
	const int spaceLeft = (ballRect.x() + ballRect.width()) - avail.x();
	const int spaceBelow = (avail.y() + avail.height()) - ballRect.y();
	const int spaceAbove = (ballRect.y() + ballRect.height()) - avail.y();

	const bool expandLeft = ballRect.center().x() > avail.x() + avail.width() / 2 || (spaceRight < panelW && spaceLeft >= spaceRight);
	const bool expandUp = ballRect.center().y() > avail.y() + avail.height() / 2 || (spaceBelow < panelH && spaceAbove >= spaceBelow);

	const int rawX = expandLeft ? (ballRect.x() + ballRect.width() - panelW) : ballRect.x();
	const int rawY = expandUp ? (ballRect.y() + ballRect.height() - panelH) : ballRect.y();
	const int minX = avail.x();
	const int maxX = avail.x() + std::max(0, avail.width() - panelW);
	const int minY = avail.y();
	const int maxY = avail.y() + std::max(0, avail.height() - panelH);

	return {
		.rect = QRect(std::clamp(rawX, minX, maxX), std::clamp(rawY, minY, maxY), panelW, panelH),
		.screenName = target->name,
		.expandLeft = expandLeft,
		.expandUp = expandUp,
	};
}

QRect FloatingBallHelper::clampWindowRect(QRect rect, const QList<ScreenGeometry>& screens, const QString& preferredScreen)
{
	const ScreenGeometry* target = findScreenForRect(rect, screens, preferredScreen);
	if(target == nullptr || !target->availableGeometry.isValid())
	{
		return rect;
	}
	const QRect avail = target->availableGeometry;
	const int w = std::clamp(rect.width(), 1, std::max(1, avail.width()));
	const int h = std::clamp(rect.height(), 1, std::max(1, avail.height()));
	const int x = std::clamp(rect.x(), avail.x(), avail.x() + std::max(0, avail.width() - w));
	const int y = std::clamp(rect.y(), avail.y(), avail.y() + std::max(0, avail.height() - h));
	return {x, y, w, h};
}

QString FloatingBallHelper::pickScreenName(QRect rect, const QList<ScreenGeometry>& screens, const QString& preferredScreen)
{
	const ScreenGeometry* target = findScreenForRect(rect, screens, preferredScreen);
	return target != nullptr ? target->name : preferredScreen;
}

QVariantMap FloatingBallHelper::resolveBallPosition(bool hasSaved,
                                                    const QString& savedScreen,
                                                    int savedX,
                                                    int savedY,
                                                    int ballWidth,
                                                    int ballHeight) const
{
	const QList<ScreenGeometry> screens = currentScreens();
	QString resolvedScreen;
	const QPoint pos = resolveBallPosition(hasSaved, savedScreen, QPoint(savedX, savedY), QSize(ballWidth, ballHeight), screens, &resolvedScreen);
	const ScreenGeometry* target = findScreenForRect(QRect(pos, QSize(ballWidth, ballHeight)), screens, resolvedScreen);
	const QRect avail = target != nullptr ? target->availableGeometry : QRect();
	return {
		{u"x"_s, pos.x()},
		{u"y"_s, pos.y()},
		{u"screenName"_s, resolvedScreen},
		{u"availableX"_s, avail.x()},
		{u"availableY"_s, avail.y()},
		{u"availableWidth"_s, avail.width()},
		{u"availableHeight"_s, avail.height()},
	};
}

QVariantMap FloatingBallHelper::computePanelGeometry(int ballX,
                                                     int ballY,
                                                     int ballWidth,
                                                     int ballHeight,
                                                     int panelWidth,
                                                     int panelHeight,
                                                     const QString& screenName) const
{
	const QList<ScreenGeometry> screens = currentScreens();
	const PanelPlacement placed = computePanelPlacement(QRect(ballX, ballY, ballWidth, ballHeight),
	                                                    QSize(panelWidth, panelHeight),
	                                                    screens,
	                                                    screenName);
	return {
		{u"x"_s, placed.rect.x()},
		{u"y"_s, placed.rect.y()},
		{u"width"_s, placed.rect.width()},
		{u"height"_s, placed.rect.height()},
		{u"screenName"_s, placed.screenName},
		{u"expandLeft"_s, placed.expandLeft},
		{u"expandUp"_s, placed.expandUp},
	};
}

QVariantMap FloatingBallHelper::clampWindowRect(int x, int y, int width, int height, const QString& screenName) const
{
	const QList<ScreenGeometry> screens = currentScreens();
	const QRect rect(x, y, width, height);
	const QRect clamped = clampWindowRect(rect, screens, screenName);
	return {
		{u"x"_s, clamped.x()},
		{u"y"_s, clamped.y()},
		{u"width"_s, clamped.width()},
		{u"height"_s, clamped.height()},
		{u"screenName"_s, pickScreenName(clamped, screens, screenName)},
	};
}

QPoint FloatingBallHelper::cursorPos() const
{
	return QCursor::pos();
}

void FloatingBallHelper::ensureAlwaysOnTop(QWindow* window) const
{
	if(window == nullptr)
	{
		return;
	}
#ifdef Q_OS_WIN
	if(QGuiApplication::platformName() == u"windows"_s)
	{
		if(window->isVisible())
		{
			const auto hwnd = reinterpret_cast<HWND>(window->winId());
			if(hwnd != nullptr)
			{
				UINT flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
				constexpr LONG_PTR snapMask = static_cast<LONG_PTR>(WS_MAXIMIZEBOX | WS_THICKFRAME);
				const LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
				if((style & snapMask) != 0)
				{
					SetWindowLongPtrW(hwnd, GWL_STYLE, style & ~snapMask);
					flags |= SWP_FRAMECHANGED;
				}
				SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
			}
		}
		return;
	}
#elif QT_CONFIG(xcb)
	if(QGuiApplication::platformName() == u"xcb"_s)
	{
		if(!window->flags().testFlag(Qt::WindowStaysOnTopHint))
		{
			window->setFlag(Qt::WindowStaysOnTopHint, true);
		}
		if(window->isVisible())
		{
			sendXcbAlwaysOnTop(window);
		}
		return;
	}
#endif
	if(!window->flags().testFlag(Qt::WindowStaysOnTopHint))
	{
		window->setFlag(Qt::WindowStaysOnTopHint, true);
	}
}
