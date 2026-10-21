cmake_minimum_required(VERSION 3.22)
project(DragonwildsPlayerControl LANGUAGES C CXX)
if(NOT WIN32 OR NOT MSVC OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "Dragonwilds player control requires Windows x64 MSVC")
endif()
set(MINHOOK "${CMAKE_CURRENT_SOURCE_DIR}/../../enshrouded/control/vendor/minhook")
add_library(DragonSaveMinHook STATIC "${MINHOOK}/src/buffer.c" "${MINHOOK}/src/hook.c"
    "${MINHOOK}/src/trampoline.c" "${MINHOOK}/src/hde/hde64.c")
# The already reviewed third-party C source keeps its upstream warning policy.
# Every LanGame C++ translation unit below retains /W4 /WX.
target_compile_definitions(DragonSaveMinHook PRIVATE UNICODE _UNICODE NOMINMAX WIN32_LEAN_AND_MEAN)
set_target_properties(DragonSaveMinHook PROPERTIES MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
add_library(LgsmPlayerControl SHARED bridge.cpp save_bridge.cpp save_native.cpp save_sinks.cpp save_profile.cpp save_files.cpp)
target_compile_features(LgsmPlayerControl PRIVATE cxx_std_23)
target_compile_definitions(LgsmPlayerControl PRIVATE UNICODE _UNICODE NOMINMAX WIN32_LEAN_AND_MEAN)
target_compile_options(LgsmPlayerControl PRIVATE /W4 /WX /utf-8 /guard:cf)
target_link_options(LgsmPlayerControl PRIVATE /guard:cf)
target_link_libraries(LgsmPlayerControl PRIVATE bcrypt DragonSaveMinHook)
set_target_properties(LgsmPlayerControl PROPERTIES
    MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
