/* Freestanding libc shim for imgui-on-wasm — see string.h. */
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

int toupper(int c);
int tolower(int c);
int isspace(int c);
int isdigit(int c);
int isalnum(int c);
int isprint(int c);

#ifdef __cplusplus
}
#endif
