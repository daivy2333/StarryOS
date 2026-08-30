#!/bin/sh
# Wrapper `cc` that appends `-no-pie` to executable links and passes
# shared-object (and other non-executable) requests through unchanged.
#
# ArceOS host tests (e.g. `crates/axnet`) link axtask/axplat/percpu rlibs
# that carry non-PIC absolute relocations; default PIE link of the test
# binary aborts with `relocation R_X86_64_32S cannot be used`. Appending
# `-no-pie` to the final executable link resolves it.
#
# Usage:
#   RUSTFLAGS="-C linker=/path/to/scripts/cc-nopie.sh" cargo test ...
if printf ' %s ' "$*" | grep -q -- ' -shared '; then
    exec cc "$@"
else
    exec cc "$@" -no-pie
fi