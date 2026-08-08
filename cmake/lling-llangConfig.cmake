include_guard(GLOBAL)

include(CMakeFindDependencyMacro)
find_dependency(Threads)
find_dependency(vinary-tree-interop 0.1 CONFIG)

get_filename_component(_LLING_LLANG_PREFIX "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)

if(NOT TARGET lling-llang::shared)
  add_library(lling-llang::shared SHARED IMPORTED)
  set_target_properties(lling-llang::shared PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${_LLING_LLANG_PREFIX}/include"
    INTERFACE_LINK_LIBRARIES "vinary-tree::interop"
  )
  if(WIN32)
    set_target_properties(lling-llang::shared PROPERTIES
      IMPORTED_LOCATION "${_LLING_LLANG_PREFIX}/bin/lling_llang.dll"
      IMPORTED_IMPLIB "${_LLING_LLANG_PREFIX}/lib/lling_llang.dll.lib"
    )
  elseif(APPLE)
    set_target_properties(lling-llang::shared PROPERTIES
      IMPORTED_LOCATION "${_LLING_LLANG_PREFIX}/lib/liblling_llang.dylib"
    )
  else()
    set_target_properties(lling-llang::shared PROPERTIES
      IMPORTED_LOCATION "${_LLING_LLANG_PREFIX}/lib/liblling_llang.so"
    )
  endif()
endif()

if(NOT TARGET lling-llang::static)
  add_library(lling-llang::static STATIC IMPORTED)
  set_target_properties(lling-llang::static PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${_LLING_LLANG_PREFIX}/include"
    INTERFACE_LINK_LIBRARIES "vinary-tree::interop"
  )
  if(WIN32)
    set_target_properties(lling-llang::static PROPERTIES
      IMPORTED_LOCATION "${_LLING_LLANG_PREFIX}/lib/lling_llang.lib"
      INTERFACE_LINK_LIBRARIES "bcrypt;userenv;ws2_32;ntdll;synchronization;advapi32;Threads::Threads"
    )
  elseif(APPLE)
    find_library(_LLING_LLANG_ICONV_LIBRARY NAMES iconv REQUIRED)
    find_library(_LLING_LLANG_COREFOUNDATION_FRAMEWORK NAMES CoreFoundation REQUIRED)
    find_library(_LLING_LLANG_SECURITY_FRAMEWORK NAMES Security REQUIRED)
    set_target_properties(lling-llang::static PROPERTIES
      IMPORTED_LOCATION "${_LLING_LLANG_PREFIX}/lib/liblling_llang.a"
      INTERFACE_LINK_LIBRARIES "${CMAKE_DL_LIBS};Threads::Threads;m;${_LLING_LLANG_ICONV_LIBRARY};${_LLING_LLANG_COREFOUNDATION_FRAMEWORK};${_LLING_LLANG_SECURITY_FRAMEWORK}"
    )
  else()
    set_target_properties(lling-llang::static PROPERTIES
      IMPORTED_LOCATION "${_LLING_LLANG_PREFIX}/lib/liblling_llang.a"
      INTERFACE_LINK_LIBRARIES "${CMAKE_DL_LIBS};Threads::Threads;m"
    )
  endif()
endif()

if(NOT DEFINED LLING_LLANG_LINKAGE)
  set(LLING_LLANG_LINKAGE "SHARED")
endif()
string(TOUPPER "${LLING_LLANG_LINKAGE}" _LLING_LLANG_LINKAGE)
if(NOT _LLING_LLANG_LINKAGE STREQUAL "SHARED" AND NOT _LLING_LLANG_LINKAGE STREQUAL "STATIC")
  message(FATAL_ERROR "LLING_LLANG_LINKAGE must be SHARED or STATIC")
endif()
if(NOT TARGET lling-llang::lling-llang)
  add_library(lling-llang::lling-llang INTERFACE IMPORTED)
  if(_LLING_LLANG_LINKAGE STREQUAL "STATIC")
    set_property(TARGET lling-llang::lling-llang PROPERTY INTERFACE_LINK_LIBRARIES lling-llang::static)
  else()
    set_property(TARGET lling-llang::lling-llang PROPERTY INTERFACE_LINK_LIBRARIES lling-llang::shared)
  endif()
endif()

set(lling-llang_FOUND TRUE)
unset(_LLING_LLANG_LINKAGE)
unset(_LLING_LLANG_PREFIX)
