/* Freestanding libc shim for imgui-on-wasm — see string.h.
 * imgui is compiled with IMGUI_USE_STB_SPRINTF (printf family handled by
 * the vendored stb_sprintf, fully freestanding) and
 * IMGUI_DISABLE_FILE_FUNCTIONS (no FILE* I/O); what's left is sscanf,
 * which InputScalar references through a data table — stubbed in Rust. */
#pragma once
#include <stddef.h>
#include <stdarg.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FILE FILE;

int sscanf(const char *s, const char *fmt, ...);
int vsnprintf(char *buf, size_t n, const char *fmt, va_list args);
int snprintf(char *buf, size_t n, const char *fmt, ...);
int sprintf(char *buf, const char *fmt, ...);
int printf(const char *fmt, ...);

#ifdef __cplusplus
}
#endif
