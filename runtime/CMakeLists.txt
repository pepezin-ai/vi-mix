# Runtime C++ d'AnyXboxX : DLL qui remplacent les bibliothÃ¨ques de la console.
#
#   cargo build --release            (DLL Rust d'abord, voir rust_dlls plus bas)
#   cmake -S runtime -B runtime/build -A x64
#   cmake --build runtime/build --config Release
#
# Les DLL sont produites dans runtime/build/bin, oÃ¹ le mode natif les cherche
# (ou dans le dossier donnÃ© par la variable ANYXBOXX_RUNTIME).
cmake_minimum_required(VERSION 3.21)
project(AnyXboxXRuntime LANGUAGES CXX)

if(NOT WIN32 OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
  message(FATAL_ERROR "Le runtime d'AnyXboxX se compile pour Windows x64 uniquement.")
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
# MÃªme CRT que Python (ucrtbase) : stderr est partagÃ© avec le journal d'AnyXboxX.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")
# Â« $<0:> Â» Ã©vite le sous-dossier Release/Debug des gÃ©nÃ©rateurs multi-configurations.
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin/$<0:>")

function(anyxboxx_dll name)
  add_library(${name} SHARED ${ARGN})
  set_target_properties(${name} PROPERTIES PREFIX "" OUTPUT_NAME ${name})
  target_include_directories(${name} PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/common")
  target_compile_definitions(${name} PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX UNICODE _UNICODE)
  target_compile_options(${name} PRIVATE /W4 /WX /permissive- /utf-8)
endfunction()

# Module Task de libHttpClient (MIT) : XTaskQueue et XAsync, servis par xgameruntime.
set(TASK_DIR "${CMAKE_CURRENT_SOURCE_DIR}/third_party/libhttpclient-task")
add_library(anyxboxx_task STATIC
  ${TASK_DIR}/source/AsyncLib.cpp
  ${TASK_DIR}/source/TaskQueue.cpp
  ${TASK_DIR}/source/ThreadPool_win32.cpp
  ${TASK_DIR}/source/WaitTimer_win32.cpp)
target_include_directories(anyxboxx_task PUBLIC "${TASK_DIR}/include" PRIVATE "${TASK_DIR}/source")
target_compile_definitions(anyxboxx_task PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(anyxboxx_task PRIVATE /W3 /utf-8 /permissive-)

anyxboxx_dll(xgameruntime_cpp xgameruntime/xgameruntime.cpp xgameruntime/served_interfaces.cpp
             xgameruntime/task_queue_interface.cpp xgameruntime/user_interface.cpp
             xgameruntime/system_interface.cpp xgameruntime/game_config.cpp
             xgameruntime/registry.cpp xgameruntime/save_store.cpp xgameruntime/save_interface.cpp
             xgameruntime/local_storage_interface.cpp xgameruntime/package_interface.cpp
             xgameruntime/frame_counter.cpp xgameruntime/event_interface.cpp
             xgameruntime/platform_interface.cpp xgameruntime/gamer_picture.cpp
             xgameruntime/store_interface.cpp
             xgameruntime/xgameruntime.def)
# Keep the oracle's DLL name (including its PE export directory) in a separate folder.
set_target_properties(xgameruntime_cpp PROPERTIES OUTPUT_NAME xgameruntime
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/oracle/$<0:>")
target_link_libraries(xgameruntime_cpp PRIVATE anyxboxx_task dxgi ole32 iphlpapi)

# DLL du runtime portÃ©es en Rust (crates/anyxboxx-*, docs/rust-full-status.md) : compilÃ©es par
# cargo, copiÃ©es ici Ã  cÃ´tÃ© des DLL C++. Â« source=nom dans bin Â», sÃ©parÃ©es par des virgules.
set(ANYXBOXX_RUST_RELEASE "${CMAKE_CURRENT_SOURCE_DIR}/../target/release" CACHE PATH
    "Dossier de cargo build --release (les DLL Rust du runtime)")
set(ANYXBOXX_RUST_DLLS "xgameruntime.dll=xgameruntime.dll,xcrdapi.dll=XCRDAPI.dll,kernelx.dll=kernelx.dll,d3d12_xs.dll=d3d12_xs.dll,anyxboxx_tls.dll=anyxboxx_tls.dll,xmem.dll=xmem.dll,anyxboxx-native-launcher.exe=anyxboxx.exe,anyxboxx_gpu_run.exe=anyxboxx_gpu_run.exe")
add_custom_target(rust_dlls ALL
  COMMAND ${CMAKE_COMMAND} -DRUST_RELEASE=${ANYXBOXX_RUST_RELEASE} -DBIN=${CMAKE_BINARY_DIR}/bin
          -DDLLS=${ANYXBOXX_RUST_DLLS} -P ${CMAKE_CURRENT_SOURCE_DIR}/rust_dlls.cmake
  VERBATIM)

# Keep the C++ native launcher as an oracle for parity tests; anyxboxx.exe is copied by rust_dlls.
add_executable(anyxboxx_cpp launcher/launcher.cpp)
target_compile_definitions(anyxboxx_cpp PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX UNICODE _UNICODE)
target_compile_options(anyxboxx_cpp PRIVATE /W4 /WX /permissive- /utf-8)
# Base haute : la base préférée des jeux (0x140000000) reste libre pour eux.
target_link_options(anyxboxx_cpp PRIVATE /BASE:0x7FF700000000 /DYNAMICBASE)

# ExÃ©cution sur le GPU du PC d'un shader traduit RDNA 2 â†’ HLSL â†’ DXIL (emu/rdna2_hlsl.py).
add_executable(anyxboxx_gpu_run_cpp gpu_runner/gpu_run.cpp gpu_runner/adapter.cpp)
target_compile_definitions(anyxboxx_gpu_run_cpp PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX UNICODE _UNICODE)
target_compile_options(anyxboxx_gpu_run_cpp PRIVATE /W4 /WX /permissive- /utf-8)
target_link_libraries(anyxboxx_gpu_run_cpp PRIVATE d3d12 dxgi)

