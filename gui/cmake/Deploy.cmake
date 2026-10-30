# SPDX-License-Identifier: GPL-3.0-or-later
# Install rules that turn a build of ref-gui into a folder (or ref-gui.app) a user can run:
#
#   cmake -S gui -B build -DCMAKE_INSTALL_BINDIR=. ...
#   cmake --install build --prefix stage
#
# Layout of the prefix, the same shape on every platform (macOS: all of it inside ref-gui.app):
#
#   ref-gui[.exe]   the program        lib/      Qt and other shared libraries (Linux)
#   qt.conf         where Qt looks     plugins/  Qt plugins
#   qml/            QML modules        (Windows: Qt's DLLs sit next to ref-gui.exe)
#
# What goes in is found from the program itself: Qt's deploy API scans the QML sources for the
# modules they import and copies those with their plugins and libraries. Nothing is listed here.
# CMAKE_INSTALL_BINDIR=. puts ref-gui in the prefix itself, not in bin/; it has to be given at
# configure time because Qt reads it then.
#
# Needs the target ref-gui; included at the end of src/app/CMakeLists.txt.

set(REF_GUI_ASSETS_APP "${PROJECT_SOURCE_DIR}/../assets/app")

# Directories of QML modules that are not Qt's own, e.g. KSyntaxHighlighting's in the
# dependency prefix. The scan only finds modules in the import paths it is given.
set(ref_gui_qml_import_paths "")
foreach(prefix IN LISTS CMAKE_PREFIX_PATH)
	if(IS_DIRECTORY "${prefix}/lib/qml")
		list(APPEND ref_gui_qml_import_paths "${prefix}/lib/qml")
	endif()
endforeach()
set_target_properties(ref-gui PROPERTIES QT_QML_IMPORT_PATH "${ref_gui_qml_import_paths}")

set(ref_gui_deploy_tool_options "")
set(ref_gui_deploy_exclude_plugin_types "")

if(WIN32)
	# GUI subsystem: no console window.
	set_target_properties(ref-gui PROPERTIES WIN32_EXECUTABLE ON)

	# Icon and version information of ref-gui.exe.
	enable_language(RC)
	set(REF_GUI_ICON "${REF_GUI_ASSETS_APP}/ref.ico")
	set(REF_GUI_VERSION_COMMAS "${PROJECT_VERSION_MAJOR},${PROJECT_VERSION_MINOR},${PROJECT_VERSION_PATCH},0")
	configure_file("${CMAKE_CURRENT_LIST_DIR}/ref-gui.rc.in" "${CMAKE_CURRENT_BINARY_DIR}/ref-gui.rc" @ONLY)
	target_sources(ref-gui PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/ref-gui.rc")

	# windeployqt copies Qt and the VC++ runtime, but knows nothing of other DLLs.
	install(IMPORTED_RUNTIME_ARTIFACTS KF6::SyntaxHighlighting RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")

	# The software OpenGL fallback is 20 MB of DLL that a desktop with a GPU driver never loads.
	list(APPEND ref_gui_deploy_tool_options --no-opengl-sw)
elseif(APPLE)
	set(icon_file "${REF_GUI_ASSETS_APP}/ref.icns")
	target_sources(ref-gui PRIVATE "${icon_file}")
	set_source_files_properties("${icon_file}" PROPERTIES MACOSX_PACKAGE_LOCATION Resources)
	set_target_properties(ref-gui PROPERTIES
		MACOSX_BUNDLE ON
		MACOSX_BUNDLE_ICON_FILE ref.icns
		MACOSX_BUNDLE_BUNDLE_NAME ref
		MACOSX_BUNDLE_GUI_IDENTIFIER io.github.gdndzzk.ref
		MACOSX_BUNDLE_SHORT_VERSION_STRING "${PROJECT_VERSION}"
		MACOSX_BUNDLE_BUNDLE_VERSION "${PROJECT_VERSION}"
	)

	# macdeployqt looks for the non-Qt libraries the plugins need in these directories.
	foreach(prefix IN LISTS CMAKE_PREFIX_PATH)
		list(APPEND ref_gui_deploy_tool_options "-libpath=${prefix}/lib")
	endforeach()
else()
	# The libraries sit in lib/ next to the program, wherever the folder is moved to.
	set_target_properties(ref-gui PROPERTIES INSTALL_RPATH "$ORIGIN/lib")

	# Plugins of the modules we load that a desktop run does not use: QML debugger and profiler
	# plugins, the input and display backends of embedded devices, and the helpers of the
	# Wayland platform plugin, which is not deployed (the xcb plugin runs on XWayland).
	set(ref_gui_deploy_exclude_plugin_types
		qmltooling egldeviceintegrations generic
		wayland-decoration-client wayland-graphics-integration-client wayland-shell-integration
	)
endif()

install(TARGETS ref-gui RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}" BUNDLE DESTINATION .)

qt_generate_deploy_qml_app_script(
	TARGET ref-gui
	OUTPUT_SCRIPT ref_gui_deploy_script
	NO_UNSUPPORTED_PLATFORM_ERROR
	NO_TRANSLATIONS
	DEPLOY_TOOL_OPTIONS ${ref_gui_deploy_tool_options}
	EXCLUDE_PLUGIN_TYPES ${ref_gui_deploy_exclude_plugin_types}
)
install(SCRIPT "${ref_gui_deploy_script}")
