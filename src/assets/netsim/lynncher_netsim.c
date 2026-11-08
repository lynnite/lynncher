
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <time.h>

typedef ssize_t (*sendto_t)(int, const void *, size_t, int,
                            const struct sockaddr *, socklen_t);
typedef ssize_t (*recvfrom_t)(int, void *, size_t, int, struct sockaddr *,
                              socklen_t *);

static sendto_t real_sendto = NULL;
static recvfrom_t real_recvfrom = NULL;

static int64_t g_base_ns = 0;
static int64_t g_jitter_ns = 0;
static double g_loss = 0.0;
static double g_dupes = 0.0;
static int g_enabled = 0;
static pthread_once_t g_init_once = PTHREAD_ONCE_INIT;
static __thread unsigned int g_rng_state = 0;


static int64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static unsigned int rng_next(void) {
    unsigned int x = g_rng_state;
    if (x == 0) {
        x = (unsigned int)(now_ns() ^ (intptr_t)&x) | 1u;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    g_rng_state = x;
    return x;
}

static double rng_uniform01(void) {
    return (rng_next() >> 8) * (1.0 / 16777216.0);
}

static void apply_delay(void) {
    int64_t d = g_base_ns;
    if (g_jitter_ns > 0)
        d += (int64_t)(rng_uniform01() * (double)g_jitter_ns);
    if (d <= 0)
        return;
    struct timespec ts;
    ts.tv_sec = (time_t)(d / 1000000000LL);
    ts.tv_nsec = (long)(d % 1000000000LL);
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR)
        ;
}


static void init_once(void) {
    const char *env_ms = getenv("LYNNCHER_NETSIM_MS");
    const char *env_jit = getenv("LYNNCHER_NETSIM_JITTER_MS");
    const char *env_loss = getenv("LYNNCHER_NETSIM_LOSS");
    const char *env_dupes = getenv("LYNNCHER_NETSIM_DUPES");

    long ms = env_ms ? strtol(env_ms, NULL, 10) : 0;
    long jit = env_jit ? strtol(env_jit, NULL, 10) : 0;
    double loss = env_loss ? strtod(env_loss, NULL) : 0.0;
    double dupes = env_dupes ? strtod(env_dupes, NULL) : 0.0;

    if (ms < 0)
        ms = 0;
    if (jit < 0)
        jit = 0;
    if (loss < 0)
        loss = 0;
    if (loss > 100)
        loss = 100;
    if (dupes < 0)
        dupes = 0;
    if (dupes > 100)
        dupes = 100;

    g_base_ns = (int64_t)ms * 1000000LL;
    g_jitter_ns = (int64_t)jit * 1000000LL;
    g_loss = loss / 100.0;
    g_dupes = dupes / 100.0;
    g_enabled = (g_base_ns > 0 || g_jitter_ns > 0 || g_loss > 0.0 ||
                 g_dupes > 0.0)
                    ? 1
                    : 0;

    const char *logpath = getenv("LYNNCHER_NETSIM_LOG");
    if (logpath && *logpath) {
        FILE *f = fopen(logpath, "a");
        if (f) {
            fprintf(f,
                    "[netsim] armed: enabled=%d base_ns=%lld jitter_ns=%lld "
                    "loss=%.3f dupes=%.3f\n",
                    g_enabled, (long long)g_base_ns, (long long)g_jitter_ns,
                    g_loss, g_dupes);
            fclose(f);
        }
    }
}

static void ensure_init(void) {
    pthread_once(&g_init_once, init_once);
}


ssize_t sendto(int fd, const void *buf, size_t len, int flags,
               const struct sockaddr *dest, socklen_t addrlen) {
    if (!real_sendto)
        real_sendto = (sendto_t)dlsym(RTLD_NEXT, "sendto");
    ensure_init();

    if (!g_enabled || !real_sendto || dest == NULL) {
        if (real_sendto)
            return real_sendto(fd, buf, len, flags, dest, addrlen);
        errno = ENOSYS;
        return -1;
    }

    if (g_loss > 0.0 && rng_uniform01() < g_loss) {
        return (ssize_t)len;
    }

    apply_delay();

    ssize_t r = real_sendto(fd, buf, len, flags, dest, addrlen);

    if (g_dupes > 0.0 && rng_uniform01() < g_dupes) {
        apply_delay();
        (void)real_sendto(fd, buf, len, flags, dest, addrlen);
    }

    return r;
}


ssize_t recvfrom(int fd, void *buf, size_t len, int flags, struct sockaddr *src,
                 socklen_t *addrlen) {
    if (!real_recvfrom)
        real_recvfrom = (recvfrom_t)dlsym(RTLD_NEXT, "recvfrom");
    ensure_init();

    if (!g_enabled || !real_recvfrom) {
        if (real_recvfrom)
            return real_recvfrom(fd, buf, len, flags, src, addrlen);
        errno = ENOSYS;
        return -1;
    }

    char probe;
    ssize_t has = real_recvfrom(fd, &probe, 1, MSG_PEEK | MSG_DONTWAIT, NULL,
                                NULL);
    if (has < 0) {
        return -1;
    }

    apply_delay();

    ssize_t r = real_recvfrom(fd, buf, len, flags, src, addrlen);

    if (r >= 0 && g_loss > 0.0 && rng_uniform01() < g_loss) {
        errno = EAGAIN;
        return -1;
    }

    return r;
}
