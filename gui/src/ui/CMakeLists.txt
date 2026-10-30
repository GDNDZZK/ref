# View models between the QML front end and whatever produces the conversation.
qt_add_library(qutie_ui STATIC
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

target_include_directories(qutie_ui PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(qutie_ui PUBLIC Qt6::Core qutie_llm)
qutie_configure_target(qutie_ui)
