# Talking to models: the wire formats and clients of their APIs, and where to reach them.
qt_add_library(refgui_llm STATIC
	config.cpp
	config.h
	message.h
	openai_client.cpp
	openai_client.h
	openai_wire.cpp
	openai_wire.h
)

target_include_directories(refgui_llm PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_llm PUBLIC refgui_infra)
refgui_configure_target(refgui_llm)
