# SPDX-License-Identifier: GPL-3.0-or-later
qt_add_executable(tst_jsonc tst_jsonc.cpp)
target_link_libraries(tst_jsonc PRIVATE qutie_infra Qt6::Test)
qutie_configure_target(tst_jsonc)
add_test(NAME tst_jsonc COMMAND tst_jsonc)

qt_add_executable(tst_sse tst_sse.cpp)
target_link_libraries(tst_sse PRIVATE qutie_infra Qt6::Test)
qutie_configure_target(tst_sse)
add_test(NAME tst_sse COMMAND tst_sse)

qt_add_executable(tst_config tst_config.cpp)
target_link_libraries(tst_config PRIVATE qutie_llm Qt6::Test)
qutie_configure_target(tst_config)
add_test(NAME tst_config COMMAND tst_config)

qt_add_executable(tst_openai_wire tst_openai_wire.cpp)
target_link_libraries(tst_openai_wire PRIVATE qutie_llm Qt6::Test)
qutie_configure_target(tst_openai_wire)
add_test(NAME tst_openai_wire COMMAND tst_openai_wire)

qt_add_executable(tst_openai_client tst_openai_client.cpp fake_server.h)
target_link_libraries(tst_openai_client PRIVATE qutie_llm Qt6::Test)
qutie_configure_target(tst_openai_client)
add_test(NAME tst_openai_client COMMAND tst_openai_client)

qt_add_executable(tst_fence_splitter tst_fence_splitter.cpp)
target_link_libraries(tst_fence_splitter PRIVATE qutie_ui Qt6::Test)
qutie_configure_target(tst_fence_splitter)
add_test(NAME tst_fence_splitter COMMAND tst_fence_splitter)

qt_add_executable(tst_conversation_model tst_conversation_model.cpp)
target_link_libraries(tst_conversation_model PRIVATE qutie_ui Qt6::Test)
qutie_configure_target(tst_conversation_model)
add_test(NAME tst_conversation_model COMMAND tst_conversation_model)

qt_add_executable(tst_stream_pacer tst_stream_pacer.cpp)
target_link_libraries(tst_stream_pacer PRIVATE qutie_ui Qt6::Test)
qutie_configure_target(tst_stream_pacer)
add_test(NAME tst_stream_pacer COMMAND tst_stream_pacer)

qt_add_executable(tst_agent_driver tst_agent_driver.cpp fake_server.h)
target_link_libraries(tst_agent_driver PRIVATE qutie_ui Qt6::Test)
qutie_configure_target(tst_agent_driver)
add_test(NAME tst_agent_driver COMMAND tst_agent_driver)
