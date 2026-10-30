# What the input box does apart from drawing: the draft with its image tokens, history, the
# `/` and `@` lists.
qt_add_library(refgui_compose STATIC
	completion_target.cpp
	completion_target.h
	image_file.cpp
	image_file.h
	prompt_draft.cpp
	prompt_draft.h
	prompt_history.cpp
	prompt_history.h
)

target_include_directories(refgui_compose PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_compose PUBLIC Qt6::Core refgui_proto)
refgui_configure_target(refgui_compose)
