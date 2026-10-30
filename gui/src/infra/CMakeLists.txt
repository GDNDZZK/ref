qt_add_library(refgui_infra STATIC
	http_stream.cpp
	http_stream.h
	jsonc.cpp
	jsonc.h
	sse.cpp
	sse.h
)

target_include_directories(refgui_infra PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_infra PUBLIC Qt6::Core Qt6::Network QCoro::Core)
refgui_configure_target(refgui_infra)
