# Adapted from an early version of qutie by LightBlueCube
# SPDX-License-Identifier: GPL-3.0-or-later
qt_add_executable(tst_fence_splitter tst_fence_splitter.cpp)
target_link_libraries(tst_fence_splitter PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_fence_splitter)
add_test(NAME tst_fence_splitter COMMAND tst_fence_splitter)

qt_add_executable(tst_conversation_model tst_conversation_model.cpp session_test_data.h)
# Gui is only linked so that QAbstractItemModelTester can create the QFont and QBrush values it probes with.
target_link_libraries(tst_conversation_model PRIVATE refgui_session Qt6::Gui Qt6::Test)
refgui_configure_target(tst_conversation_model)
add_test(NAME tst_conversation_model COMMAND tst_conversation_model)

qt_add_executable(tst_stream_pacer tst_stream_pacer.cpp)
target_link_libraries(tst_stream_pacer PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_stream_pacer)
add_test(NAME tst_stream_pacer COMMAND tst_stream_pacer)

qt_add_executable(tst_proto tst_proto.cpp)
target_link_libraries(tst_proto PRIVATE refgui_proto Qt6::Test)
target_compile_definitions(tst_proto PRIVATE REF_FIXTURES_DIR="${PROJECT_SOURCE_DIR}/../crates/proto/fixtures")
refgui_configure_target(tst_proto)
add_test(NAME tst_proto COMMAND tst_proto)

qt_add_executable(tst_server_client tst_server_client.cpp coro_test.h fake_backend.h)
target_link_libraries(tst_server_client PRIVATE refgui_client Qt6::Test)
refgui_configure_target(tst_server_client)
add_test(NAME tst_server_client COMMAND tst_server_client)

qt_add_executable(tst_composer_requests tst_composer_requests.cpp coro_test.h fake_backend.h)
target_link_libraries(tst_composer_requests PRIVATE refgui_client Qt6::Test)
refgui_configure_target(tst_composer_requests)
add_test(NAME tst_composer_requests COMMAND tst_composer_requests)

qt_add_executable(tst_composer_model tst_composer_model.cpp coro_test.h fake_backend.h)
target_link_libraries(tst_composer_model PRIVATE refgui_composer Qt6::Test)
refgui_configure_target(tst_composer_model)
add_test(NAME tst_composer_model COMMAND tst_composer_model)
set_tests_properties(tst_composer_model PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen") # the clipboard needs a platform

qt_add_executable(tst_launcher tst_launcher.cpp coro_test.h)
target_link_libraries(tst_launcher PRIVATE refgui_client Qt6::Test)
refgui_configure_target(tst_launcher)
add_test(NAME tst_launcher COMMAND tst_launcher)

qt_add_executable(tst_conversation_feed tst_conversation_feed.cpp session_test_data.h)
target_link_libraries(tst_conversation_feed PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_conversation_feed)
add_test(NAME tst_conversation_feed COMMAND tst_conversation_feed)

qt_add_executable(tst_session_controller tst_session_controller.cpp coro_test.h fake_backend.h session_test_data.h)
target_link_libraries(tst_session_controller PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_session_controller)
add_test(NAME tst_session_controller COMMAND tst_session_controller)

qt_add_executable(tst_command_line tst_command_line.cpp)
target_link_libraries(tst_command_line PRIVATE refgui_cli Qt6::Test)
refgui_configure_target(tst_command_line)
add_test(NAME tst_command_line COMMAND tst_command_line)

qt_add_executable(tst_icon_provider tst_icon_provider.cpp)
target_link_libraries(tst_icon_provider PRIVATE refgui_icons Qt6::Gui Qt6::Test)
refgui_configure_target(tst_icon_provider)
add_test(NAME tst_icon_provider COMMAND tst_icon_provider)

qt_add_executable(tst_settings tst_settings.cpp)
target_link_libraries(tst_settings PRIVATE refgui_workspace Qt6::Test)
refgui_configure_target(tst_settings)
add_test(NAME tst_settings COMMAND tst_settings)

qt_add_executable(tst_command_registry tst_command_registry.cpp)
target_link_libraries(tst_command_registry PRIVATE refgui_workspace Qt6::Test)
refgui_configure_target(tst_command_registry)
add_test(NAME tst_command_registry COMMAND tst_command_registry)

# Tests against the real backend. Point REF_SERVER at a built ref-server
# (`cargo build -p ref-server` gives target/debug/ref-server) to enable them.
set(REF_SERVER "" CACHE FILEPATH "ref-server binary for the tests that run against it")
if(REF_SERVER)
	qt_add_executable(tst_mock_session tst_mock_session.cpp coro_test.h session_test_data.h)
	target_link_libraries(tst_mock_session PRIVATE refgui_session Qt6::Test)
	target_compile_definitions(tst_mock_session PRIVATE REF_SERVER_BINARY="${REF_SERVER}")
	refgui_configure_target(tst_mock_session)
	add_test(NAME tst_mock_session COMMAND tst_mock_session)

	qt_add_executable(tst_mock_composer tst_mock_composer.cpp coro_test.h session_test_data.h)
	target_link_libraries(tst_mock_composer PRIVATE refgui_composer Qt6::Test)
	target_compile_definitions(tst_mock_composer PRIVATE REF_SERVER_BINARY="${REF_SERVER}")
	refgui_configure_target(tst_mock_composer)
	add_test(NAME tst_mock_composer COMMAND tst_mock_composer)

	# The whole program, offscreen. ref-gui imports the KSyntaxHighlighting QML plugin at run
	# time, which lives in the dependency prefix.
	qt_add_executable(tst_app_smoke tst_app_smoke.cpp)
	target_link_libraries(tst_app_smoke PRIVATE refgui_client Qt6::Test)
	target_compile_definitions(tst_app_smoke PRIVATE
		REF_SERVER_BINARY="${REF_SERVER}"
		REF_GUI_BINARY="$<TARGET_FILE:ref-gui>"
	)
	add_dependencies(tst_app_smoke ref-gui)
	refgui_configure_target(tst_app_smoke)
	add_test(NAME tst_app_smoke COMMAND tst_app_smoke)
	set(qml_import_paths "")
	foreach(prefix IN LISTS CMAKE_PREFIX_PATH)
		list(APPEND qml_import_paths "${prefix}/lib/qml")
	endforeach()
	# QML_IMPORT_PATH is a path list; ';' separates the test's environment entries, hence the escape.
	if(WIN32)
		list(JOIN qml_import_paths "\\;" qml_import_path)
	else()
		list(JOIN qml_import_paths ":" qml_import_path)
	endif()
	set_tests_properties(tst_app_smoke PROPERTIES ENVIRONMENT "QML_IMPORT_PATH=${qml_import_path}")
endif()

foreach(name tst_prompt_draft tst_completion tst_prompt_history)
	qt_add_executable(${name} ${name}.cpp)
	target_link_libraries(${name} PRIVATE refgui_compose Qt6::Test)
	refgui_configure_target(${name})
	add_test(NAME ${name} COMMAND ${name})
endforeach()

qt_add_executable(tst_notification_text tst_notification_text.cpp)
target_link_libraries(tst_notification_text PRIVATE refgui_notify Qt6::Test)
refgui_configure_target(tst_notification_text)
add_test(NAME tst_notification_text COMMAND tst_notification_text)

qt_add_executable(tst_notification_center tst_notification_center.cpp fake_backend.h)
target_link_libraries(tst_notification_center PRIVATE refgui_notify Qt6::Test)
refgui_configure_target(tst_notification_center)
add_test(NAME tst_notification_center COMMAND tst_notification_center)

# Needs `dbus-daemon`; skips itself without it. Never touches the user's session bus.
if(UNIX AND NOT APPLE)
	qt_add_executable(tst_dbus_notifier tst_dbus_notifier.cpp)
	target_link_libraries(tst_dbus_notifier PRIVATE refgui_notify Qt6::Test)
	refgui_configure_target(tst_dbus_notifier)
	add_test(NAME tst_dbus_notifier COMMAND tst_dbus_notifier)
endif()
qt_add_executable(tst_session_list tst_session_list.cpp fake_backend.h)
target_link_libraries(tst_session_list PRIVATE refgui_workspace Qt6::Test)
refgui_configure_target(tst_session_list)
add_test(NAME tst_session_list COMMAND tst_session_list)

# Workspace is part of the program; the test compiles its sources.
qt_add_executable(tst_workspace tst_workspace.cpp fake_backend.h
	${PROJECT_SOURCE_DIR}/src/app/workspace.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace_commands.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace_requests.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace_split.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace.h
	${PROJECT_SOURCE_DIR}/src/app/chat_session.cpp
	${PROJECT_SOURCE_DIR}/src/app/chat_session.h
	${PROJECT_SOURCE_DIR}/src/app/tile_model.cpp
	${PROJECT_SOURCE_DIR}/src/app/tile_model.h
)
target_link_libraries(tst_workspace PRIVATE refgui_workspace refgui_session refgui_composer refgui_cli Qt6::Gui Qt6::Qml Qt6::Test)
refgui_configure_target(tst_workspace)
add_test(NAME tst_workspace COMMAND tst_workspace)
