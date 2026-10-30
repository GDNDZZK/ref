cmake_minimum_required(VERSION 3.25)

project(qutie VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(QT_QML_GENERATE_QMLLS_INI ON)

# 6.10 is the floor imposed by KSyntaxHighlighting.
find_package(Qt6 6.10 REQUIRED COMPONENTS Core Gui Network Qml Quick Test)
qt_standard_project_setup(REQUIRES 6.10)

# QCoro::Core brings QCoro::Task. Tested with 0.13.
find_package(QCoro6 REQUIRED COMPONENTS Core)

include(cmake/CompilerOptions.cmake)

add_subdirectory(src)

enable_testing()
add_subdirectory(tests)
