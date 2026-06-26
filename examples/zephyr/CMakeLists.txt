# R21 — minimal Zephyr application that links an Axon freestanding object.
#
# The Axon program is compiled separately to an arm-zephyr-eabi object
# (`axon build --freestanding --target zephyr --emit-obj app.ax --out axon_app.o`)
# and linked into this app via AXON_OBJ. The C main() calls the Axon @[entry]
# symbol `axon_main`, and provides the `axon_console_putc` hook the Axon code
# calls for console output (wired to printk).
#
# Build (see scripts/zephyr_qemu_gate.sh for the full flow):
#   west build -b qemu_cortex_m3 examples/zephyr \
#       -- -DAXON_OBJ=/abs/path/to/axon_app.o
#   west build -t run

cmake_minimum_required(VERSION 3.20.0)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(axon_zephyr)

target_sources(app PRIVATE src/main.c)

# Path to the pre-built Axon object (passed via -DAXON_OBJ=...). The object is
# an arm-zephyr-eabi relocatable ELF emitted by the Axon compiler; it exposes
# `axon_main` (the @[entry] fn) and references the `axon_console_putc` hook
# defined in main.c.
if(NOT DEFINED AXON_OBJ)
  message(FATAL_ERROR "AXON_OBJ is not set — pass -DAXON_OBJ=/path/to/axon_app.o")
endif()

# Link the Axon object into the final image. The Axon code has no libc/runtime
# deps beyond the single `axon_console_putc` symbol main.c provides. We import
# the pre-built object as a static library so CMake threads it onto the link
# line and `axon_main` resolves. `--whole-archive` would also work, but an
# imported object linked to `app` is the cleanest and respects --gc-sections
# (axon_main is reached from main(), so it is kept).
target_link_libraries(app PRIVATE "${AXON_OBJ}")
