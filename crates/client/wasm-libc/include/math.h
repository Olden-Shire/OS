/* Freestanding libc shim for imgui-on-wasm — see string.h.
 * Implementations are no_mangle wrappers over the pure-Rust `libm` crate
 * (crates/client/src/wasm_libc.rs); sqrt/fabs lower to wasm opcodes. */
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

float  sqrtf(float x);
float  fabsf(float x);
float  cosf(float x);
float  sinf(float x);
float  acosf(float x);
float  atan2f(float y, float x);
float  powf(float x, float y);
float  fmodf(float x, float y);
float  ceilf(float x);
float  floorf(float x);
float  logf(float x);
float  expf(float x);

double sqrt(double x);
double fabs(double x);
double log(double x);
double exp(double x);
double cos(double x);
double sin(double x);
double acos(double x);
double atan2(double y, double x);
double pow(double x, double y);
double fmod(double x, double y);
double ceil(double x);
double floor(double x);

#ifdef __cplusplus
}
#endif
