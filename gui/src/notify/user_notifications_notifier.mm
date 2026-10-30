// macOS only. Not built or run on the Linux development machine.
#include "notify/user_notifications_notifier.h"

#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>

#include <QMetaObject>

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
	m_state->delegate.onActivated = ^(NSString* session) {
		const QString id = QString::fromNSString(session);
		QMetaObject::invokeMethod(this, [this, id] { emit activated(id); }, Qt::QueuedConnection);
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
		                          this,
		                          [this, reason] {
			                          m_state->reason = reason;
			                          emit availabilityChanged();
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

	// The session as identifier: a newer notification replaces the older one.
	UNNotificationRequest* request = [UNNotificationRequest requestWithIdentifier:toNSString(notification.sessionId)
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
		                                                           this, [this, notification, reason] { emit showFailed(notification, reason); },
		                                                           Qt::QueuedConnection);
	                                                       }];
}

} // namespace refgui::notify
