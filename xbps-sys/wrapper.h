/*
 * bindgen entry point. We deliberately do NOT hand-declare any xbps
 * types or function signatures ourselves — struct xbps_handle in
 * particular is used by value (not just behind an opaque pointer) in
 * package_store.rs, so its exact field layout/size must come from the
 * real header on the build machine, not a guess baked into this repo.
 * This mirrors how the original C project depended on `libxbps-devel`
 * at build time via meson's dependency('libxbps', ...).
 */
#include <xbps.h>
