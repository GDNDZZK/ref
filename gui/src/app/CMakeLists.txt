# Adapted from an early version of LightBlueCube/qutie
qt_add_executable(ref-gui main.cpp)

include(qml_files.cmake)

# Must be set before qt_add_qml_module() reads the files.
set_source_files_properties(${REFGUI_APP_SINGLETONS} PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(ref-gui
	URI RefGui
	SOURCES
		app_translator.cpp
		app_translator.h
		local_only_network.cpp
		local_only_network.h
		chat_image_provider.cpp
		chat_image_provider.h
		chat_session.cpp
		chat_session.h
		clipboard.h
		composer_model_type.h
		link_colorizer.cpp
		link_colorizer.h
		notification_center_type.h
		question_prompt_type.h
		qml_types.h
		system_fonts.h
		tool_info.h
		tile_model.cpp
		tile_model.h
		tile_stage.cpp
		tile_stage.h
		workspace.cpp
		workspace_commands.cpp
		workspace_requests.cpp
		workspace_split.cpp
		workspace.h
	QML_FILES
		${REFGUI_APP_QML_NAMES}
	)
# Interface text is English in the sources. The .ts files hold the other languages; their
# .qm are compiled into the program (resource prefix /i18n). ref-gui_en.ts holds only the plural
# forms of the English text ("%n line(s)" shows as "1 line" and "2 lines"). `update_translations`
# refreshes the .ts files from the sources.
qt_add_translations(ref-gui
	TS_FILES ${PROJECT_SOURCE_DIR}/translations/ref-gui_zh_CN.ts
	PLURALS_TS_FILE ${PROJECT_SOURCE_DIR}/translations/ref-gui_en.ts
)

# CodeBlock.qml imports org.kde.syntaxhighlighting at run time; nothing links against it.
find_package(KF6SyntaxHighlighting REQUIRED)

target_link_libraries(ref-gui PRIVATE refgui_workspace refgui_session refgui_composer refgui_notify refgui_cli refgui_icons Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2)
target_compile_definitions(ref-gui PRIVATE REF_GUI_VERSION="${PROJECT_VERSION}")
refgui_configure_target(ref-gui)

include(${PROJECT_SOURCE_DIR}/cmake/Deploy.cmake)
