# Builds the Qt plugin that capture/gui.ts loads into an unmodified ref-gui.
cmake_minimum_required(VERSION 3.25)
project(refpromo_driver LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.10 REQUIRED COMPONENTS Quick Test)

qt_add_plugin(refpromo PLUGIN_TYPE generic CLASS_NAME DriverPlugin driver.cpp)
target_link_libraries(refpromo PRIVATE Qt6::Quick Qt6::Test)
set_target_properties(refpromo PROPERTIES LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/plugins/generic)
