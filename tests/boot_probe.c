/*
 * Lichee RV Boot Partition Probe
 *
 * Run this on the official Linux image to identify the boot partition format
 * and U-Boot environment content without relying on external commands.
 *
 * Build:
 *   export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
 *   riscv64-linux-musl-gcc -static -O2 -Wall -Wextra -o tests/boot_probe tests/boot_probe.c
 *
 * Run on board:
 *   ./boot_probe | tee boot-probe.txt
 */

#define _GNU_SOURCE

#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <linux/fs.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>

#ifndef BLKGETSIZE64
#define BLKGETSIZE64 _IOR(0x12, 114, size_t)
#endif

#define READ_SCAN_BYTES (12 * 1024 * 1024)
#define HEXDUMP_BYTES   512
#define STRING_LIMIT    256

static const char *paths[] = {
    "/dev/by-name/boot",
    "/dev/by-name/env",
    "/dev/by-name/env-redund",
    "/dev/by-name/recovery",
    NULL,
};

static void line(char ch)
{
    for (int i = 0; i < 78; i++) {
        putchar(ch);
    }
    putchar('\n');
}

static uint32_t be32(const unsigned char *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | (uint32_t)p[3];
}

static uint32_t le32(const unsigned char *p)
{
    return ((uint32_t)p[3] << 24) | ((uint32_t)p[2] << 16) |
           ((uint32_t)p[1] << 8) | (uint32_t)p[0];
}

static void hexdump(const unsigned char *buf, size_t len, uint64_t base)
{
    for (size_t off = 0; off < len; off += 16) {
        size_t line_len = len - off;
        if (line_len > 16) {
            line_len = 16;
        }
        printf("%08" PRIx64 ":", base + (uint64_t)off);
        for (size_t i = 0; i < line_len; i++) {
            printf(" %02x", buf[off + i]);
        }
        for (size_t i = line_len; i < 16; i++) {
            printf("   ");
        }
        printf("  |");
        for (size_t i = 0; i < line_len; i++) {
            unsigned char c = buf[off + i];
            putchar(isprint(c) ? c : '.');
        }
        printf("|\n");
    }
}

static void print_strings(const unsigned char *buf, size_t len, size_t limit)
{
    size_t printed = 0;
    size_t i = 0;

    while (i < len && printed < limit) {
        while (i < len && !isprint(buf[i])) {
            i++;
        }
        size_t start = i;
        while (i < len && (isprint(buf[i]) || buf[i] == '\t')) {
            i++;
        }
        size_t slen = i - start;
        if (slen >= 4) {
            printf("  +0x%zx: ", start);
            fwrite(buf + start, 1, slen, stdout);
            putchar('\n');
            printed++;
        }
    }
    if (printed == 0) {
        printf("  <no printable strings >= 4 bytes>\n");
    }
}

static int read_prefix(const char *path, unsigned char **out, size_t *out_len)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return -1;
    }

    unsigned char *buf = malloc(READ_SCAN_BYTES);
    if (!buf) {
        close(fd);
        errno = ENOMEM;
        return -1;
    }

    size_t total = 0;
    while (total < READ_SCAN_BYTES) {
        ssize_t n = read(fd, buf + total, READ_SCAN_BYTES - total);
        if (n < 0) {
            free(buf);
            close(fd);
            return -1;
        }
        if (n == 0) {
            break;
        }
        total += (size_t)n;
    }

    close(fd);
    *out = buf;
    *out_len = total;
    return 0;
}

static uint64_t block_size_bytes(const char *path)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return 0;
    }

    uint64_t bytes = 0;
    if (ioctl(fd, BLKGETSIZE64, &bytes) != 0) {
        bytes = 0;
    }
    close(fd);
    return bytes;
}

static void print_symlink_target(const char *path)
{
    char target[512];
    ssize_t n = readlink(path, target, sizeof(target) - 1);
    if (n >= 0) {
        target[n] = '\0';
        printf("symlink: %s -> %s\n", path, target);
    }
}

static void detect_at(const unsigned char *buf, size_t len, size_t off)
{
    const unsigned char *p = buf + off;
    size_t left = len - off;

    if (left >= 8 && memcmp(p, "ANDROID!", 8) == 0) {
        printf("MAGIC @0x%zx: Android boot image\n", off);
        if (left >= 0x30) {
            printf("  kernel_size=%u bytes\n", le32(p + 0x08));
            printf("  kernel_addr=0x%08x\n", le32(p + 0x0c));
            printf("  ramdisk_size=%u bytes\n", le32(p + 0x10));
            printf("  ramdisk_addr=0x%08x\n", le32(p + 0x14));
            printf("  second_size=%u bytes\n", le32(p + 0x18));
            printf("  second_addr=0x%08x\n", le32(p + 0x1c));
            printf("  tags_addr=0x%08x\n", le32(p + 0x20));
            printf("  page_size=%u\n", le32(p + 0x24));
            printf("  os_version=0x%08x\n", le32(p + 0x28));
            printf("  name=%.16s\n", (const char *)(p + 0x30));
        }
    }

    if (left >= 4 && be32(p) == 0x27051956) {
        printf("MAGIC @0x%zx: U-Boot legacy uImage\n", off);
        if (left >= 0x40) {
            printf("  ih_time=%u\n", be32(p + 0x08));
            printf("  ih_size=%u bytes\n", be32(p + 0x0c));
            printf("  ih_load=0x%08x\n", be32(p + 0x10));
            printf("  ih_ep=0x%08x\n", be32(p + 0x14));
            printf("  ih_os=%u ih_arch=%u ih_type=%u ih_comp=%u\n",
                   p[0x1c], p[0x1d], p[0x1e], p[0x1f]);
            printf("  name=%.32s\n", (const char *)(p + 0x20));
        }
    }

    if (left >= 4 && be32(p) == 0xd00dfeed) {
        printf("MAGIC @0x%zx: Flattened Device Tree / FIT candidate\n", off);
        if (left >= 0x28) {
            printf("  totalsize=%u bytes\n", be32(p + 0x04));
            printf("  off_dt_struct=0x%x off_dt_strings=0x%x\n",
                   be32(p + 0x08), be32(p + 0x0c));
        }
    }

    if (left >= 4 && memcmp(p, "\x7f" "ELF", 4) == 0) {
        printf("MAGIC @0x%zx: ELF image\n", off);
    }

    if (left >= 2 && p[0] == 0x1f && p[1] == 0x8b) {
        printf("MAGIC @0x%zx: gzip stream\n", off);
    }
}

static void scan_magics(const unsigned char *buf, size_t len)
{
    printf("-- magic scan --\n");
    int hits = 0;
    for (size_t off = 0; off + 8 <= len; off += 4) {
        int before = hits;
        if (memcmp(buf + off, "ANDROID!", 8) == 0) {
            detect_at(buf, len, off);
            hits++;
        } else if (be32(buf + off) == 0x27051956 ||
                   be32(buf + off) == 0xd00dfeed ||
                   memcmp(buf + off, "\x7f" "ELF", 4) == 0 ||
                   (buf[off] == 0x1f && buf[off + 1] == 0x8b)) {
            detect_at(buf, len, off);
            hits++;
        }
        if (hits > before && hits >= 32) {
            printf("  scan stopped after 32 hits\n");
            break;
        }
    }
    if (hits == 0) {
        printf("  no common boot magic found in first %zu bytes\n", len);
    }
}

static void print_env_like(const unsigned char *buf, size_t len)
{
    printf("-- possible U-Boot env strings --\n");

    if (len < 8) {
        printf("  too small\n");
        return;
    }

    printf("  first_le32_crc=0x%08x first_be32_crc=0x%08x\n",
           le32(buf), be32(buf));

    size_t start_candidates[] = {4, 5, 8, 16, 0};
    for (size_t c = 0; c < sizeof(start_candidates) / sizeof(start_candidates[0]); c++) {
        size_t start = start_candidates[c];
        if (start >= len) {
            continue;
        }
        printf("  strings from offset 0x%zx:\n", start);
        print_strings(buf + start, len - start, 48);
    }
}

static void probe_path(const char *path)
{
    unsigned char *buf = NULL;
    size_t len = 0;

    line('=');
    printf("PATH: %s\n", path);
    print_symlink_target(path);

    uint64_t bsz = block_size_bytes(path);
    if (bsz) {
        printf("block_size_bytes=%" PRIu64 "\n", bsz);
    } else {
        printf("block_size_bytes=<unknown> (%s)\n", strerror(errno));
    }

    if (read_prefix(path, &buf, &len) != 0) {
        printf("ERROR: cannot read: %s\n", strerror(errno));
        return;
    }

    printf("read_prefix_bytes=%zu\n", len);
    printf("-- first %d bytes --\n", HEXDUMP_BYTES);
    hexdump(buf, len < HEXDUMP_BYTES ? len : HEXDUMP_BYTES, 0);
    scan_magics(buf, len);

    if (strstr(path, "env") != NULL) {
        print_env_like(buf, len);
    } else {
        printf("-- printable strings --\n");
        print_strings(buf, len, STRING_LIMIT);
    }

    free(buf);
}

int main(void)
{
    printf("BOOT_PROBE_OUTPUT_BEGIN\n");
    printf("purpose=identify Lichee RV Tina boot/env partition formats\n");

    for (int i = 0; paths[i]; i++) {
        probe_path(paths[i]);
    }

    printf("BOOT_PROBE_OUTPUT_END\n");
    return 0;
}
