# Adapted from an early version of qutie by LightBlueCube
# The conversation as the QML front end shows it, kept in step with a session on the backend.
qt_add_library(refgui_session STATIC
	blocks.cpp
	blocks.h
	conversation_feed.cpp
	conversation_feed.h
	conversation_model.cpp
	conversation_model.h
	fence_splitter.cpp
	fence_splitter.h
	markdown_text.cpp
	markdown_text.h
	session_controller.cpp
	session_controller.h
	stream_pacer.cpp
	stream_pacer.h
)

target_include_directories(refgui_session PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_session PUBLIC Qt6::Core refgui_proto refgui_client)
refgui_configure_target(refgui_session)
