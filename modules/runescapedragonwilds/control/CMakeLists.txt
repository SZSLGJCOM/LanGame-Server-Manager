cmake_minimum_required(VERSION 3.22)
project(DragonwildsPlayerControl LANGUAGES CXX)
if(NOT WIN32 OR NOT MSVC OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "Dragonwilds player control requires Windows x64 MSVC")
endif()
add_library(LgsmPlayerControl SHARED bridge.cpp)
target_compile_features(LgsmPlayerControl PRIVATE cxx_std_23)
target_compile_definitions(LgsmPlayerControl PRIVATE UNICODE _UNICODE NOMINMAX WIN32_LEAN_AND_MEAN)
target_compile_options(LgsmPlayerControl PRIVATE /W4 /WX /utf-8 /guard:cf)
target_link_options(LgsmPlayerControl PRIVATE /guard:cf)
target_link_libraries(LgsmPlayerControl PRIVATE bcrypt)
set_target_properties(LgsmPlayerControl PROPERTIES
    MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
