# CMake toolchain file for C dependencies built through the cmake crate (opusic-sys) on Windows MSVC
# targets; config.toml names it in CMAKE_TOOLCHAIN_FILE_<target>. +crt-static links Rust against the
# static CRT, but CMake 3.15+ compiles C with the DLL CRT (/MD) unless told otherwise, which mixes
# runtimes (LNK4098). Rust uses the release CRT in every profile, so this is MultiThreaded throughout.
set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreaded)
# opus sets CMAKE_MSVC_RUNTIME_LIBRARY itself, from this option (default OFF).
set(OPUS_STATIC_RUNTIME ON CACHE BOOL "build with static runtime library.")

# Naming a toolchain file stops the cmake crate from describing a cross build, so do it here
# (opus picks its SIMD code from CMAKE_SYSTEM_PROCESSOR).
if(NOT "$ENV{TARGET}" STREQUAL "$ENV{HOST}")
  set(CMAKE_SYSTEM_NAME Windows)
  if("$ENV{TARGET}" MATCHES "^aarch64")
    set(CMAKE_SYSTEM_PROCESSOR ARM64)
  else()
    set(CMAKE_SYSTEM_PROCESSOR AMD64)
  endif()
endif()

# config.toml gives fdk-aac's C++ sources /std:c++latest on ARM64 through CFLAGS_aarch64_pc_windows_msvc, and the
# cmake crate copies CFLAGS into these flags too, where it clashes with opus's /std:c11 (cl D8016). Drop it here:
# this toolchain file is only used by cmake builds, and opus is C.
foreach(flags CMAKE_C_FLAGS CMAKE_C_FLAGS_RELEASE CMAKE_C_FLAGS_DEBUG CMAKE_C_FLAGS_RELWITHDEBINFO CMAKE_C_FLAGS_MINSIZEREL)
  if(DEFINED ${flags})
    string(REPLACE "/std:c++latest" "" ${flags} "${${flags}}")
  endif()
endforeach()
