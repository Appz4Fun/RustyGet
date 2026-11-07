cmake_minimum_required(VERSION 3.25...3.31)

if("${CMAKE_SOURCE_DIR}" STREQUAL "${CMAKE_BINARY_DIR}")
	message(FATAL_ERROR "In-source builds are not allowed. You should create separate directory for build files.")
endif()


# Force all FetchContent downloads/builds into a fixed, cacheable directory
# This ensures CI cache (build/deps) captures all dependencies
set(FETCHCONTENT_BASE_DIR ${CMAKE_BINARY_DIR}/deps CACHE PATH "FetchContent base directory" FORCE)

# Option to build external dependencies from source via FetchContent
# ON  = CI / reproducible builds (always build from pinned sources, no system lookup)
# OFF = Local development (try find_package first, fallback to FetchContent)
option(BUILD_DEPS_FROM_SOURCE "Build external dependencies from source via FetchContent (for CI / reproducible builds)" OFF)

# Prepend local cmake/ directory to module path for in-tree FindZLIB.cmake
# Required so libxml2's find_package(ZLIB) finds the FetchContent zlibstatic target
list(PREPEND CMAKE_MODULE_PATH ${CMAKE_SOURCE_DIR}/cmake)

set(VERSION "27.0")
set(PACKAGE "rustyget")

set(PACKAGE_BUGREPORT "https://github.com/xbmc4lyfe/RustyGet/issues")
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_C_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_C_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

string(REGEX MATCH "^([0-9]+)\\.([0-9]+)" VERSION_MATCH ${VERSION})
set(VERSION_MAJOR ${CMAKE_MATCH_1})
set(VERSION_MINOR ${CMAKE_MATCH_2})

if(NOT CMAKE_BUILD_TYPE)
	set(CMAKE_BUILD_TYPE "Release")
endif()

option(BUILD_ONLY_TESTS "Build only tests (for CI)")
option(ENABLE_TESTS "Enable unit tests")
set(USE_SANITIZERS "" CACHE STRING
    "Comma-separated sanitizers: address,undefined,thread,memory,leak (MSVC supports only address)")

project(
	rustyget
	VERSION ${VERSION}
	DESCRIPTION "RustyGet is a binary downloader, which downloads files from Usenet"
	LANGUAGES C CXX
)

if(WIN32 OR ANDROID)
	message(FATAL_ERROR "RustyGet builds on Linux and macOS only: Windows and Android support was retired.")
endif()

set(EXTERNAL_DEPS "")
include(cmake/common.cmake)

add_library(libnzbget STATIC)
set_target_properties(libnzbget PROPERTIES OUTPUT_NAME rustyget)
target_include_directories(libnzbget PUBLIC ${CMAKE_SOURCE_DIR} ${CMAKE_BINARY_DIR})

include(daemon/sources.cmake)

include(cmake/posix.cmake)

configure_file(
	${CMAKE_SOURCE_DIR}/cmake/config.h.in
	${CMAKE_BINARY_DIR}/config.h
)

if(EXTERNAL_DEPS)
	add_dependencies(libnzbget ${EXTERNAL_DEPS})
endif()

if(INCLUDES)
	target_include_directories(libnzbget PUBLIC ${INCLUDES})
endif()
if(LIBS)
	target_link_libraries(libnzbget PUBLIC ${LIBS})
endif()
target_precompile_headers(libnzbget PRIVATE ${CMAKE_SOURCE_DIR}/daemon/main/nzbget.h)
apply_compiler_flags(libnzbget)
apply_sanitizers(libnzbget)

if(NOT BUILD_ONLY_TESTS)
	add_executable(${PACKAGE} ${CMAKE_SOURCE_DIR}/daemon/main/nzbget.cpp)
	target_link_libraries(${PACKAGE} PRIVATE nzbget-rs-entry libnzbget)
	target_precompile_headers(${PACKAGE} REUSE_FROM libnzbget)
	apply_compiler_flags(${PACKAGE})
	apply_sanitizers(${PACKAGE})
endif()

if(NOT BUILD_ONLY_TESTS)
	if(CMAKE_BUILD_TYPE STREQUAL "Release" AND CMAKE_STRIP)
		add_custom_command(
			TARGET ${PACKAGE} POST_BUILD
			COMMAND ${CMAKE_STRIP} $<TARGET_FILE:${PACKAGE}>
		)
	endif()
	include(${CMAKE_SOURCE_DIR}/cmake/install.cmake)
endif()

if(ENABLE_TESTS OR BUILD_ONLY_TESTS)
	include(CTest)
	add_subdirectory(tests)
endif()
