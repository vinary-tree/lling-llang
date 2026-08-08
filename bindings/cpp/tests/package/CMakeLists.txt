cmake_minimum_required(VERSION 3.20)
project(lling_llang_package_smoke LANGUAGES CXX)

find_package(lling-llang 0.2 CONFIG REQUIRED)
add_executable(lling_llang_package_smoke ../package_smoke.cpp)
target_compile_features(lling_llang_package_smoke PRIVATE cxx_std_20)
target_link_libraries(lling_llang_package_smoke PRIVATE lling-llang::lling-llang)
