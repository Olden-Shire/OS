/* Freestanding libc shim for imgui-on-wasm — see string.h.
 * IM_ASSERT compiles out: a failed assert in the overlay isn't worth
 * carrying abort/format machinery into the wasm binary. */
#pragma once
#define assert(expr) ((void)0)
