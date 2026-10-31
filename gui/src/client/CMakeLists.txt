# Finding, starting and talking to ref-server.
qt_add_library(refgui_client STATIC
	backend.cpp
	backend.h
	endpoint.h
	instance_files.cpp
	instance_files.h
	launcher.cpp
	launcher.h
	mock_backend.cpp
	mock_backend.h
	process_alive.cpp
	process_alive.h
	server_client.cpp
	server_client.h
	spawn_lock.cpp
	spawn_lock.h
	stop_source.cpp
	stop_source.h
)

target_include_directories(refgui_client PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_client PUBLIC refgui_proto Qt6::Network Qt6::WebSockets QCoro6::Core QCoro6::WebSockets)
refgui_configure_target(refgui_client)
