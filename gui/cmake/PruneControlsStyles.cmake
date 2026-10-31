# SPDX-License-Identifier: GPL-3.0-or-later
# Run at install time after the deploy step, with REF_GUI_DEPLOY_ROOT set to the install folder.
# Removes the Qt Quick Controls styles other than Basic: ref-gui sets the Basic style itself,
# and the scan for QML imports deploys every style because QtQuick.Controls lists them as
# optional imports. Removing them is what Qt documents for styles an application does not use.
#
# What goes: the style folders in qml/QtQuick/Controls/ (Fusion, Imagine, Material, Universal,
# FluentWinUI3, Windows, macOS, ...) and the libraries QuickControls2<Style> and
# QuickControls2<Style>StyleImpl. What stays: Basic, its impl folder, and the libraries of the
# Controls module itself.

set(kept_style_folders Basic impl)
set(kept_library_suffixes Basic BasicStyleImpl Impl)

# Everything is listed and filtered below: a "*" in the middle of a glob stands for one folder
# level only, and the macOS app keeps its QML modules four levels down (ref.app/Contents/Resources/qml).
file(GLOB_RECURSE entries LIST_DIRECTORIES true "${REF_GUI_DEPLOY_ROOT}/*")
foreach(entry IN LISTS entries)
	get_filename_component(parent "${entry}" DIRECTORY)
	get_filename_component(name "${entry}" NAME)
	if(parent MATCHES "/QtQuick/Controls$" AND IS_DIRECTORY "${entry}" AND NOT name IN_LIST kept_style_folders)
		file(REMOVE_RECURSE "${entry}")
	endif()
endforeach()

# Libraries are called libQt6QuickControls2Fusion.so.6 (Linux), Qt6QuickControls2Fusion.dll
# (Windows) and QtQuickControls2Fusion.framework (macOS). The QML folders were handled above.
file(GLOB_RECURSE entries LIST_DIRECTORIES true "${REF_GUI_DEPLOY_ROOT}/*QuickControls2*")
foreach(entry IN LISTS entries)
	if(NOT EXISTS "${entry}" OR entry MATCHES "/QtQuick/Controls/")
		continue()
	endif()
	get_filename_component(name "${entry}" NAME)
	if(NOT name MATCHES "QuickControls2([A-Za-z0-9]+)")
		continue() # QuickControls2 alone is the Controls library
	endif()
	if(NOT CMAKE_MATCH_1 IN_LIST kept_library_suffixes)
		file(REMOVE_RECURSE "${entry}")
	endif()
endforeach()
