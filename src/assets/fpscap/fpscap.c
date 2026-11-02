#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>

typedef void (*glx_swap_t)(void *dpy, unsigned long drawable);
typedef int (*egl_swap_t)(void *dpy, void *surface);
typedef int (*sdl_swap_t)(void *window);
typedef void *(*dlsym_t)(void *handle, const char *symbol);

static glx_swap_t real_glx_swap = NULL;
static egl_swap_t real_egl_swap = NULL;
static sdl_swap_t real_sdl_swap = NULL;
static dlsym_t real_dlsym = NULL;

static double g_target_fps = 0.0;
static int64_t g_target_ns = 0;
static int64_t g_next_ns = 0;
static pthread_once_t g_init_once = PTHREAD_ONCE_INIT;

static int64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static void sleep_until_ns(int64_t deadline) {
    struct timespec ts;
    ts.tv_sec = (time_t)(deadline / 1000000000LL);
    ts.tv_nsec = (long)(deadline % 1000000000LL);
    while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &ts, NULL) == EINTR) {
    }
}

static void init_once(void) {
    const char *env = getenv("LYNNCHER_FPS_CAP");
    if (env && *env) {
        long v = strtol(env, NULL, 10);
        if (v > 0) {
            g_target_fps = (double)v;
            g_target_ns = (int64_t)(1000000000.0 / g_target_fps);
            g_next_ns = 0;
        }
    }
    const char *logpath = getenv("LYNNCHER_FPS_LOG");
    if (logpath && *logpath) {
        FILE *f = fopen(logpath, "a");
        if (f) {
            fprintf(f, "[fpscap] armed: LYNNCHER_FPS_CAP=%s target_ns=%lld\n",
                    env ? env : "(unset)", (long long)g_target_ns);
            fclose(f);
        }
    }
}

static void pace_frame(void) {
    pthread_once(&g_init_once, init_once);
    if (g_target_ns <= 0)
        return;

    int64_t t = now_ns();
    if (g_next_ns == 0 || g_next_ns < t - g_target_ns) {
        g_next_ns = t + g_target_ns;
        return;
    }

    if (g_next_ns > t) {
        sleep_until_ns(g_next_ns);
        g_next_ns += g_target_ns;
    } else {
        g_next_ns += g_target_ns;
    }
}

static void *resolve_real(void *handle, const char *symbol) {
    if (!real_dlsym)
        real_dlsym = (dlsym_t)dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.2.5");
    if (!real_dlsym)
        return NULL;
    return real_dlsym(handle, symbol);
}

static void note_active(const char *which) {
    const char *logpath = getenv("LYNNCHER_FPS_LOG");
    if (!logpath || !*logpath)
        return;
    FILE *f = fopen(logpath, "a");
    if (f) {
        fprintf(f, "[fpscap] pacing active via %s\n", which);
        fclose(f);
    }
}

void glXSwapBuffers(void *dpy, unsigned long drawable) {
    static int noted = 0;
    if (!real_glx_swap)
        real_glx_swap = (glx_swap_t)resolve_real(RTLD_NEXT, "glXSwapBuffers");
    if (!noted) {
        noted = 1;
        if (real_glx_swap) note_active("glXSwapBuffers");
        else note_active("glXSwapBuffers (WARNING: real pointer NULL - swap skipped)");
    }
    if (real_glx_swap)
        real_glx_swap(dpy, drawable);
    pace_frame();
}

int eglSwapBuffers(void *dpy, void *surface) {
    static int noted = 0;
    if (!real_egl_swap)
        real_egl_swap = (egl_swap_t)resolve_real(RTLD_NEXT, "eglSwapBuffers");
    if (!noted) {
        noted = 1;
        if (real_egl_swap) note_active("eglSwapBuffers");
        else note_active("eglSwapBuffers (WARNING: real pointer NULL - swap skipped)");
    }
    int r = 0;
    if (real_egl_swap)
        r = real_egl_swap(dpy, surface);
    pace_frame();
    return r;
}

int SDL_GL_SwapWindow(void *window) {
    static int noted = 0;
    if (!real_sdl_swap)
        real_sdl_swap = (sdl_swap_t)resolve_real(RTLD_NEXT, "SDL_GL_SwapWindow");
    if (!noted) {
        noted = 1;
        if (real_sdl_swap) note_active("SDL_GL_SwapWindow");
        else note_active("SDL_GL_SwapWindow (WARNING: real pointer NULL - swap skipped)");
    }
    int r = 0;
    if (real_sdl_swap)
        r = real_sdl_swap(window);
    pace_frame();
    return r;
}

void *dlsym(void *handle, const char *symbol) {
    static int resolving_internally = 0;

    if (!real_dlsym && !resolving_internally) {
        resolving_internally = 1;
        real_dlsym = (dlsym_t)dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.2.5");
        resolving_internally = 0;
    }

    if (symbol && real_dlsym) {
        if (strcmp(symbol, "eglSwapBuffers") == 0 ||
            strcmp(symbol, "eglSwapBuffersWithDamageKHR") == 0) {
            if (!real_egl_swap)
                real_egl_swap = (egl_swap_t)real_dlsym(handle, symbol);
            return (void *)eglSwapBuffers;
        }
        if (strcmp(symbol, "glXSwapBuffers") == 0) {
            if (!real_glx_swap)
                real_glx_swap = (glx_swap_t)real_dlsym(handle, symbol);
            return (void *)glXSwapBuffers;
        }
        if (strcmp(symbol, "SDL_GL_SwapWindow") == 0) {
            if (!real_sdl_swap)
                real_sdl_swap = (sdl_swap_t)real_dlsym(handle, symbol);
            return (void *)SDL_GL_SwapWindow;
        }
    }

    if (!real_dlsym)
        return NULL;
    return real_dlsym(handle, symbol);
}
