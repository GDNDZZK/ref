qt_add_library(qutie_infra STATIC
	http_stream.cpp
	http_stream.h
	jsonc.cpp
	jsonc.h
	sse.cpp
	sse.h
)

target_include_directories(qutie_infra PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(qutie_infra PUBLIC Qt6::Core Qt6::Network QCoro::Core)
qutie_configure_target(qutie_infra)
