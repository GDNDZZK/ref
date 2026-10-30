# System notifications: what to say, and how to show it on this platform.
set(notify_sources
	notification_center.cpp
	notification_center.h
	notification_text.cpp
	notification_text.h
	notifier.h
	platform_notifier.cpp
)

if(WIN32)
	list(APPEND notify_sources toast_notifier.cpp toast_notifier.h)
elseif(APPLE)
	enable_language(OBJCXX)
	list(APPEND notify_sources user_notifications_notifier.mm user_notifications_notifier.h)
else()
	list(APPEND notify_sources dbus_notifier.cpp dbus_notifier.h)
endif()

qt_add_library(refgui_notify STATIC ${notify_sources})

target_include_directories(refgui_notify PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_notify PUBLIC Qt6::Core Qt6::Gui refgui_proto refgui_client)
if(WIN32)
	target_link_libraries(refgui_notify PRIVATE windowsapp shell32)
elseif(APPLE)
	target_link_libraries(refgui_notify PRIVATE "-framework Foundation" "-framework UserNotifications")
else()
	target_link_libraries(refgui_notify PUBLIC Qt6::DBus)
endif()
refgui_configure_target(refgui_notify)
