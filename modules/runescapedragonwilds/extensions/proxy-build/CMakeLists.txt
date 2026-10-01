cmake_minimum_required(VERSION 3.22)
project(DragonwildsOfficialProxy LANGUAGES CXX ASM_MASM RC)
if(NOT LGSM_PROXY_SOURCE_DIR)
    message(FATAL_ERROR "LGSM_PROXY_SOURCE_DIR must identify the verified source directory")
endif()
set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
add_compile_definitions(UNICODE _UNICODE PLATFORM_WINDOWS NOMINMAX
    RC_FILE_BUILD_STATIC RC_HELPERS_BUILD_STATIC FMT_HEADER_ONLY)
add_compile_options("$<$<COMPILE_LANGUAGE:CXX>:/utf-8>")
add_library(Constructs INTERFACE)
target_include_directories(Constructs INTERFACE
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/Constructs/include")
add_library(File STATIC
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/File/src/File.cpp"
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/File/src/FileType/WinFile.cpp"
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/Helpers/src/SysError.cpp")
target_include_directories(File PUBLIC
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/File/include"
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/Helpers/include"
    "${LGSM_PROXY_SOURCE_DIR}/deps/first/String/include"
    "${LGSM_PROXY_SOURCE_DIR}/deps/third/fmt-11.2.0/include")
set(UE4SS_PROXY_PATH "$ENV{WINDIR}/System32/version.dll" CACHE FILEPATH
    "Signed local Windows DLL whose exports the official generator forwards")
add_subdirectory("${LGSM_PROXY_SOURCE_DIR}/UE4SS/proxy_generator" proxy_generator)
