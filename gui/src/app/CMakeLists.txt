qt_add_executable(qutie main.cpp)

# Must be set before qt_add_qml_module() reads the files.
set_source_files_properties(Theme.qml Motion.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(qutie
	URI Qutie
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

target_link_libraries(qutie PRIVATE qutie_ui qutie_llm Qt6::Gui Qt6::Qml Qt6::Quick)
qutie_configure_target(qutie)
