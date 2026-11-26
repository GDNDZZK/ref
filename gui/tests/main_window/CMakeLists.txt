include(${PROJECT_SOURCE_DIR}/src/app/qml_files.cmake)
set(main_window_qml_files "")
foreach(qml_name IN LISTS REFGUI_APP_QML_NAMES)
	set(qml_path "${PROJECT_SOURCE_DIR}/src/app/${qml_name}")
	set_source_files_properties("${qml_path}" PROPERTIES QT_RESOURCE_ALIAS "${qml_name}")
	list(APPEND main_window_qml_files "${qml_path}")
endforeach()
foreach(singleton_name IN LISTS REFGUI_APP_SINGLETONS)
	set_source_files_properties("${PROJECT_SOURCE_DIR}/src/app/${singleton_name}" PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
endforeach()

refgui_add_test(tst_main_window
	SOURCES
		${CMAKE_CURRENT_SOURCE_DIR}/../fake_backend.h
		${CMAKE_CURRENT_SOURCE_DIR}/../session_test_data.h
		${PROJECT_SOURCE_DIR}/src/app/app_translator.cpp
		${PROJECT_SOURCE_DIR}/src/app/app_translator.h
		${PROJECT_SOURCE_DIR}/src/app/chat_image_provider.cpp
		${PROJECT_SOURCE_DIR}/src/app/chat_image_provider.h
		${PROJECT_SOURCE_DIR}/src/app/chat_session.cpp
		${PROJECT_SOURCE_DIR}/src/app/chat_session.h
		${PROJECT_SOURCE_DIR}/src/app/clipboard.h
		${PROJECT_SOURCE_DIR}/src/app/composer_model_type.h
		${PROJECT_SOURCE_DIR}/src/app/floating_ball_helper.cpp
		${PROJECT_SOURCE_DIR}/src/app/floating_ball_helper.h
		${PROJECT_SOURCE_DIR}/src/app/link_colorizer.cpp
		${PROJECT_SOURCE_DIR}/src/app/link_colorizer.h
		${PROJECT_SOURCE_DIR}/src/app/local_only_network.cpp
		${PROJECT_SOURCE_DIR}/src/app/local_only_network.h
		${PROJECT_SOURCE_DIR}/src/app/notification_center_type.h
		${PROJECT_SOURCE_DIR}/src/app/question_prompt_type.h
		${PROJECT_SOURCE_DIR}/src/app/qml_types.h
		${PROJECT_SOURCE_DIR}/src/app/system_fonts.h
		${PROJECT_SOURCE_DIR}/src/app/text_edit_support.h
		${PROJECT_SOURCE_DIR}/src/app/tile_model.cpp
		${PROJECT_SOURCE_DIR}/src/app/tile_model.h
		${PROJECT_SOURCE_DIR}/src/app/tile_stage.cpp
		${PROJECT_SOURCE_DIR}/src/app/tile_stage.h
		${PROJECT_SOURCE_DIR}/src/app/tool_info.h
		${PROJECT_SOURCE_DIR}/src/app/workspace.cpp
		${PROJECT_SOURCE_DIR}/src/app/workspace_commands.cpp
		${PROJECT_SOURCE_DIR}/src/app/workspace_requests.cpp
		${PROJECT_SOURCE_DIR}/src/app/workspace_split.cpp
		${PROJECT_SOURCE_DIR}/src/app/workspace.h
	LIBS refgui_workspace refgui_session refgui_composer refgui_notify refgui_cli refgui_icons Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2
)
if(WIN32)
	target_link_libraries(tst_main_window PRIVATE user32)
endif()
qt_add_qml_module(tst_main_window
	URI RefGui
	QML_FILES ${main_window_qml_files}
)
target_include_directories(tst_main_window PRIVATE ${PROJECT_SOURCE_DIR}/src/app ${CMAKE_CURRENT_SOURCE_DIR}/..)
set_tests_properties(tst_main_window PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen;QML_IMPORT_PATH=${qml_import_path}")
if(UNIX AND NOT APPLE)
	set_property(TEST tst_main_window APPEND PROPERTY ENVIRONMENT_MODIFICATION
		"DBUS_SESSION_BUS_ADDRESS=set:unix:path=${CMAKE_BINARY_DIR}/tests/no-session-bus")
endif()
