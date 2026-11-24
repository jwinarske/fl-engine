// A C ABI over dart:io's embedding API (runtime/include/bin/dart_io_api.h),
// which is C++: a namespace, and a settings struct with default member
// initializers. Rust binds this header; the pipeline compiles the .cc beside
// the VM, so cargo never sees C++.
//
// Copyright 2026 The fl-engine Authors. BSD-3-Clause.

#ifndef FL_DART_IO_H_
#define FL_DART_IO_H_

#include <stdbool.h>
#include <stdint.h>

#include "dart_api.h"

#ifdef __cplusplus
extern "C" {
#endif

// Process-wide: starts dart:io's event handler thread (which is what makes
// Timer work) and its process/socket machinery. Once, before Dart_Initialize.
DART_EXPORT void fl_dart_io_bootstrap(void);

// Process-wide: stops what bootstrap started. Once, after Dart_Cleanup.
DART_EXPORT void fl_dart_io_cleanup(void);

// Process-wide: how many of dart:io's blocking operations (files,
// directories, DNS lookups, process launches) may run at once, each on a
// VM worker thread. The SDK's default is 32. Takes effect only if called
// before the first such operation, which creates the port that reads it, so
// call it before Dart_Initialize. Values below 1 are taken as 1.
DART_EXPORT void fl_dart_io_set_max_concurrency(intptr_t max_concurrency);

// The value fl_dart_io_set_max_concurrency last set, or the SDK's default.
DART_EXPORT intptr_t fl_dart_io_max_concurrency(void);

// Per isolate: resolves dart:io's natives and runs its _setupHooks (timers,
// event handler). Requires a current isolate with an open scope, after the
// program is loaded. `script_uri` may be NULL. Returns an error handle on
// failure, as every Dart_Handle-returning call does.
DART_EXPORT Dart_Handle fl_dart_io_setup_isolate(const char* script_uri,
                                                 bool disable_exit);

#ifdef __cplusplus
}
#endif

#endif  // FL_DART_IO_H_
