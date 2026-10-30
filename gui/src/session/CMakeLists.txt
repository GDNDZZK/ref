# Adapted from an early version of qutie by LightBlueCube
# View models between the QML front end and whatever produces the conversation.
qt_add_library(refgui_session STATIC
	agent_driver.cpp
	agent_driver.h
	conversation_model.cpp
	conversation_model.h
	driver.h
	fence_splitter.cpp
	fence_splitter.h
	mock_driver.cpp
	mock_driver.h
	stream_pacer.cpp
	stream_pacer.h
)

target_include_directories(refgui_session PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_session PUBLIC Qt6::Core refgui_llm)
refgui_configure_target(refgui_session)
