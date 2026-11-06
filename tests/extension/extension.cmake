list(APPEND TESTS_SRC
	${CMAKE_CURRENT_SOURCE_DIR}/extension/Extension.cpp
	${CMAKE_CURRENT_SOURCE_DIR}/extension/ExtensionManager.cpp
	${CMAKE_CURRENT_SOURCE_DIR}/extension/ScriptConfig.cpp
)

file(COPY ${CMAKE_CURRENT_SOURCE_DIR}/testdata/extension/scripts DESTINATION ${CMAKE_CURRENT_BINARY_DIR})
