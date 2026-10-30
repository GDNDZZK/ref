# The input box of a session apart from how it is drawn: the draft, the lists, the queue.
qt_add_library(refgui_composer STATIC
	attachment_model.cpp
	attachment_model.h
	composer_model.cpp
	composer_model.h
	file_model.cpp
	file_model.h
	shared_history.cpp
	shared_history.h
	queue_model.cpp
	queue_model.h
)

target_include_directories(refgui_composer PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_composer PUBLIC refgui_compose refgui_session refgui_client Qt6::Gui)
refgui_configure_target(refgui_composer)
