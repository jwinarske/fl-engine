# dart-prebuilt

The Dart VM, as `fl-dart-sys` consumes it: one directory per pinned version,
holding what cargo needs to link the VM and nothing cargo would have to build.

```
<version>/
  VERSION                     version=, revision= (sdk git hash), snapshot_hash=
  include/                    the embedding headers, checked in
  lib/<target-triple>/<mode>/ fetched, never committed (mode: jit | aot)
    link.txt                  what to link, in order -- see below
    *.a
  snapshots/                  fetched: platform dill and any VM snapshot blobs
  bin/host/gen_snapshot       fetched
```

The headers are checked in so the bindings in `fl-dart-sys` can be regenerated
and reviewed without fetching anything (`cargo xtask bindgen`). The libraries
are not: they are per target and per mode, and the pipeline that produces them
(`tools/dart-prebuilt/build.sh`, run by `.github/workflows/dart-prebuilt.yml`)
is the source of record. `SHA256SUMS`, checked in beside `VERSION` once a
release is published and reviewed, is what `cargo xtask fetch-prebuilt` trusts.

## `link.txt`

The pipeline writes it, so that the link line is decided where the archives
are built rather than guessed at where they are consumed. One directive per
line, blank lines and `#` comments ignored:

```
static=fl_dart_aot   # lib<name>.a here: VM + dart:io + ICU + zlib + BoringSSL + shim + libc++
system=dl            # a system library
system=pthread
```

`cxx` names a C++ runtime to link dynamically. The pipeline never writes it:
its archives carry the toolchain's static libc++, the way every SDK binary
does, so a board needs no C++ runtime installed. The directive exists for an
artifact built some other way (a distro's shared libstdc++, say), and it is a
line in this file rather than a `cfg` in the build script because it depends
on which toolchain built the archives, not on which target they are for.
