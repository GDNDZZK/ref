# What the window keeps above single sessions: settings, commands, the session lists.
qt_add_library(refgui_workspace STATIC
	command_registry.cpp
	command_registry.h
	settings.cpp
	settings.h
)

target_include_directories(refgui_workspace PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_workspace PUBLIC Qt6::Core refgui_proto refgui_client)
refgui_configure_target(refgui_workspace)
