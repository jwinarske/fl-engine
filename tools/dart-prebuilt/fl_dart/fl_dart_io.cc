// See fl_dart_io.h.
//
// Copyright 2026 The fl-engine Authors. BSD-3-Clause.

#include "fl_dart_io.h"

#include "include/bin/dart_io_api.h"

#if defined(DART_IO_SECURE_SOCKET_DISABLED)
#include "bin/io_service_no_ssl.h"
#else
#include "bin/io_service.h"
#endif

extern "C" {

void fl_dart_io_bootstrap(void) {
  dart::bin::BootstrapDartIo();
}

void fl_dart_io_cleanup(void) {
  dart::bin::CleanupDartIo();
}

void fl_dart_io_set_max_concurrency(intptr_t max_concurrency) {
  dart::bin::IOService::set_max_concurrency(max_concurrency < 1 ? 1
                                                                : max_concurrency);
}

intptr_t fl_dart_io_max_concurrency(void) {
  return dart::bin::IOService::max_concurrency();
}

Dart_Handle fl_dart_io_setup_isolate(const char* script_uri,
                                     bool disable_exit) {
  // The same settings Flutter's DartIO::InitForIsolate passes, minus the two
  // closures that come from dart:ui (HTTP policy, locale), which M1 has not.
  dart::bin::DartIoSettings settings;
  settings.script_uri = script_uri;
  settings.disable_exit = disable_exit;
  // Off: it registers a VM-service extension, and the service is not part of
  // M1 (debugging arrives at M9). Left at its default true, the JIT runtime
  // fails the first isolate's setup with "To access
  // 'dart:io::_NetworkProfiling' from native code, it must be annotated".
  settings.enable_network_profiling = false;
  return dart::bin::SetupDartIoLibrary(settings);
}

}  // extern "C"
