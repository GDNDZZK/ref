# Adapted from an early version of qutie by LightBlueCube
# SPDX-License-Identifier: GPL-3.0-or-later
qt_add_executable(tst_jsonc tst_jsonc.cpp)
target_link_libraries(tst_jsonc PRIVATE refgui_infra Qt6::Test)
refgui_configure_target(tst_jsonc)
add_test(NAME tst_jsonc COMMAND tst_jsonc)

qt_add_executable(tst_sse tst_sse.cpp)
target_link_libraries(tst_sse PRIVATE refgui_infra Qt6::Test)
refgui_configure_target(tst_sse)
add_test(NAME tst_sse COMMAND tst_sse)

qt_add_executable(tst_config tst_config.cpp)
target_link_libraries(tst_config PRIVATE refgui_llm Qt6::Test)
refgui_configure_target(tst_config)
add_test(NAME tst_config COMMAND tst_config)

qt_add_executable(tst_openai_wire tst_openai_wire.cpp)
target_link_libraries(tst_openai_wire PRIVATE refgui_llm Qt6::Test)
refgui_configure_target(tst_openai_wire)
add_test(NAME tst_openai_wire COMMAND tst_openai_wire)

qt_add_executable(tst_openai_client tst_openai_client.cpp fake_server.h)
target_link_libraries(tst_openai_client PRIVATE refgui_llm Qt6::Test)
refgui_configure_target(tst_openai_client)
add_test(NAME tst_openai_client COMMAND tst_openai_client)

qt_add_executable(tst_fence_splitter tst_fence_splitter.cpp)
target_link_libraries(tst_fence_splitter PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_fence_splitter)
add_test(NAME tst_fence_splitter COMMAND tst_fence_splitter)

qt_add_executable(tst_conversation_model tst_conversation_model.cpp)
target_link_libraries(tst_conversation_model PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_conversation_model)
add_test(NAME tst_conversation_model COMMAND tst_conversation_model)

qt_add_executable(tst_stream_pacer tst_stream_pacer.cpp)
target_link_libraries(tst_stream_pacer PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_stream_pacer)
add_test(NAME tst_stream_pacer COMMAND tst_stream_pacer)

qt_add_executable(tst_agent_driver tst_agent_driver.cpp fake_server.h)
target_link_libraries(tst_agent_driver PRIVATE refgui_session Qt6::Test)
refgui_configure_target(tst_agent_driver)
add_test(NAME tst_agent_driver COMMAND tst_agent_driver)

qt_add_executable(tst_proto tst_proto.cpp)
target_link_libraries(tst_proto PRIVATE refgui_proto Qt6::Test)
target_compile_definitions(tst_proto PRIVATE REF_FIXTURES_DIR="${PROJECT_SOURCE_DIR}/../crates/proto/fixtures")
refgui_configure_target(tst_proto)
add_test(NAME tst_proto COMMAND tst_proto)

qt_add_executable(tst_server_client tst_server_client.cpp coro_test.h fake_backend.h)
target_link_libraries(tst_server_client PRIVATE refgui_client Qt6::Test)
refgui_configure_target(tst_server_client)
add_test(NAME tst_server_client COMMAND tst_server_client)

qt_add_executable(tst_launcher tst_launcher.cpp coro_test.h)
target_link_libraries(tst_launcher PRIVATE refgui_client Qt6::Test)
refgui_configure_target(tst_launcher)
add_test(NAME tst_launcher COMMAND tst_launcher)
