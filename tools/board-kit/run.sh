#!/bin/sh
# Exit criteria on a real board (Raspberry Pi 5, VisionFive 2), from the
# board kit tools/board-kit/build.sh makes:
#
#   tar xJf fl-dart-board-kit-<target>.tar.xz && cd fl-dart-board-kit-<target>
#   ./run.sh | tee report.txt
#
# One PASS or FAIL line per check, a summary, exit 0 only if all pass.
# Plain POSIX sh: a board image may have no bash. Under FL_DART_TEST_RUNNER
# (e.g. "qemu-aarch64 -L /usr/aarch64-linux-gnu") it runs off-board too,
# which is how the kit itself is tested; that run is not a board result.
cd "$(dirname "$0")" || exit 1

fails=0
passes=0
pass() { passes=$((passes + 1)); echo "PASS  $1"; }
fail() { fails=$((fails + 1)); echo "FAIL  $1"; [ -n "$2" ] && printf '%s\n' "$2" | sed 's/^/      /'; }

# shellcheck disable=SC2086 # the runner is a command line, split on purpose
fl() { $FL_DART_TEST_RUNNER ./fl-dart "$@"; }

out="$(mktemp)"
err="$(mktemp)"
trap 'rm -f "$out" "$err"' EXIT

# run <program> [args]: stdout to $out, stderr to $err, status in $rc.
# Under a runner, one rerun when qemu-user itself crashed ("QEMU internal
# SIGSEGV": a fault in the emulator's code, not the guest's), as the L2
# harness does (crates/fl-dart-cli/tests/common). A second crash stands, and
# on a board there is no runner and no rerun.
reruns=0
run() {
  fl "$@" > "$out" 2> "$err"; rc=$?
  if [ -n "$FL_DART_TEST_RUNNER" ] && grep -q 'QEMU internal SIGSEGV' "$err"; then
    reruns=$((reruns + 1))
    echo "INFO  qemu crashed in its own code running '$*'; once more"
    fl "$@" > "$out" 2> "$err"; rc=$?
  fi
}

echo "fl-dart board run"
echo "  kit:     $(sed -n 's/^commit=//p' MANIFEST) $(sed -n 's/^target=//p' MANIFEST)"
echo "  host:    $(uname -srm)"
model=""
[ -r /proc/device-tree/model ] && model="$(tr -d '\0' < /proc/device-tree/model)"
[ -n "$model" ] || model="$(sed -n 's/^\(model name\|uarch\|isa\)[[:space:]]*: //p' /proc/cpuinfo | head -n1)"
echo "  board:   ${model:-unknown}"
[ -n "$FL_DART_TEST_RUNNER" ] && echo "  runner:  $FL_DART_TEST_RUNNER (not a board result)"
echo

# Criterion 2: AOT hello world, exit 0.
run run-aot hello.aot
if [ $rc -eq 0 ] && [ "$(cat "$out")" = "Hello World" ]; then
  pass "2  run-aot hello.aot prints Hello World, exits 0"
else
  fail "2  run-aot hello.aot (exit $rc)" "$(cat "$out" "$err")"
fi

run run-aot loop.aot a b
want="$(printf 'main\nmicrotask\nfuture\nargs=a,b\ntimer')"
if [ $rc -eq 0 ] && [ "$(cat "$out")" = "$want" ]; then
  pass "2  run loop drains microtasks, futures, timers"
else
  fail "2  run loop (exit $rc)" "$(cat "$out" "$err")"
fi

# Criterion 3: FFI both directions, and racing posts from many threads.
run run-aot ffi.aot
want="$(printf 'add=42\nlen=11\nport=from rust,7')"
if [ $rc -eq 0 ] && [ "$(cat "$out")" = "$want" ]; then
  pass "3  FFI: scalars and a string to Rust, a port message from a Rust thread"
else
  fail "3  FFI (exit $rc)" "$(cat "$out" "$err")"
fi

run run-aot burst.aot
if [ $rc -eq 0 ] && [ "$(cat "$out")" = "burst=8000 ok" ]; then
  pass "3  8 threads post 1,000 messages each, all arrive in order"
else
  fail "3  racing posts (exit $rc)" "$(cat "$out" "$err")"
fi

# Criterion 4: uncaught exception, Dart stack trace, nonzero exit.
run run-aot throws.aot
if [ $rc -eq 255 ] && grep -q 'unhandled exception: Bad state: thrown on purpose' "$err" \
  && grep -q 'inner' "$err"; then
  pass "4  uncaught exception: exit 255, message and stack trace"
else
  fail "4  uncaught exception (exit $rc)" "$(cat "$err")"
fi

# Criterion 5: snapshot from another SDK, named as a version mismatch.
run run-aot mismatch.aot
if [ $rc -eq 254 ] && grep -q 'snapshot version mismatch' "$err" && grep -q 'ffffffffff' "$err"; then
  pass "5  other SDK's snapshot: exit 254, both versions named"
else
  fail "5  version mismatch (exit $rc)" "$(cat "$err")"
fi

# Criterion 6: isolate lifecycles flat, full VM lifecycles clean. Limits as
# the soak lane (crates/fl-dart-cli/tests/soak.rs).
# One malloc arena, as soak.rs: glibc's default arenas step RSS at random
# under burst's threads. Exported: an assignment before a function call does
# not reliably reach the commands it runs.
export MALLOC_ARENA_MAX=1
for prog in hello burst; do
  run cycle "$prog.aot" 1000
  line="$(cat "$out")"
  val() { printf '%s\n' "$line" | tr ' ' '\n' | sed -n "s/^$1=//p"; }
  tenth="$(val rss_tenth_kb)"; end="$(val rss_end_kb)"
  fdw="$(val fds_warm)"; fde="$(val fds_end)"; ph="$(val persistent_handles)"
  # Under qemu-user, RSS says nothing about fl-dart: qemu 8.2 keeps ~39 kB
  # per guest thread it ever ran (a C loop of empty threads: +311 MB over
  # 8,000), and burst starts 8 threads a cycle. Off-board, the RSS limit is
  # reported, not asserted; descriptors and handles still are. On a board
  # there is no runner, and all three are asserted.
  rss_ok=1
  [ -n "$end" ] && [ $((end - tenth)) -le 512 ] || rss_ok=0
  if [ -n "$FL_DART_TEST_RUNNER" ] && [ $rss_ok -eq 0 ] && [ -n "$end" ]; then
    echo "INFO  6  RSS +$((end - tenth)) kB over 900 cycles of $prog under the runner; not asserted (qemu keeps per-thread memory)"
    rss_ok=1
  fi
  if [ $rc -eq 0 ] && [ $rss_ok -eq 1 ] && [ "$fdw" = "$fde" ] && [ "$ph" = 0 ]; then
    pass "6  1,000 isolate cycles of $prog: RSS +$((end - tenth)) kB, fds $fde, handles 0"
  else
    fail "6  1,000 isolate cycles of $prog (exit $rc)" "$line$(cat "$err")"
  fi
done

unset MALLOC_ARENA_MAX
i=0
bad=""
while [ $i -lt 100 ]; do
  run run-aot hello.aot
  if [ $rc -ne 0 ] || [ "$(cat "$out")" != "Hello World" ]; then bad="run $i: exit $rc $(cat "$err")"; break; fi
  i=$((i + 1))
done
if [ -z "$bad" ]; then
  pass "6  100 processes: full VM init and cleanup, all exit 0"
else
  fail "6  100 processes" "$bad"
fi

echo
echo "$passes passed, $fails failed"
[ $reruns -gt 0 ] && echo "($reruns rerun after a qemu-internal crash)"
[ $fails -eq 0 ]
