cmake_minimum_required(VERSION 3.14)
project(way-embed C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Build llama.cpp as a subdirectory
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON ON CACHE BOOL "" FORCE)
# way-embed embeds locally and makes no HTTPS requests, so the vendored
# cpp-httplib is built without OpenSSL. Its OpenSSL code does not compile
# against the OpenSSL on current macOS runners.
set(LLAMA_OPENSSL OFF CACHE BOOL "" FORCE)
set(GGML_CUDA OFF CACHE BOOL "" FORCE)
set(GGML_METAL OFF CACHE BOOL "" FORCE)
set(GGML_VULKAN OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)

add_subdirectory(llama.cpp)

# way-embed binary
add_executable(way-embed way-embed.cpp)
target_link_libraries(way-embed PRIVATE common llama)
target_include_directories(way-embed PRIVATE
    llama.cpp/include
    llama.cpp/common
)

# Install to bin/
install(TARGETS way-embed DESTINATION ${CMAKE_SOURCE_DIR}/../../bin)
