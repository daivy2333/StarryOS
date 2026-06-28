/*
 * Lichee RV Dock Explorer
 *
 * Collect board bring-up information from official Linux and print it as
 * copyable plain text. This program intentionally avoids shelling out to
 * external commands so it can run on small BusyBox/Tina/Debian systems.
 *
 * Build:
 *   export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
 *   riscv64-linux-musl-gcc -static -O2 -Wall -Wextra -o tests/explorer tests/explorer.c
 *
 * Run on board:
 *   ./explorer | tee explorer-licheerv.txt
 */

#define _GNU_SOURCE

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

#define MAX_FILE_PRINT (128 * 1024)
#define DT_MAX_PROP    256

static void print_line(char ch, int n)
{
    for (int i = 0; i < n; i++) {
        putchar(ch);
    }
    putchar('\n');
}

static void section(const char *name)
{
    putchar('\n');
    print_line('=', 78);
    printf("SECTION: %s\n", name);
    print_line('=', 78);
}

static void subsection(const char *name)
{
    putchar('\n');
    printf("-- %s --\n", name);
}

static int read_file_alloc(const char *path, unsigned char **out, size_t *out_len,
                           size_t max_bytes)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return -1;
    }

    size_t cap = 4096;
    unsigned char *buf = malloc(cap + 1);
    if (!buf) {
        close(fd);
        return -1;
    }

    size_t len = 0;
    while (len < max_bytes) {
        if (len == cap) {
            size_t next = cap * 2;
            if (next > max_bytes) {
                next = max_bytes;
            }
            unsigned char *tmp = realloc(buf, next + 1);
            if (!tmp) {
                free(buf);
                close(fd);
                return -1;
            }
            buf = tmp;
            cap = next;
        }

        ssize_t n = read(fd, buf + len, cap - len);
        if (n < 0) {
            free(buf);
            close(fd);
            return -1;
        }
        if (n == 0) {
            break;
        }
        len += (size_t)n;
    }

    buf[len] = '\0';
    close(fd);
    *out = buf;
    *out_len = len;
    return 0;
}

static int is_mostly_text(const unsigned char *buf, size_t len)
{
    if (len == 0) {
        return 1;
    }

    size_t printable = 0;
    for (size_t i = 0; i < len; i++) {
        unsigned char c = buf[i];
        if (c == '\0' || c == '\n' || c == '\r' || c == '\t' ||
            (c >= 32 && c <= 126)) {
            printable++;
        }
    }
    return printable * 100 / len >= 85;
}

static void print_text_buffer(const unsigned char *buf, size_t len)
{
    for (size_t i = 0; i < len; i++) {
        unsigned char c = buf[i];
        if (c == '\0') {
            putchar('\n');
        } else if (c == '\r' || c == '\n' || c == '\t' ||
                   (c >= 32 && c <= 126)) {
            putchar(c);
        } else {
            putchar('.');
        }
    }
    if (len == 0 || buf[len - 1] != '\n') {
        putchar('\n');
    }
}

static void print_hex_buffer(const unsigned char *buf, size_t len)
{
    for (size_t off = 0; off < len; off += 16) {
        printf("  %04zx:", off);
        size_t line = len - off;
        if (line > 16) {
            line = 16;
        }
        for (size_t i = 0; i < line; i++) {
            printf(" %02x", buf[off + i]);
        }
        for (size_t i = line; i < 16; i++) {
            printf("   ");
        }
        printf("  |");
        for (size_t i = 0; i < line; i++) {
            unsigned char c = buf[off + i];
            putchar(isprint(c) ? c : '.');
        }
        printf("|\n");
    }
}

static void print_file(const char *title, const char *path)
{
    unsigned char *buf = NULL;
    size_t len = 0;

    subsection(title);
    printf("$ cat %s\n", path);

    if (read_file_alloc(path, &buf, &len, MAX_FILE_PRINT) != 0) {
        printf("ERROR: %s\n", strerror(errno));
        return;
    }

    if (is_mostly_text(buf, len)) {
        print_text_buffer(buf, len);
    } else {
        print_hex_buffer(buf, len);
    }
    free(buf);
}

static void print_file_if_exists(const char *title, const char *path)
{
    if (access(path, R_OK) == 0) {
        print_file(title, path);
    }
}

static void print_symlink(const char *path)
{
    char target[PATH_MAX];
    ssize_t n = readlink(path, target, sizeof(target) - 1);
    if (n < 0) {
        return;
    }
    target[n] = '\0';
    printf("%s -> %s\n", path, target);
}

static int path_join(char *out, size_t out_len, const char *a, const char *b)
{
    int n = snprintf(out, out_len, "%s/%s", a, b);
    return n > 0 && (size_t)n < out_len;
}

static void list_dir_names(const char *title, const char *path)
{
    DIR *dir = opendir(path);
    subsection(title);
    printf("$ ls %s\n", path);
    if (!dir) {
        printf("ERROR: %s\n", strerror(errno));
        return;
    }

    struct dirent *de;
    while ((de = readdir(dir)) != NULL) {
        if (strcmp(de->d_name, ".") == 0 || strcmp(de->d_name, "..") == 0) {
            continue;
        }
        printf("%s\n", de->d_name);
    }
    closedir(dir);
}

static void print_uname(void)
{
    struct utsname u;
    section("basic system");
    if (uname(&u) == 0) {
        printf("sysname=%s\n", u.sysname);
        printf("nodename=%s\n", u.nodename);
        printf("release=%s\n", u.release);
        printf("version=%s\n", u.version);
        printf("machine=%s\n", u.machine);
    }

    time_t now = time(NULL);
    printf("time=%s", ctime(&now));
}

static void print_statvfs_one(const char *path)
{
    struct statvfs st;
    if (statvfs(path, &st) != 0) {
        printf("%s: ERROR: %s\n", path, strerror(errno));
        return;
    }

    unsigned long long block = st.f_frsize ? st.f_frsize : st.f_bsize;
    unsigned long long total = (unsigned long long)st.f_blocks * block;
    unsigned long long free_b = (unsigned long long)st.f_bfree * block;
    unsigned long long avail = (unsigned long long)st.f_bavail * block;
    printf("%s: total=%llu free=%llu avail=%llu block=%llu\n",
           path, total, free_b, avail, block);
}

static void print_filesystem_summary(void)
{
    section("filesystem summary");
    print_statvfs_one("/");
    print_statvfs_one("/tmp");
    print_file_if_exists("mounts", "/proc/mounts");
    print_file_if_exists("filesystems", "/proc/filesystems");
    list_dir_names("block devices", "/sys/block");
}

static void print_proc_summary(void)
{
    section("procfs platform files");
    print_file_if_exists("cmdline", "/proc/cmdline");
    print_file_if_exists("cpuinfo", "/proc/cpuinfo");
    print_file_if_exists("meminfo", "/proc/meminfo");
    print_file_if_exists("iomem", "/proc/iomem");
    print_file_if_exists("interrupts", "/proc/interrupts");
    print_file_if_exists("irq stat", "/proc/stat");
    print_file_if_exists("devices", "/proc/devices");
    print_file_if_exists("tty drivers", "/proc/tty/drivers");
    print_file_if_exists("consoles", "/proc/consoles");
    print_file_if_exists("version", "/proc/version");
}

static void print_tty_summary(void)
{
    section("tty and console sysfs");
    print_file_if_exists("active console", "/sys/class/tty/console/active");

    DIR *dir = opendir("/sys/class/tty");
    subsection("tty symlinks");
    if (!dir) {
        printf("ERROR: %s\n", strerror(errno));
        return;
    }

    struct dirent *de;
    while ((de = readdir(dir)) != NULL) {
        if (strncmp(de->d_name, "ttyS", 4) != 0 &&
            strncmp(de->d_name, "ttyAS", 5) != 0 &&
            strncmp(de->d_name, "ttyAMA", 6) != 0 &&
            strcmp(de->d_name, "console") != 0) {
            continue;
        }

        char path[PATH_MAX];
        char dev[PATH_MAX];
        if (!path_join(path, sizeof(path), "/sys/class/tty", de->d_name)) {
            continue;
        }
        printf("[%s]\n", de->d_name);
        print_symlink(path);

        if (path_join(dev, sizeof(dev), path, "device")) {
            print_symlink(dev);
        }
        if (path_join(dev, sizeof(dev), path, "dev")) {
            print_file_if_exists("tty dev", dev);
        }
    }
    closedir(dir);
}

static uint32_t be32(const unsigned char *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | (uint32_t)p[3];
}

static int dt_prop_is_string(const char *prop)
{
    return strcmp(prop, "name") == 0 ||
           strcmp(prop, "compatible") == 0 ||
           strcmp(prop, "device_type") == 0 ||
           strcmp(prop, "model") == 0 ||
           strcmp(prop, "status") == 0 ||
           strcmp(prop, "bootargs") == 0 ||
           strcmp(prop, "stdout-path") == 0;
}

static void print_dt_string_buffer(const unsigned char *buf, size_t len)
{
    for (size_t i = 0; i < len; i++) {
        unsigned char c = buf[i];
        if (c == '\0') {
            if (i + 1 < len) {
                printf(", ");
            }
        } else if (c == '\r' || c == '\n' || c == '\t' ||
                   (c >= 32 && c <= 126)) {
            putchar(c);
        } else {
            putchar('.');
        }
    }
    putchar('\n');
}

static void print_dt_prop(const char *node, const char *prop)
{
    char path[PATH_MAX];
    unsigned char *buf = NULL;
    size_t len = 0;

    if (!path_join(path, sizeof(path), node, prop)) {
        return;
    }
    if (read_file_alloc(path, &buf, &len, DT_MAX_PROP) != 0) {
        return;
    }

    printf("  %s: ", prop);
    if (len == 0) {
        printf("<empty>\n");
    } else if (dt_prop_is_string(prop)) {
        print_dt_string_buffer(buf, len);
    } else if (len % 4 == 0) {
        printf("<");
        for (size_t i = 0; i < len; i += 4) {
            printf("0x%08x", be32(buf + i));
            if (i + 4 < len) {
                printf(" ");
            }
        }
        printf(">\n");
    } else {
        printf("\n");
        print_hex_buffer(buf, len);
    }
    free(buf);
}

static int buffer_contains_word(const unsigned char *buf, size_t len,
                                const char *needle)
{
    size_t nlen = strlen(needle);
    if (nlen == 0 || len < nlen) {
        return 0;
    }
    for (size_t i = 0; i + nlen <= len; i++) {
        size_t j = 0;
        for (; j < nlen; j++) {
            unsigned char a = (unsigned char)tolower(buf[i + j]);
            unsigned char b = (unsigned char)tolower((unsigned char)needle[j]);
            if (a != b) {
                break;
            }
        }
        if (j == nlen) {
            return 1;
        }
    }
    return 0;
}

static int dt_node_interesting(const char *path)
{
    const char *props[] = {
        "compatible", "name", "device_type", NULL,
    };

    for (int i = 0; props[i]; i++) {
        char prop_path[PATH_MAX];
        unsigned char *buf = NULL;
        size_t len = 0;

        if (!path_join(prop_path, sizeof(prop_path), path, props[i])) {
            continue;
        }
        if (read_file_alloc(prop_path, &buf, &len, DT_MAX_PROP) != 0) {
            continue;
        }

        int hit = buffer_contains_word(buf, len, "uart") ||
                  buffer_contains_word(buf, len, "serial") ||
                  buffer_contains_word(buf, len, "ns16550") ||
                  buffer_contains_word(buf, len, "plic") ||
                  buffer_contains_word(buf, len, "interrupt-controller") ||
                  buffer_contains_word(buf, len, "clint") ||
                  buffer_contains_word(buf, len, "timer") ||
                  buffer_contains_word(buf, len, "memory") ||
                  buffer_contains_word(buf, len, "chosen") ||
                  buffer_contains_word(buf, len, "cpus");
        free(buf);
        if (hit) {
            return 1;
        }
    }

    if (strstr(path, "/chosen") || strstr(path, "/cpus") ||
        strstr(path, "/memory") || strstr(path, "serial") ||
        strstr(path, "uart") || strstr(path, "plic") ||
        strstr(path, "clint") || strstr(path, "timer")) {
        return 1;
    }

    return 0;
}

static void scan_dt_nodes(const char *path, int depth, int max_depth)
{
    if (depth > max_depth) {
        return;
    }

    if (dt_node_interesting(path)) {
        printf("\nNODE %s\n", path);
        print_dt_prop(path, "name");
        print_dt_prop(path, "compatible");
        print_dt_prop(path, "device_type");
        print_dt_prop(path, "model");
        print_dt_prop(path, "status");
        print_dt_prop(path, "bootargs");
        print_dt_prop(path, "stdout-path");
        print_dt_prop(path, "reg");
        print_dt_prop(path, "interrupts");
        print_dt_prop(path, "interrupt-parent");
        print_dt_prop(path, "clock-frequency");
        print_dt_prop(path, "current-speed");
        print_dt_prop(path, "timebase-frequency");
        print_dt_prop(path, "#address-cells");
        print_dt_prop(path, "#size-cells");
    }

    DIR *dir = opendir(path);
    if (!dir) {
        return;
    }

    struct dirent *de;
    while ((de = readdir(dir)) != NULL) {
        if (strcmp(de->d_name, ".") == 0 || strcmp(de->d_name, "..") == 0) {
            continue;
        }

        char child[PATH_MAX];
        struct stat st;
        if (!path_join(child, sizeof(child), path, de->d_name)) {
            continue;
        }
        if (stat(child, &st) != 0 || !S_ISDIR(st.st_mode)) {
            continue;
        }
        scan_dt_nodes(child, depth + 1, max_depth);
    }
    closedir(dir);
}

static void print_devicetree_summary(void)
{
    section("device tree summary");
    const char *base = "/sys/firmware/devicetree/base";
    if (access(base, R_OK) != 0) {
        printf("No readable %s: %s\n", base, strerror(errno));
        return;
    }

    print_file_if_exists("dt model", "/sys/firmware/devicetree/base/model");
    print_file_if_exists("dt compatible", "/sys/firmware/devicetree/base/compatible");
    scan_dt_nodes(base, 0, 8);
}

static void print_platform_sysfs(void)
{
    section("platform bus sysfs");
    list_dir_names("platform devices", "/sys/bus/platform/devices");
    list_dir_names("interrupt controllers", "/sys/class/irq");
    list_dir_names("clock class", "/sys/class/clk");
    list_dir_names("firmware", "/sys/firmware");
}

static void print_usb_network_summary(void)
{
    section("usb and network");
    list_dir_names("usb devices", "/sys/bus/usb/devices");
    list_dir_names("net devices", "/sys/class/net");
}

static void print_footer(void)
{
    section("copy marker");
    printf("EXPLORER_OUTPUT_END\n");
}

int main(void)
{
    printf("EXPLORER_OUTPUT_BEGIN\n");
    printf("program=tests/explorer.c\n");
    printf("purpose=Lichee RV Dock official Linux board information capture\n");

    print_uname();
    print_proc_summary();
    print_filesystem_summary();
    print_tty_summary();
    print_devicetree_summary();
    print_platform_sysfs();
    print_usb_network_summary();
    print_footer();

    return 0;
}
