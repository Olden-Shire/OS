/* Freestanding libc shim for compiling dear imgui to wasm32-unknown-unknown
 * (no sysroot there). Declarations only — mem* resolve to Rust's
 * compiler-builtins, the rest to the no_mangle shims in
 * crates/client/src/wasm_libc.rs. */
#pragma once
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

void  *memset(void *dst, int c, size_t n);
void  *memcpy(void *dst, const void *src, size_t n);
void  *memmove(void *dst, const void *src, size_t n);
int    memcmp(const void *a, const void *b, size_t n);
void  *memchr(const void *s, int c, size_t n);

size_t strlen(const char *s);
int    strcmp(const char *a, const char *b);
int    strncmp(const char *a, const char *b, size_t n);
char  *strcpy(char *dst, const char *src);
char  *strncpy(char *dst, const char *src, size_t n);
char  *strcat(char *dst, const char *src);
char  *strchr(const char *s, int c);
char  *strrchr(const char *s, int c);
char  *strstr(const char *hay, const char *needle);

#ifdef __cplusplus
}
#endif
