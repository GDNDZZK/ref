# Adapted from an early version of qutie by LightBlueCube
cmake_minimum_required(VERSION 3.25)

# The GUI is versioned with the rest of ref: the workspace version in the root Cargo.toml.
set(REF_CARGO_TOML "${CMAKE_CURRENT_SOURCE_DIR}/../Cargo.toml")
file(STRINGS "${REF_CARGO_TOML}" REF_VERSION_LINE REGEX "^version = \"[0-9.]+\"$" LIMIT_COUNT 1)
if(NOT REF_VERSION_LINE MATCHES "\"([0-9.]+)\"")
	message(FATAL_ERROR "No workspace version found in ${REF_CARGO_TOML}")
endif()
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${REF_CARGO_TOML}")

project(ref-gui VERSION ${CMAKE_MATCH_1} LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(QT_QML_GENERATE_QMLLS_INI ON)

# 6.10 is the floor imposed by KSyntaxHighlighting.
find_package(Qt6 6.10 REQUIRED COMPONENTS Core Gui Network WebSockets Qml Quick Test)
qt_standard_project_setup(REQUIRES 6.10)

# QCoro::Core brings QCoro::Task. Tested with 0.13.
find_package(QCoro6 REQUIRED COMPONENTS Core Network WebSockets)

include(cmake/CompilerOptions.cmake)

add_subdirectory(src)

enable_testing()
add_subdirectory(tests)
