# The ref wire protocol as C++ types, parsed from JSON.
qt_add_library(refgui_proto STATIC
	json_reader.cpp
	json_reader.h
	parse.h
	parse_config.cpp
	parse_frame.cpp
	parse_internal.h
	parse_session.cpp
	request.cpp
	request.h
	types.h
)

target_include_directories(refgui_proto PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_proto PUBLIC Qt6::Core)
refgui_configure_target(refgui_proto)
