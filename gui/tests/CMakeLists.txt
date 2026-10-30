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

# Tests against the real backend. Point REF_SERVER at a built ref-server
# (`cargo build -p ref-server` gives target/debug/ref-server) to enable them.
set(REF_SERVER "" CACHE FILEPATH "ref-server binary for the tests that run against it")
if(REF_SERVER)
	qt_add_executable(tst_mock_session tst_mock_session.cpp coro_test.h session_test_data.h)
	target_link_libraries(tst_mock_session PRIVATE refgui_session Qt6::Test)
	target_compile_definitions(tst_mock_session PRIVATE REF_SERVER_BINARY="${REF_SERVER}")
	refgui_configure_target(tst_mock_session)
	add_test(NAME tst_mock_session COMMAND tst_mock_session)

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
