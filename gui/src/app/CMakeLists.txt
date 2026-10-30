# Adapted from an early version of qutie by LightBlueCube
qt_add_executable(ref-gui main.cpp)

# Must be set before qt_add_qml_module() reads the files.
set_source_files_properties(Theme.qml Motion.qml Format.qml ToolInfo.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(ref-gui
	URI RefGui
	SOURCES
		app_translator.cpp
		app_translator.h
		chat_image_provider.cpp
		chat_image_provider.h
		chat_session.cpp
		chat_session.h
		qml_types.h
		system_fonts.h
		tile_model.cpp
		tile_model.h
		workspace.cpp
		workspace_commands.cpp
		workspace_requests.cpp
		workspace_split.cpp
		workspace.h
	QML_FILES
		Main.qml
		SessionPane.qml
		StatusPill.qml
		Sidebar.qml
		SessionRow.qml
		HistoryRow.qml
		SessionDot.qml
		ContextMenu.qml
		ThemedScrollBar.qml
		Crossfade.qml
		ToastHost.qml
		BackendBanner.qml
		TileGrid.qml
		Tile.qml
		LiveInfo.qml
		Format.qml
		DialogHost.qml
		ModalDialog.qml
		DialogButton.qml
		DialogField.qml
		Segmented.qml
		ToggleSwitch.qml
		KeyChips.qml
		PickList.qml
		PickRow.qml
		PathField.qml
		CommandPalette.qml
		SettingsDialog.qml
		SessionsDialog.qml
		ModelsDialog.qml
		AgentsDialog.qml
		VariantsDialog.qml
		McpDialog.qml
		PeersDialog.qml
		HelpDialog.qml
		ExportDialog.qml
		CdDialog.qml
		NewSessionDialog.qml
		CloseDialog.qml
		RenameDialog.qml
		DeleteHistoryDialog.qml
		Icon.qml
		Theme.qml
		Motion.qml
		Format.qml
		ChatView.qml
		Block.qml
		UserBlock.qml
		MarkdownBlock.qml
		CodeBlock.qml
		ReasoningBlock.qml
		ToolBlock.qml
		ProseText.qml
		StreamCaret.qml
		ToolStateSlot.qml
		CountdownRing.qml
		ToolInfo.qml
		ToolBody.qml
		ToolBodyBase.qml
		GenericBody.qml
		OutputSection.qml
		DiffBody.qml
		BashBody.qml
		ReadBody.qml
		GrepBody.qml
		FindBody.qml
		WebfetchBody.qml
		SessionBody.qml
		TodoBody.qml
		CommandBox.qml
		LineView.qml
		ToolChip.qml
		ToolSection.qml
		OutputText.qml
		CopyButton.qml
		PermissionSection.qml
		PermissionButton.qml
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
		ImageBlock.qml
		StatusLine.qml
		OlderPill.qml
		ShimmerText.qml
		Thumbnail.qml
		ImageStrip.qml
		Lightbox.qml
		MetaBlock.qml
		MetaText.qml
		MetaAction.qml
)

# Interface text is English in the sources. The .ts files hold the other languages; their
# .qm are compiled into the program (resource prefix /i18n). `update_translations` refreshes
# the .ts files from the sources.
qt_add_translations(ref-gui
	TS_FILES ${PROJECT_SOURCE_DIR}/translations/ref-gui_zh_CN.ts
)

# CodeBlock.qml imports org.kde.syntaxhighlighting at run time; nothing links against it.
find_package(KF6SyntaxHighlighting REQUIRED)

target_link_libraries(ref-gui PRIVATE refgui_workspace refgui_session refgui_cli refgui_icons Qt6::Gui Qt6::Qml Qt6::Quick)
target_compile_definitions(ref-gui PRIVATE REF_GUI_VERSION="${PROJECT_VERSION}")
refgui_configure_target(ref-gui)

include(${PROJECT_SOURCE_DIR}/cmake/Deploy.cmake)
