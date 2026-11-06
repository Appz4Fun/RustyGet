if(NOT TARGET Boost::unit_test_framework)
	find_package(Boost COMPONENTS unit_test_framework REQUIRED)
endif()

set(TESTS_TARGET nzbget_tests)
set(TESTS_SRC ${CMAKE_SOURCE_DIR}/tests/main.cpp)

include(queue/queue.cmake)
include(postprocess/postprocess.cmake)
list(APPEND TESTS_SRC ${CMAKE_CURRENT_SOURCE_DIR}/connect/TlsCipher.cpp)

add_executable(${TESTS_TARGET} ${TESTS_SRC})
if(EXTERNAL_DEPS)
	add_dependencies(${TESTS_TARGET} ${EXTERNAL_DEPS})
endif()

target_link_libraries(${TESTS_TARGET} PRIVATE libnzbget Boost::unit_test_framework)

apply_compiler_flags(${TESTS_TARGET})
apply_sanitizers(${TESTS_TARGET})

target_precompile_headers(${TESTS_TARGET} REUSE_FROM libnzbget)

add_test(NAME QueueTest        COMMAND ${TESTS_TARGET} --run_test=QueueTest --log_level=message)
add_test(NAME PostprocessTest  COMMAND ${TESTS_TARGET} --run_test=PostprocessTest --log_level=message)
if(TARGET ${PACKAGE})
	# these leave through exit(): the daemon's destructors must still run cleanly
	add_test(NAME ExitVersionTest COMMAND ${PACKAGE} --version)
	add_test(NAME ExitHelpTest    COMMAND ${PACKAGE} --help)
endif()
if(TARGET nzbget-rs AND NOT DISABLE_TLS AND (APPLE OR CMAKE_SYSTEM_NAME STREQUAL "Linux"))
	add_test(NAME ConnectTest COMMAND ${TESTS_TARGET} --run_test=ConnectTest --log_level=message)
	set_tests_properties(ConnectTest PROPERTIES TIMEOUT 60)
endif()
