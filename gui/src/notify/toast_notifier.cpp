// Windows only. Not built or run on the Linux development machine.
#include "notify/toast_notifier.h"

// C++/WinRT first: it must see the Windows headers before Qt's.
#include <winrt/Windows.Data.Xml.Dom.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.UI.Notifications.h>

#include <shobjidl.h>

#include <QMetaObject>
#include <QSettings>

#include <map>

using namespace Qt::StringLiterals;
namespace toast = winrt::Windows::UI::Notifications;

namespace refgui::notify
{
namespace
{

constexpr const wchar_t* AppUserModelId = L"ref.gui";
const QString AppUserModelIdKey = u"HKEY_CURRENT_USER\\Software\\Classes\\AppUserModelId\\ref.gui"_s;
const QString DisplayName = u"ref"_s;
constexpr qsizetype MaxTagLength = 64; // limit of ToastNotification::Tag

// Toasts are only allowed for a program with an AppUserModelID known to the shell.
QString registerAppUserModelId()
{
	QSettings registry(AppUserModelIdKey, QSettings::NativeFormat);
	registry.setValue(u"DisplayName"_s, DisplayName);
	registry.sync();
	if(registry.status() != QSettings::NoError)
	{
		return ToastNotifier::tr("Cannot register ref with Windows notifications.");
	}
	if(FAILED(SetCurrentProcessExplicitAppUserModelID(AppUserModelId)))
	{
		return ToastNotifier::tr("Cannot name ref to Windows notifications.");
	}
	return {};
}

QString reasonOf(toast::NotificationSetting setting)
{
	switch(setting)
	{
	case toast::NotificationSetting::Enabled:
		return {};
	case toast::NotificationSetting::DisabledForApplication:
	case toast::NotificationSetting::DisabledForUser:
		return ToastNotifier::tr("Notifications for ref are turned off in the Windows settings.");
	case toast::NotificationSetting::DisabledByGroupPolicy:
		return ToastNotifier::tr("Notifications are turned off by group policy.");
	case toast::NotificationSetting::DisabledByManifest:
		return ToastNotifier::tr("Notifications are turned off for ref by its manifest.");
	}
	return ToastNotifier::tr("Windows does not allow notifications for ref.");
}

} // namespace

struct ToastNotifier::State
{
	struct Shown
	{
		toast::ToastNotification toast{nullptr};
		winrt::event_token activated{};
		winrt::event_token failed{};
	};

	QString reason;
	toast::ToastNotifier notifier{nullptr};
	std::map<QString, Shown> shown; // by tag: a newer toast of the same session and kind replaces the older one

	void revoke(Shown& entry)
	{
		entry.toast.Activated(entry.activated);
		entry.toast.Failed(entry.failed);
	}
};

ToastNotifier::ToastNotifier(QObject* parent)
	: Notifier(parent)
	, m_state(std::make_unique<State>())
{
	m_state->reason = registerAppUserModelId();
	if(!m_state->reason.isEmpty())
	{
		return;
	}
	try
	{
		m_state->notifier = toast::ToastNotificationManager::CreateToastNotifier(AppUserModelId);
		m_state->reason = reasonOf(m_state->notifier.Setting());
	}
	catch(const winrt::hresult_error& error)
	{
		m_state->reason = tr("Windows notifications failed: %1").arg(QString::fromStdWString(error.message().c_str()));
	}
}

ToastNotifier::~ToastNotifier()
{
	for(auto& [session, entry] : m_state->shown)
	{
		m_state->revoke(entry);
	}
}

bool ToastNotifier::isAvailable() const
{
	return m_state->reason.isEmpty();
}

QString ToastNotifier::unavailableReason() const
{
	return m_state->reason;
}

void ToastNotifier::show(const Notification& notification)
{
	if(!isAvailable())
	{
		return;
	}

	try
	{
		// ToastText02: a bold headline and one line of text.
		const auto xml = toast::ToastNotificationManager::GetTemplateContent(toast::ToastTemplateType::ToastText02);
		const auto texts = xml.GetElementsByTagName(L"text");
		texts.Item(0).AppendChild(xml.CreateTextNode(winrt::hstring(notification.title.toStdWString())));
		texts.Item(1).AppendChild(xml.CreateTextNode(winrt::hstring(notification.body.toStdWString())));

		toast::ToastNotification message{xml};
		// The kind comes first so that it survives the cut, like the tag of a Web notification.
		const QString tag = u"%1:%2"_s.arg(static_cast<int>(notification.kind)).arg(notification.sessionId).left(MaxTagLength);
		message.Tag(winrt::hstring(tag.toStdWString()));

		// These run on a thread of the Windows runtime; hand over to the main thread.
		const QString session = notification.sessionId;
		State::Shown entry;
		entry.toast = message;
		entry.activated = message.Activated([this, session](const auto&, const auto&) {
			QMetaObject::invokeMethod(this, [this, session] { emit activated(session); }, Qt::QueuedConnection);
		});
		entry.failed = message.Failed([this, notification](const auto&, const toast::ToastFailedEventArgs& args) {
			const QString reason = tr("Windows could not show the notification (error %1).").arg(static_cast<qint32>(args.ErrorCode().value));
			QMetaObject::invokeMethod(this, [this, notification, reason] { emit showFailed(notification, reason); }, Qt::QueuedConnection);
		});

		if(const auto previous = m_state->shown.find(tag); previous != m_state->shown.end())
		{
			m_state->revoke(previous->second);
			m_state->shown.erase(previous);
		}
		m_state->notifier.Show(message);
		m_state->shown.emplace(tag, std::move(entry));
	}
	catch(const winrt::hresult_error& error)
	{
		emit showFailed(notification, QString::fromStdWString(error.message().c_str()));
	}
}

} // namespace refgui::notify
