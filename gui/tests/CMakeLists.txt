# Adapted from an early version of LightBlueCube/qutie
# SPDX-License-Identifier: GPL-3.0-or-later

# One test executable `name` from `name`.cpp and SOURCES, linked to LIBS and Qt6::Test, run by ctest.
function(refgui_add_test name)
	cmake_parse_arguments(PARSE_ARGV 1 arg "" "" "SOURCES;LIBS;DEFINITIONS")
	qt_add_executable(${name} ${name}.cpp ${arg_SOURCES})
	target_link_libraries(${name} PRIVATE ${arg_LIBS} Qt6::Test)
	if(arg_DEFINITIONS)
		target_compile_definitions(${name} PRIVATE ${arg_DEFINITIONS})
	endif()
	refgui_configure_target(${name})
	add_test(NAME ${name} COMMAND ${name})
endfunction()

refgui_add_test(tst_fence_splitter LIBS refgui_session)

# Gui is only linked so that QAbstractItemModelTester can create the QFont and QBrush values it probes with.
refgui_add_test(tst_conversation_model SOURCES session_test_data.h LIBS refgui_session Qt6::Gui)

refgui_add_test(tst_stream_pacer LIBS refgui_session)

refgui_add_test(tst_proto LIBS refgui_proto DEFINITIONS REF_FIXTURES_DIR="${PROJECT_SOURCE_DIR}/../crates/proto/fixtures")

refgui_add_test(tst_server_client SOURCES coro_test.h fake_backend.h LIBS refgui_client)

refgui_add_test(tst_composer_requests SOURCES coro_test.h fake_backend.h LIBS refgui_client)

refgui_add_test(tst_composer_model SOURCES coro_test.h fake_backend.h LIBS refgui_composer)
set_tests_properties(tst_composer_model PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen") # the clipboard needs a platform

refgui_add_test(tst_plain_text DEFINITIONS REF_QML_DIR="${PROJECT_SOURCE_DIR}/src/app")

refgui_add_test(tst_backend_shutdown LIBS refgui_client)

refgui_add_test(tst_launcher SOURCES coro_test.h LIBS refgui_client)

refgui_add_test(tst_conversation_feed SOURCES session_test_data.h LIBS refgui_session)

refgui_add_test(tst_session_controller SOURCES coro_test.h fake_backend.h session_test_data.h LIBS refgui_session)

refgui_add_test(tst_question_prompt LIBS refgui_session)

refgui_add_test(tst_todo_progress LIBS refgui_proto)

refgui_add_test(tst_command_line LIBS refgui_cli)

refgui_add_test(tst_icon_provider LIBS refgui_icons Qt6::Gui)

refgui_add_test(tst_settings LIBS refgui_workspace)

refgui_add_test(tst_command_registry LIBS refgui_workspace)

refgui_add_test(tst_toast_text LIBS refgui_workspace)

# Tests against the real backend. Point REF_SERVER at a built ref-server
# (`cargo build -p ref-server` gives target/debug/ref-server) to enable them.
set(REF_SERVER "" CACHE FILEPATH "ref-server binary for the tests that run against it")
if(REF_SERVER)
	refgui_add_test(tst_mock_session SOURCES coro_test.h session_test_data.h LIBS refgui_session DEFINITIONS REF_SERVER_BINARY="${REF_SERVER}")

	refgui_add_test(tst_mock_composer SOURCES coro_test.h session_test_data.h LIBS refgui_composer DEFINITIONS REF_SERVER_BINARY="${REF_SERVER}")

	# The whole program, offscreen. ref-gui imports the KSyntaxHighlighting QML plugin at run
	# time, which lives in the dependency prefix.
	refgui_add_test(tst_app_smoke LIBS refgui_client DEFINITIONS
		REF_SERVER_BINARY="${REF_SERVER}"
		REF_GUI_BINARY="$<TARGET_FILE:ref-gui>"
	)
	add_dependencies(tst_app_smoke ref-gui)
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
	refgui_add_test(${name} LIBS refgui_compose)
endforeach()

refgui_add_test(tst_notification_text LIBS refgui_notify)

refgui_add_test(tst_notification_center SOURCES fake_backend.h LIBS refgui_notify)

# Needs `dbus-daemon`; skips itself without it. Never touches the user's session bus.
if(UNIX AND NOT APPLE)
	refgui_add_test(tst_dbus_notifier LIBS refgui_notify)
endif()

refgui_add_test(tst_session_list SOURCES fake_backend.h LIBS refgui_workspace)

# Workspace is part of the program; the test compiles its sources.
refgui_add_test(tst_workspace SOURCES fake_backend.h
	${PROJECT_SOURCE_DIR}/src/app/workspace.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace_commands.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace_requests.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace_split.cpp
	${PROJECT_SOURCE_DIR}/src/app/workspace.h
	${PROJECT_SOURCE_DIR}/src/app/chat_session.cpp
	${PROJECT_SOURCE_DIR}/src/app/chat_session.h
	${PROJECT_SOURCE_DIR}/src/app/tile_model.cpp
	${PROJECT_SOURCE_DIR}/src/app/tile_model.h
	${PROJECT_SOURCE_DIR}/src/app/tile_stage.cpp
	${PROJECT_SOURCE_DIR}/src/app/tile_stage.h
	LIBS refgui_workspace refgui_session refgui_composer refgui_cli Qt6::Gui Qt6::Qml
)

# The translator is part of the program; the test compiles it and embeds the translations like the program does.
refgui_add_test(tst_app_translator
	SOURCES ${PROJECT_SOURCE_DIR}/src/app/app_translator.cpp ${PROJECT_SOURCE_DIR}/src/app/app_translator.h
	LIBS refgui_workspace
)
qt_add_lrelease(TS_FILES ${PROJECT_SOURCE_DIR}/translations/ref-gui_zh_CN.ts ${PROJECT_SOURCE_DIR}/translations/ref-gui_en.ts
	QM_FILES_OUTPUT_VARIABLE translator_test_qm_files)
qt_add_resources(tst_app_translator translator_test_i18n PREFIX "/i18n" BASE "${CMAKE_CURRENT_BINARY_DIR}" FILES ${translator_test_qm_files})

refgui_add_test(tst_markdown_text LIBS refgui_session Qt6::Gui)
set_tests_properties(tst_markdown_text PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

refgui_add_test(tst_tool_content LIBS refgui_session)

# Text from the model must not reach the network. Compiles the program's network policy.
refgui_add_test(tst_network_policy
	SOURCES ${PROJECT_SOURCE_DIR}/src/app/local_only_network.cpp ${PROJECT_SOURCE_DIR}/src/app/local_only_network.h
	LIBS refgui_session Qt6::Network Qt6::Qml Qt6::Quick
	DEFINITIONS REF_QML_DIR="${PROJECT_SOURCE_DIR}/src/app"
)
set_tests_properties(tst_network_policy PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

# No test may reach the session bus of the user's desktop, where a notification would really show.
# A test that needs a bus starts its own, like tst_dbus_notifier.
if(UNIX AND NOT APPLE)
	get_property(refgui_tests DIRECTORY PROPERTY TESTS)
	foreach(refgui_test IN LISTS refgui_tests)
		set_property(TEST ${refgui_test} APPEND PROPERTY ENVIRONMENT_MODIFICATION
			"DBUS_SESSION_BUS_ADDRESS=set:unix:path=${CMAKE_CURRENT_BINARY_DIR}/no-session-bus")
	endforeach()
endif()
