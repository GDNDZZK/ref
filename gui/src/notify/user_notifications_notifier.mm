// macOS only. Not built or run on the Linux development machine.
#include "notify/user_notifications_notifier.h"

#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>

#include <QCoreApplication>
#include <QMetaObject>
#include <QPointer>

using namespace Qt::StringLiterals;

namespace
{

NSString* const SessionKey = @"session";

NSString* toNSString(const QString& text)
{
	return text.toNSString();
}

} // namespace

// Receives the user's click. UserNotifications calls the delegate on its own queue.
@interface RefNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@property(nonatomic, assign) refgui::notify::UserNotificationsNotifier* owner;
@property(nonatomic, copy) void (^onActivated)(NSString* session);
@end

@implementation RefNotificationDelegate

- (void)userNotificationCenter:(UNUserNotificationCenter*)center
       willPresentNotification:(UNNotification*)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completionHandler
{
	// The backend only notifies when the user is not looking, so show it even if ref is active.
	completionHandler(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList);
}

- (void)userNotificationCenter:(UNUserNotificationCenter*)center
    didReceiveNotificationResponse:(UNNotificationResponse*)response
             withCompletionHandler:(void (^)(void))completionHandler
{
	NSString* session = response.notification.request.content.userInfo[SessionKey];
	if(session != nil && self.onActivated != nil)
	{
		self.onActivated(session);
	}
	completionHandler();
}

@end

namespace refgui::notify
{

struct UserNotificationsNotifier::State
{
	QString reason;
	RefNotificationDelegate* delegate = nil;
};

UserNotificationsNotifier::UserNotificationsNotifier(QObject* parent)
	: Notifier(parent)
	, m_state(std::make_unique<State>())
{
	// UNUserNotificationCenter raises an exception outside an app bundle.
	if([[NSBundle mainBundle] bundleIdentifier] == nil)
	{
		m_state->reason = tr("Notifications need ref to run as ref.app.");
		return;
	}

	m_state->reason = tr("Asking macOS for permission to show notifications…");
	UNUserNotificationCenter* center = [UNUserNotificationCenter currentNotificationCenter];

	m_state->delegate = [[RefNotificationDelegate alloc] init];
	// The callbacks of macOS come on other threads and can come after this object is gone. A QPointer is only
	// safe to look at on the thread of its object, so each callback hops to the main thread first.
	const QPointer<UserNotificationsNotifier> self(this);
	m_state->delegate.onActivated = ^(NSString* session) {
		const QString id = QString::fromNSString(session);
		QMetaObject::invokeMethod(
		    qApp,
		    [self, id] {
			    if(self)
			    {
				    emit self->activated(id);
			    }
		    },
		    Qt::QueuedConnection);
	};
	center.delegate = m_state->delegate;

	[center requestAuthorizationWithOptions:(UNAuthorizationOptionAlert | UNAuthorizationOptionSound)
	                      completionHandler:^(BOOL granted, NSError* error) {
		                      QString reason;
		                      if(error != nil)
		                      {
			                      reason = tr("macOS refused notifications for ref: %1").arg(QString::fromNSString(error.localizedDescription));
		                      }
		                      else if(!granted)
		                      {
			                      reason = tr("Notifications for ref are turned off in the macOS System Settings.");
		                      }
		                      QMetaObject::invokeMethod(
		                          qApp,
		                          [self, reason] {
			                          if(!self)
			                          {
				                          return;
			                          }
			                          self->m_state->reason = reason;
			                          emit self->availabilityChanged();
		                          },
		                          Qt::QueuedConnection);
	                      }];
}

UserNotificationsNotifier::~UserNotificationsNotifier()
{
	if(m_state->delegate != nil)
	{
		m_state->delegate.onActivated = nil; // the center may outlive us and still call it
		[UNUserNotificationCenter currentNotificationCenter].delegate = nil;
	}
}

bool UserNotificationsNotifier::isAvailable() const
{
	return m_state->reason.isEmpty();
}

QString UserNotificationsNotifier::unavailableReason() const
{
	return m_state->reason;
}

void UserNotificationsNotifier::show(const Notification& notification)
{
	if(!isAvailable())
	{
		return;
	}

	UNMutableNotificationContent* content = [[UNMutableNotificationContent alloc] init];
	content.title = toNSString(notification.title);
	content.body = toNSString(notification.body);
	content.userInfo = @{SessionKey : toNSString(notification.sessionId)};
	content.threadIdentifier = toNSString(notification.sessionId);

	// Session and kind as identifier: a newer notification of both replaces the older one, as in the Web interface.
	const QString identifier = u"%1:%2"_s.arg(notification.sessionId).arg(static_cast<int>(notification.kind));
	const QPointer<UserNotificationsNotifier> self(this);
	UNNotificationRequest* request = [UNNotificationRequest requestWithIdentifier:toNSString(identifier)
	                                                                      content:content
	                                                                      trigger:nil];
	[[UNUserNotificationCenter currentNotificationCenter] addNotificationRequest:request
	                                                       withCompletionHandler:^(NSError* error) {
		                                                       if(error == nil)
		                                                       {
			                                                       return;
		                                                       }
		                                                       const QString reason = QString::fromNSString(error.localizedDescription);
		                                                       QMetaObject::invokeMethod(
		                                                           qApp,
		                                                           [self, notification, reason] {
			                                                           if(self)
			                                                           {
				                                                           emit self->showFailed(notification, reason);
			                                                           }
		                                                           },
		                                                           Qt::QueuedConnection);
	                                                       }];
}

} // namespace refgui::notify
