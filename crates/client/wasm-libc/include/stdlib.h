/* Freestanding libc shim for imgui-on-wasm — see string.h. */
#pragma once
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

void  *malloc(size_t size);
void   free(void *ptr);
void  *realloc(void *ptr, size_t size);

void   qsort(void *base, size_t count, size_t size,
             int (*cmp)(const void *, const void *));

double strtod(const char *s, char **end);
double atof(const char *s);
int    atoi(const char *s);
int    abs(int v);

#ifdef __cplusplus
}
#endif
