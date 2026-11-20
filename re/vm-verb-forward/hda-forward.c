/*
 * hda-forward: an HD Audio codec that forwards every verb to a real codec.
 *
 * Each verb the guest sends through the emulated Intel HDA controller goes over a TCP connection
 * to a helper that executes it on real hardware (Linux /dev/snd/hwC*D*, HDA_IOCTL_VERB_WRITE) and
 * returns the real response. Used to let a Windows guest's codec driver initialise a physical
 * codec so the sequence can be recorded.
 *
 *   -device intel-hda -device hda-forward,host=127.0.0.1,port=7171
 *
 * Wire format (little endian): request {u32 nid, u32 data(20 bit)}, reply {u32 response}.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include "qemu/osdep.h"
#include "qemu/sockets.h"
#include <netdb.h>
#include "qemu/module.h"
#include "qemu/error-report.h"
#include "qapi/error.h"
#include "hw/qdev-properties.h"
#include "intel-hda.h"
#include "qom/object.h"
#include "qemu/timer.h"
#include <netinet/tcp.h>

#define TYPE_HDA_FORWARD "hda-forward"
OBJECT_DECLARE_SIMPLE_TYPE(HDAForwardState, HDA_FORWARD)

struct HDAForwardState {
    HDACodecDevice hda;
    char *host;
    uint32_t port;
    int fd;
    uint64_t count;
    uint32_t fmt;            /* last converter format the guest programmed */
    struct {
        QEMUTimer *t;
        uint32_t stnr;
        bool output;
        int64_t start;
        uint64_t done;       /* bytes consumed since start */
    } st[16];
};

static uint32_t fmt_bytes_per_sec(uint32_t fmt)
{
    static const int bytes[8] = { 1, 2, 4, 4, 4, 4, 4, 4 };
    uint32_t rate = (fmt & 0x4000) ? 44100 : 48000;
    rate = rate * (((fmt >> 11) & 7) + 1) / (((fmt >> 8) & 7) + 1);
    return rate * bytes[(fmt >> 4) & 7] * ((fmt & 0xf) + 1);
}

/* consume (and drop) stream data at the real rate so the guest's DMA position advances */
static void hda_forward_tick(void *opaque)
{
    HDAForwardState *s = opaque;
    int64_t now = qemu_clock_get_ns(QEMU_CLOCK_VIRTUAL);
    uint8_t buf[4096];

    for (int i = 0; i < 16; i++) {
        if (!s->st[i].t || !s->st[i].start) {
            continue;
        }
        uint64_t want = (uint64_t)(now - s->st[i].start) * fmt_bytes_per_sec(s->fmt) / NANOSECONDS_PER_SECOND;
        while (s->st[i].done < want) {
            uint32_t n = MIN(sizeof(buf), want - s->st[i].done);
            if (!hda_codec_xfer(&s->hda, s->st[i].stnr, s->st[i].output, buf, n)) {
                break;
            }
            s->st[i].done += n;
        }
        timer_mod(s->st[i].t, now + 10 * SCALE_MS);
    }
}

static bool fwd_io(int fd, void *buf, size_t len, bool send)
{
    uint8_t *p = buf;
    while (len) {
        ssize_t n = send ? write(fd, p, len) : read(fd, p, len);
        if (n < 0 && errno == EINTR) {
            continue;
        }
        if (n <= 0) {
            return false;
        }
        p += n;
        len -= n;
    }
    return true;
}

static void hda_forward_command(HDACodecDevice *hda, uint32_t nid, uint32_t data)
{
    HDAForwardState *s = HDA_FORWARD(hda);
    uint32_t req[2] = { cpu_to_le32(nid), cpu_to_le32(data) };
    uint32_t resp = 0;

    if (s->fd < 0 || !fwd_io(s->fd, req, sizeof(req), true) ||
        !fwd_io(s->fd, &resp, sizeof(resp), false)) {
        if (s->fd >= 0) {
            error_report("hda-forward: connection lost after %" PRIu64 " verbs", s->count);
            close(s->fd);
            s->fd = -1;
        }
        hda_codec_response(hda, true, 0);
        return;
    }
    s->count++;
    if ((nid == 2 || nid == 3) && (data >> 16) == 0x2) {
        s->fmt = data & 0xffff;              /* SET_CONVERTER_FORMAT */
    }
    hda_codec_response(hda, true, le32_to_cpu(resp));
}

static void hda_forward_stream(HDACodecDevice *hda, uint32_t stnr, bool running, bool output)
{
    HDAForwardState *s = HDA_FORWARD(hda);
    uint32_t i = stnr & 15;

    if (!s->st[i].t) {
        s->st[i].t = timer_new_ns(QEMU_CLOCK_VIRTUAL, hda_forward_tick, s);
    }
    s->st[i].stnr = stnr;
    s->st[i].output = output;
    if (running) {
        s->st[i].start = qemu_clock_get_ns(QEMU_CLOCK_VIRTUAL);
        s->st[i].done = 0;
        timer_mod(s->st[i].t, s->st[i].start + 10 * SCALE_MS);
    } else {
        s->st[i].start = 0;
        timer_del(s->st[i].t);
    }
}

static void hda_forward_init(HDACodecDevice *hda, Error **errp)
{
    HDAForwardState *s = HDA_FORWARD(hda);
    char port[16];
    int one = 1;

    struct addrinfo hints = { .ai_family = AF_UNSPEC, .ai_socktype = SOCK_STREAM }, *ai, *a;

    s->fmt = 0x0011;                         /* 48 kHz 16-bit stereo until told otherwise */
    snprintf(port, sizeof(port), "%u", s->port);
    s->fd = -1;
    if (getaddrinfo(s->host ? s->host : "127.0.0.1", port, &hints, &ai) != 0) {
        error_setg(errp, "hda-forward: cannot resolve %s", s->host);
        return;
    }
    for (a = ai; a && s->fd < 0; a = a->ai_next) {
        s->fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (s->fd >= 0 && connect(s->fd, a->ai_addr, a->ai_addrlen) < 0) {
            close(s->fd);
            s->fd = -1;
        }
    }
    freeaddrinfo(ai);
    if (s->fd < 0) {
        error_setg(errp, "hda-forward: cannot connect to %s:%u", s->host, s->port);
        return;
    }
    setsockopt(s->fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
    info_report("hda-forward: connected to %s:%u", s->host, s->port);
}

static void hda_forward_exit(HDACodecDevice *hda)
{
    HDAForwardState *s = HDA_FORWARD(hda);
    if (s->fd >= 0) {
        close(s->fd);
    }
}

static const Property hda_forward_properties[] = {
    DEFINE_PROP_STRING("host", HDAForwardState, host),
    DEFINE_PROP_UINT32("port", HDAForwardState, port, 7171),
};

static void hda_forward_class_init(ObjectClass *klass, void *data)
{
    DeviceClass *dc = DEVICE_CLASS(klass);
    HDACodecDeviceClass *k = HDA_CODEC_DEVICE_CLASS(klass);

    k->init = hda_forward_init;
    k->exit = hda_forward_exit;
    k->command = hda_forward_command;
    k->stream = hda_forward_stream;
    set_bit(DEVICE_CATEGORY_SOUND, dc->categories);
    dc->desc = "HDA codec forwarding verbs to a real codec";
    device_class_set_props(dc, hda_forward_properties);
}

static const TypeInfo hda_forward_info = {
    .name          = TYPE_HDA_FORWARD,
    .parent        = TYPE_HDA_CODEC_DEVICE,
    .instance_size = sizeof(HDAForwardState),
    .class_init    = hda_forward_class_init,
};

static void hda_forward_register_types(void)
{
    type_register_static(&hda_forward_info);
}

type_init(hda_forward_register_types)
