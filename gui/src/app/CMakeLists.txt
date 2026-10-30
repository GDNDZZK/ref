# Adapted from an early version of qutie by LightBlueCube
qt_add_executable(ref-gui main.cpp)

# Must be set before qt_add_qml_module() reads the files.
set_source_files_properties(Theme.qml Motion.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(ref-gui
	URI RefGui
	SOURCES
		chat_session.cpp
		chat_session.h
		system_fonts.h
	QML_FILES
		Main.qml
		Theme.qml
		Motion.qml
		ChatView.qml
		Block.qml
		UserBlock.qml
		MarkdownBlock.qml
		CodeBlock.qml
		ReasoningBlock.qml
		ToolBlock.qml
		ProseText.qml
		StreamCaret.qml
		StatusDot.qml
		Composer.qml
		SendButton.qml
		TextButton.qml
		CircleButton.qml
		ThemeToggle.qml
		Chevron.qml
		Pulse.qml
		Flourish.qml
		AlertBadge.qml
		ErrorBlock.qml
)

# CodeBlock.qml imports org.kde.syntaxhighlighting at run time; nothing links against it.
find_package(KF6SyntaxHighlighting REQUIRED)

target_link_libraries(ref-gui PRIVATE refgui_session Qt6::Gui Qt6::Qml Qt6::Quick)
target_compile_definitions(ref-gui PRIVATE REF_GUI_VERSION="${PROJECT_VERSION}")
refgui_configure_target(ref-gui)
