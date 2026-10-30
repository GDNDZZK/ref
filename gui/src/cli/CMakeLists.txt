# What the command line asks for, apart from the GUI.
qt_add_library(refgui_cli STATIC
	command_line.cpp
	command_line.h
	launch_options.h
)

target_include_directories(refgui_cli PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_cli PUBLIC Qt6::Core refgui_client refgui_proto)
refgui_configure_target(refgui_cli)
