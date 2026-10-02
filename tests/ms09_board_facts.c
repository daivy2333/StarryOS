/* MS09 board facts collector — read-only Linux userspace probe.
 *
 * Iteration 000 (tasks 1.1/1.2): reports the running FDT model/compatible/
 * chosen, memory/reserved-memory, Linux-visible CPUs, UART and MAC
 * compatible/reg/status, MDIO/PHY nodes, phandle-resolved clock/reset/
 * pinctrl references, and netdev driver binding/carrier state.
 *
 * Read-only by contract: no /dev/mem, no MMIO, no sysfs/DT/network writes,
 * no shell pipelines, no extra dependencies.  Secrets (board serial, NIC
 * MAC, rng-seed, bootargs beyond console=) are never printed; only the
 * whitelisted property set below is emitted, so unknown fields fail closed.
 *
 * Output grammar (one fact per line, human-readable for Iteration 001
 * cross-checking; no run-identity fields):
 *   MS09_FACTS_BEGIN / MS09_FACTS_END
 *   MS09_FACTS_ROOT: source=<sysfs|procfs> root=<dir>
 *   MS09_FACTS_MODEL: <text>
 *   MS09_FACTS_COMPATIBLE: <string-list>
 *   MS09_FACTS_CHOSEN: stdout-path=<...> console=<console= tokens only>
 *   MS09_FACTS_MEMORY: node=<name> addr=<hex> size=<hex>
 *   MS09_FACTS_RESERVED: node=<name> addr=<hex> size=<hex>
 *   MS09_FACTS_CPU: node=<name> reg=<u32> status=<...>
 *   MS09_FACTS_CPU_SYS: online=... possible=... present=...
 *   MS09_FACTS_UART: path=... compatible=... reg=.../... status=... irq=<raw>
 *                    clocks=... resets=... pinctrl=...
 *   MS09_FACTS_MAC: path=... compatible=... reg=.../... status=...
 *                   phy-mode=... max-speed=...
 *   MS09_FACTS_PHY: path=... reg=<addr> compatible=...
 *   MS09_FACTS_NET: iface=... of_node=<dt path> driver=... carrier=<0|1|...>
 *   MS09_FACTS_FIRMWARE_ONLY: item=... note=需启动阶段核对
 *   MS09_FACTS_ERROR: item=... result=<missing|perm|ioerr:<strerror>>
 */
#define _DEFAULT_SOURCE
#include <dirent.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define MS09_MAX_DEPTH 32
#define MS09_MAX_NODES 4096
#define MS09_MAX_PHANDLES 512
#define MS09_MAX_MAC_NODES 16
#define MS09_PROP_CAP (256u * 1024u)
#define MS09_PATH_CAP 512u
#define MS09_READ_CAP 4096u
#define MS09_REF_OUT_CAP 4096u

enum fact_error {
    FACT_OK = 0,
    FACT_MISSING,
    FACT_PERM,
    FACT_IOERR,
};

/* --- pure decoding helpers (host-tested) ------------------------------- */

uint32_t fact_be32(const uint8_t *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | (uint32_t)p[3];
}

size_t fact_decode_string_list(const uint8_t *buf, size_t len,
                               const char **out, size_t max_out)
{
    size_t n = 0;
    size_t i = 0;
    while (i < len && n < max_out) {
        out[n++] = (const char *)buf + i;
        while (i < len && buf[i] != '\0')
            i++;
        if (i < len)
            i++;
    }
    return n;
}

int fact_decode_reg(const uint8_t *buf, size_t len,
                    unsigned int ac, unsigned int sc,
                    uint64_t *addr, uint64_t *size, size_t max_pairs)
{
    size_t cells = len / 4;
    size_t per_pair = (size_t)ac + sc;
    size_t i = 0;
    size_t pairs = 0;
    if (ac == 0 || sc == 0 || (len % 4) != 0 || cells < per_pair)
        return -1;
    while (i + per_pair <= cells && pairs < max_pairs) {
        uint64_t a = 0;
        uint64_t s = 0;
        unsigned int k;
        for (k = 0; k < ac; k++)
            a = (a << 32) | fact_be32(buf + 4 * (i + k));
        for (k = 0; k < sc; k++)
            s = (s << 32) | fact_be32(buf + 4 * (i + ac + k));
        addr[pairs] = a;
        size[pairs] = s;
        pairs++;
        i += per_pair;
    }
    return (int)pairs;
}

enum fact_error fact_classify_errno(int err)
{
    if (err == 0)
        return FACT_OK;
    if (err == ENOENT)
        return FACT_MISSING;
    if (err == EACCES || err == EPERM)
        return FACT_PERM;
    return FACT_IOERR;
}

const char *fact_phandle_lookup(const uint32_t *handles, const char *const *paths,
                                size_t count, uint32_t handle)
{
    size_t i;
    for (i = 0; i < count; i++) {
        if (handles[i] == handle)
            return paths[i];
    }
    return NULL;
}

int fact_path_join(char *out, size_t out_len, const char *parent, const char *name)
{
    size_t plen = strlen(parent);
    size_t nlen = strlen(name);
    size_t need = plen + (plen > 1 ? 1 : 0) + nlen + 1;
    if (out_len < need)
        return -1;
    memcpy(out, parent, plen);
    if (plen > 1)
        out[plen++] = '/';
    memcpy(out + plen, name, nlen + 1);
    return (int)(plen + nlen);
}

/* Format a phandle reference property (clocks/resets/pinctrl-0 cell
 * sequence) into `out` (NUL-terminated).  Cell grammar: leading
 * (phandle,arg) pairs, with one optional trailing bare phandle cell
 * (e.g. pinctrl-0 = <&pinctrl>).  Resolved handles render as <path>
 * (pairs: <path>#<arg>); unresolved ones keep the raw form
 * phandle=<hex> (pairs: phandle=<hex>#<arg>).  An existing property is
 * never reported missing: zero cells render as "<empty>", a length
 * that is not a whole number of cells as "<malformed>".
 *
 * `lookup` resolves a handle to a DT path or NULL.  Returns 0 on
 * success, -1 when `out` is too small (output then undefined). */
int fact_format_ref_cells(const uint8_t *buf, size_t len,
                          const char *(*lookup)(uint32_t handle),
                          char *out, size_t out_cap)
{
    size_t cells = len / 4;
    size_t i = 0;
    size_t pos = 0;

#define REF_APPEND(...)                                                    \
    do {                                                                   \
        int ra_n = snprintf(out + pos, out_cap - pos, __VA_ARGS__);         \
        if (ra_n < 0 || (size_t)ra_n >= out_cap - pos)                      \
            return -1;                                                      \
        pos += (size_t)ra_n;                                                \
    } while (0)

    if (len == 0) {
        REF_APPEND("<empty>");
        return 0;
    }
    if ((len % 4) != 0) {
        REF_APPEND("<malformed>");
        return 0;
    }
    while (i < cells) {
        uint32_t handle = fact_be32(buf + 4 * i);
        const char *path = lookup(handle);
        if (i > 0)
            REF_APPEND(",");
        if (cells - i >= 2) {
            uint32_t arg = fact_be32(buf + 4 * (i + 1));
            if (path)
                REF_APPEND("%s#%u", path, arg);
            else
                REF_APPEND("phandle=%x#%u", handle, arg);
            i += 2;
        } else {
            if (path)
                REF_APPEND("%s", path);
            else
                REF_APPEND("phandle=%x", handle);
            i += 1;
        }
    }
    return 0;

#undef REF_APPEND
}

/* --- read-only filesystem access (probe-only, not host-tested) --------- */

#ifndef MS09_BOARD_FACTS_TESTING

static uint8_t g_prop[MS09_PROP_CAP];
static char g_dt_root[MS09_PATH_CAP];

/* Read a full file.  Returns byte count (>= 0) or -errno. */
static long fact_read_file(const char *path, uint8_t *buf, size_t cap)
{
    FILE *f = fopen(path, "rb");
    long n;
    if (!f)
        return -(long)errno;
    n = (long)fread(buf, 1, cap, f);
    if (n == 0 && ferror(f)) {
        int saved = errno;
        fclose(f);
        return -(long)saved;
    }
    fclose(f);
    return n;
}

/* Read a device-tree property file.  Returns byte count or -errno. */
static long fact_read_prop(const char *node_dir, const char *name,
                           uint8_t *buf, size_t cap)
{
    char path[MS09_PATH_CAP];
    int n = snprintf(path, sizeof(path), "%s/%s", node_dir, name);
    if (n < 0 || (size_t)n >= sizeof(path))
        return -(long)ENAMETOOLONG;
    return fact_read_file(path, buf, cap);
}

static const char *fact_error_name(enum fact_error e)
{
    switch (e) {
    case FACT_MISSING:
        return "missing";
    case FACT_PERM:
        return "perm";
    case FACT_IOERR:
        return "ioerr";
    default:
        return "ok";
    }
}

static void fact_report_error(const char *item, int neg_errno)
{
    printf("MS09_FACTS_ERROR: item=%s result=%s:%s\n", item,
           fact_error_name(fact_classify_errno(-neg_errno)),
           strerror(-neg_errno));
}

/* Decode a string-list property into caller-owned storage: `buf` keeps the
 * raw bytes alive so returned pointers stay valid across later property
 * reads (which reuse g_prop).  Returns the string count, 0 when missing. */
static int fact_string_list_prop_buf(const char *node_dir, const char *name,
                                     uint8_t *buf, size_t buf_cap,
                                     const char **out, size_t max_out)
{
    long n = fact_read_prop(node_dir, name, buf, buf_cap);
    if (n < 0)
        return (int)fact_classify_errno((int)-n) == FACT_MISSING ? 0 : -1;
    return (int)fact_decode_string_list(buf, (size_t)n, out, max_out);
}

static int fact_u32_prop(const char *node_dir, const char *name, uint32_t *val)
{
    long n = fact_read_prop(node_dir, name, g_prop, sizeof(g_prop));
    if (n != 4)
        return -1;
    *val = fact_be32(g_prop);
    return 0;
}

/* --- phandle table ------------------------------------------------------ */

static uint32_t g_phandles[MS09_MAX_PHANDLES];
static char g_phandle_paths[MS09_MAX_PHANDLES][MS09_PATH_CAP];
static size_t g_phandle_count;

static const char *fact_resolve_phandle(uint32_t handle)
{
    size_t i;
    for (i = 0; i < g_phandle_count; i++) {
        if (g_phandles[i] == handle)
            return g_phandle_paths[i];
    }
    return NULL;
}

/* Print a phandle-cell reference property via fact_format_ref_cells;
 * unreadable properties keep the missing/perm/ioerr classification. */
static void fact_print_ref_prop(const char *node_dir, const char *name)
{
    long n = fact_read_prop(node_dir, name, g_prop, sizeof(g_prop));
    char out[MS09_REF_OUT_CAP];
    if (n < 0) {
        enum fact_error e = fact_classify_errno((int)-n);
        if (e != FACT_MISSING)
            printf(" %s=<unreadable:%s>", name, fact_error_name(e));
        return;
    }
    printf(" %s=", name);
    if (fact_format_ref_cells(g_prop, (size_t)n, fact_resolve_phandle,
                              out, sizeof(out)) == 0)
        fputs(out, stdout);
    else
        fputs("<overflow>", stdout);
}

/* --- tree walk ---------------------------------------------------------- */

static size_t g_nodes_seen;

/* Pass 1: record every node's phandle → DT path for reference resolution. */
static void fact_collect_phandles(const char *dir, const char *dt_path, int depth)
{
    DIR *d;
    struct dirent *de;
    uint32_t handle = 0;
    if (depth > MS09_MAX_DEPTH || g_nodes_seen >= MS09_MAX_NODES ||
        g_phandle_count >= MS09_MAX_PHANDLES)
        return;
    g_nodes_seen++;
    if (fact_u32_prop(dir, "phandle", &handle) == 0 &&
        strlen(dt_path) < MS09_PATH_CAP) {
        memcpy(g_phandle_paths[g_phandle_count], dt_path, strlen(dt_path) + 1);
        g_phandles[g_phandle_count] = handle;
        g_phandle_count++;
    }
    d = opendir(dir);
    if (!d)
        return;
    while ((de = readdir(d)) != NULL) {
        char child_dir[MS09_PATH_CAP];
        char child_path[MS09_PATH_CAP];
        long probe;
        if (de->d_name[0] == '.')
            continue;
        if (fact_path_join(child_dir, sizeof(child_dir), dir, de->d_name) < 0)
            continue;
        if (fact_path_join(child_path, sizeof(child_path), dt_path, de->d_name) < 0)
            continue;
        probe = fact_read_file(child_dir, g_prop, 1);
        if (probe != -EISDIR)
            continue;
        fact_collect_phandles(child_dir, child_path, depth + 1);
    }
    closedir(d);
}

/* MAC node DT paths for the netdev association pass. */
static char g_mac_paths[MS09_MAX_MAC_NODES][MS09_PATH_CAP];
static size_t g_mac_count;

/* --- report pass -------------------------------------------------------- */

static void fact_print_hex_cells(const char *node_dir, const char *name)
{
    long n = fact_read_prop(node_dir, name, g_prop, sizeof(g_prop));
    size_t cells = n > 0 ? (size_t)n / 4 : 0;
    size_t i;
    if (n < 0 || cells == 0) {
        enum fact_error e = n < 0 ? fact_classify_errno((int)-n) : FACT_MISSING;
        printf(" %s=<%s>", name,
               e == FACT_MISSING ? "missing" : fact_error_name(e));
        return;
    }
    printf(" %s=", name);
    for (i = 0; i < cells; i++)
        printf("%s%08x", i ? "," : "", fact_be32(g_prop + 4 * i));
}

static void fact_report_node(const char *dir, const char *dt_path,
                             const char *name, const char *parent_name,
                             unsigned int parent_ac, unsigned int parent_sc,
                             int depth)
{
    const char *compat[8] = {0};
    uint8_t compat_buf[256];
    int ncompat;
    int i;
    int is_uart = 0, is_mac = 0, is_mdio = 0, is_phy = 0, is_memory = 0,
        is_cpu = 0;
    uint64_t addr[8], size[8];

    if (depth > MS09_MAX_DEPTH || g_nodes_seen >= MS09_MAX_NODES)
        return;
    g_nodes_seen++;

    ncompat = fact_string_list_prop_buf(dir, "compatible", compat_buf,
                                        sizeof(compat_buf), compat, 8);
    for (i = 0; i < ncompat; i++) {
        if (strstr(compat[i], "uart"))
            is_uart = 1;
        if (strstr(compat[i], "mdio"))
            is_mdio = 1;
        else if (strstr(compat[i], "gmac") || strstr(compat[i], "dwmac"))
            is_mac = 1;
        if (strstr(compat[i], "ethernet-phy"))
            is_phy = 1;
    }
    if (strncmp(name, "memory", 6) == 0)
        is_memory = 1;
    if (strcmp(parent_name, "cpus") == 0 && strncmp(name, "cpu@", 4) == 0)
        is_cpu = 1;

    if (is_memory) {
        long n = fact_read_prop(dir, "reg", g_prop, sizeof(g_prop));
        if (n >= 0) {
            int pairs = fact_decode_reg(g_prop, (size_t)n, parent_ac, parent_sc,
                                        addr, size, 8);
            for (i = 0; i < pairs; i++)
                printf("MS09_FACTS_MEMORY: node=%s path=%s addr=0x%llx size=0x%llx\n",
                       name, dt_path,
                       (unsigned long long)addr[i], (unsigned long long)size[i]);
        } else {
            fact_report_error(name, (int)n);
        }
        return; /* memory nodes have no relevant children */
    }

    if (strcmp(name, "reserved-memory") == 0)
        return; /* children reported via the parent_name check below */

    if (is_cpu) {
        uint32_t reg = 0;
        char status[32];
        long sn;
        printf("MS09_FACTS_CPU: node=%s reg=", name);
        if (fact_u32_prop(dir, "reg", &reg) == 0)
            printf("%u", reg);
        else
            printf("<missing>");
        sn = fact_read_prop(dir, "status", (uint8_t *)status, sizeof(status) - 1);
        if (sn > 0) {
            status[sn] = '\0';
            printf(" status=%s\n", status);
        } else {
            printf(" status=<%s>\n", sn < 0 ? fact_error_name(fact_classify_errno((int)-sn))
                                            : "missing");
        }
        return;
    }

    if (is_uart) {
        long rn = fact_read_prop(dir, "reg", g_prop, sizeof(g_prop));
        printf("MS09_FACTS_UART: path=%s compatible=", dt_path);
        for (i = 0; i < ncompat; i++)
            printf("%s%s", i ? "," : "", compat[i]);
        if (rn >= 0) {
            int pairs = fact_decode_reg(g_prop, (size_t)rn, parent_ac, parent_sc,
                                        addr, size, 1);
            if (pairs == 1)
                printf(" reg=0x%llx/0x%llx", (unsigned long long)addr[0],
                       (unsigned long long)size[0]);
            else
                printf(" reg=<malformed>");
        } else {
            printf(" reg=<%s>", fact_error_name(fact_classify_errno((int)-rn)));
        }
        fact_print_hex_cells(dir, "interrupts");
        fact_print_ref_prop(dir, "clocks");
        fact_print_ref_prop(dir, "resets");
        fact_print_ref_prop(dir, "pinctrl-0");
        putchar('\n');
        return;
    }

    if (is_mac) {
        long rn = fact_read_prop(dir, "reg", g_prop, sizeof(g_prop));
        char phy_mode[32] = "missing";
        uint32_t max_speed = 0;
        long pm;
        printf("MS09_FACTS_MAC: path=%s compatible=", dt_path);
        for (i = 0; i < ncompat; i++)
            printf("%s%s", i ? "," : "", compat[i]);
        if (rn >= 0) {
            int pairs = fact_decode_reg(g_prop, (size_t)rn, parent_ac, parent_sc,
                                        addr, size, 1);
            if (pairs == 1)
                printf(" reg=0x%llx/0x%llx", (unsigned long long)addr[0],
                       (unsigned long long)size[0]);
            else
                printf(" reg=<malformed>");
        } else {
            printf(" reg=<%s>", fact_error_name(fact_classify_errno((int)-rn)));
        }
        pm = fact_read_prop(dir, "phy-mode", (uint8_t *)phy_mode,
                            sizeof(phy_mode) - 1);
        if (pm > 0)
            phy_mode[pm] = '\0';
        if (fact_u32_prop(dir, "max-speed", &max_speed) == 0)
            printf(" max-speed=%u", max_speed);
        printf(" phy-mode=%s", phy_mode);
        fact_print_hex_cells(dir, "interrupts");
        fact_print_ref_prop(dir, "clocks");
        fact_print_ref_prop(dir, "resets");
        fact_print_ref_prop(dir, "pinctrl-0");
        putchar('\n');
        if (g_mac_count < MS09_MAX_MAC_NODES) {
            memcpy(g_mac_paths[g_mac_count], dt_path, strlen(dt_path) + 1);
            g_mac_count++;
        }
        return;
    }

    if (is_mdio) {
        /* MDIO bus itself is structural; its children are PHYs. */
        DIR *d = opendir(dir);
        struct dirent *de;
        if (d) {
            while ((de = readdir(d)) != NULL) {
                char child_dir[MS09_PATH_CAP], child_path[MS09_PATH_CAP];
                uint32_t reg = 0;
                const char *pcompat[4] = {0};
                uint8_t pcompat_buf[128];
                int pn;
                if (de->d_name[0] == '.')
                    continue;
                if (fact_path_join(child_dir, sizeof(child_dir), dir, de->d_name) < 0)
                    continue;
                if (fact_path_join(child_path, sizeof(child_path), dt_path,
                                   de->d_name) < 0)
                    continue;
                if (fact_read_prop(child_dir, "reg", g_prop, 4) != 4)
                    continue;
                reg = fact_be32(g_prop);
                pn = fact_string_list_prop_buf(child_dir, "compatible",
                                               pcompat_buf, sizeof(pcompat_buf),
                                               pcompat, 4);
                printf("MS09_FACTS_PHY: path=%s reg=%u compatible=", child_path, reg);
                for (i = 0; i < pn; i++)
                    printf("%s%s", i ? "," : "", pcompat[i]);
                putchar('\n');
            }
            closedir(d);
        }
        return;
    }

    if (is_phy) {
        uint32_t reg = 0;
        printf("MS09_FACTS_PHY: path=%s compatible=", dt_path);
        for (i = 0; i < ncompat; i++)
            printf("%s%s", i ? "," : "", compat[i]);
        if (fact_u32_prop(dir, "reg", &reg) == 0)
            printf(" reg=%u", reg);
        putchar('\n');
        return;
    }

    if (strcmp(parent_name, "reserved-memory") == 0) {
        long n = fact_read_prop(dir, "reg", g_prop, sizeof(g_prop));
        if (n >= 0) {
            int pairs = fact_decode_reg(g_prop, (size_t)n, parent_ac, parent_sc,
                                        addr, size, 8);
            for (i = 0; i < pairs; i++)
                printf("MS09_FACTS_RESERVED: node=%s path=%s addr=0x%llx size=0x%llx\n",
                       name, dt_path, (unsigned long long)addr[i],
                       (unsigned long long)size[i]);
        }
        return;
    }
}

static void fact_walk_report(const char *dir, const char *dt_path,
                             const char *name, const char *parent_name,
                             unsigned int parent_ac, unsigned int parent_sc,
                             int depth)
{
    DIR *d;
    struct dirent *de;
    unsigned int own_ac = parent_ac, own_sc = parent_sc;
    uint32_t ac, sc;

    fact_report_node(dir, dt_path, name, parent_name, parent_ac, parent_sc, depth);

    if (fact_u32_prop(dir, "#address-cells", &ac) == 0)
        own_ac = ac;
    if (fact_u32_prop(dir, "#size-cells", &sc) == 0)
        own_sc = sc;

    d = opendir(dir);
    if (!d)
        return;
    while ((de = readdir(d)) != NULL) {
        char child_dir[MS09_PATH_CAP], child_path[MS09_PATH_CAP];
        uint8_t probe_buf[1];
        long probe;
        if (de->d_name[0] == '.')
            continue;
        if (fact_path_join(child_dir, sizeof(child_dir), dir, de->d_name) < 0)
            continue;
        if (fact_path_join(child_path, sizeof(child_path), dt_path, de->d_name) < 0)
            continue;
        /* Properties are files (read succeeds); sub-nodes are directories
         * (read fails with EISDIR).  Only recurse into directories. */
        probe = fact_read_file(child_dir, probe_buf, 1);
        if (probe != -EISDIR)
            continue;
        fact_walk_report(child_dir, child_path, de->d_name, name,
                         own_ac, own_sc, depth + 1);
    }
    closedir(d);
}

/* --- chosen / root properties ------------------------------------------- */

static void fact_report_root(const char *root_dir)
{
    const char *compat[8] = {0};
    uint8_t compat_buf[256];
    int ncompat;
    long n;
    char model[128] = "missing";
    char stdout_path[128] = "missing";
    char chosen_dir[MS09_PATH_CAP];

    n = fact_read_prop(root_dir, "model", (uint8_t *)model, sizeof(model) - 1);
    if (n > 0)
        model[n] = '\0';
    printf("MS09_FACTS_MODEL: %s\n", model);

    ncompat = fact_string_list_prop_buf(root_dir, "compatible", compat_buf,
                                        sizeof(compat_buf), compat, 8);
    printf("MS09_FACTS_COMPATIBLE:");
    for (int i = 0; i < ncompat; i++)
        printf(" %s", compat[i]);
    putchar('\n');

    if (fact_path_join(chosen_dir, sizeof(chosen_dir), root_dir, "chosen") >= 0) {
        long sn = fact_read_prop(chosen_dir, "stdout-path",
                                 (uint8_t *)stdout_path, sizeof(stdout_path) - 1);
        if (sn > 0)
            stdout_path[sn] = '\0';
        printf("MS09_FACTS_CHOSEN: stdout-path=%s console=", stdout_path);
        n = fact_read_prop(chosen_dir, "bootargs", g_prop, sizeof(g_prop) - 1);
        if (n > 0) {
            char *save = NULL;
            char *tok;
            g_prop[n] = '\0';
            for (tok = strtok_r((char *)g_prop, " \t", &save); tok;
                 tok = strtok_r(NULL, " \t", &save)) {
                if (strncmp(tok, "console=", 8) == 0)
                    printf("%s ", tok);
            }
        } else {
            printf("<%s>", n < 0 ? fact_error_name(fact_classify_errno((int)-n))
                                 : "missing");
        }
        putchar('\n');
    } else {
        printf("MS09_FACTS_CHOSEN: <missing>\n");
    }
}

/* --- CPU / netdev sysfs -------------------------------------------------- */

static void fact_report_cpu_sys(void)
{
    static const char *files[] = {"online", "possible", "present"};
    size_t i;
    for (i = 0; i < sizeof(files) / sizeof(files[0]); i++) {
        char path[MS09_PATH_CAP];
        long n;
        snprintf(path, sizeof(path), "/sys/devices/system/cpu/%s", files[i]);
        n = fact_read_file(path, g_prop, sizeof(g_prop) - 1);
        printf("MS09_FACTS_CPU_SYS: %s=", files[i]);
        if (n > 0) {
            g_prop[n] = '\0';
            while (n > 0 && (g_prop[n - 1] == '\n' || g_prop[n - 1] == '\r'))
                g_prop[--n] = '\0';
            printf("%s", (char *)g_prop);
        } else {
            enum fact_error e = n < 0 ? fact_classify_errno((int)-n) : FACT_MISSING;
            printf("<%s>", e == FACT_MISSING ? "missing" : fact_error_name(e));
        }
        putchar(i + 1 < sizeof(files) / sizeof(files[0]) ? ' ' : '\n');
    }
}

static void fact_report_netdevs(void)
{
    const char *net = "/sys/class/net";
    DIR *d = opendir(net);
    struct dirent *de;
    char root_canon[MS09_PATH_CAP];
    const char *cmp_root = g_dt_root;
    if (!d) {
        printf("MS09_FACTS_ERROR: item=sysfs_net result=%s\n",
               fact_error_name(fact_classify_errno(errno)));
        return;
    }
    /* of_node links resolve into the canonical devicetree location, so a
     * procfs-style root alias (/proc/device-tree) must be normalized too. */
    if (realpath(g_dt_root, root_canon))
        cmp_root = root_canon;
    while ((de = readdir(d)) != NULL) {
        char of_link[MS09_PATH_CAP], of_resolved[MS09_PATH_CAP];
        char carrier_path[MS09_PATH_CAP], driver_link[MS09_PATH_CAP];
        char err_item[272]; /* "netdev_of_node:" + NAME_MAX ifname + NUL */
        struct stat st;
        size_t m;
        if (de->d_name[0] == '.' || strcmp(de->d_name, "lo") == 0)
            continue;
        snprintf(of_link, sizeof(of_link), "%s/%s/device/of_node", net, de->d_name);
        if (lstat(of_link, &st) != 0) {
            if (errno == ENOENT)
                continue; /* no of_node attribute: virtual iface, not an error */
            snprintf(err_item, sizeof(err_item), "netdev_of_node:%s", de->d_name);
            fact_report_error(err_item, -errno);
            continue;
        }
        /* of_node is a relative sysfs symlink; compare the absolute path it
         * resolves to, never the raw link target. */
        if (!realpath(of_link, of_resolved)) {
            snprintf(err_item, sizeof(err_item), "netdev_of_node:%s", de->d_name);
            fact_report_error(err_item, -errno);
            continue;
        }
        for (m = 0; m < g_mac_count; m++) {
            char expected[MS09_PATH_CAP];
            long n;
            if (snprintf(expected, sizeof(expected), "%s%s", cmp_root,
                         g_mac_paths[m]) >= (int)sizeof(expected))
                continue;
            if (strcmp(of_resolved, expected) != 0)
                continue;
            printf("MS09_FACTS_NET: iface=%s of_node=%s", de->d_name,
                   g_mac_paths[m]);
            snprintf(driver_link, sizeof(driver_link), "%s/%s/device/driver",
                     net, de->d_name);
            {
                char drv[MS09_PATH_CAP];
                ssize_t dl = readlink(driver_link, drv, sizeof(drv) - 1);
                if (dl > 0) {
                    char *base;
                    drv[dl] = '\0';
                    base = strrchr(drv, '/');
                    printf(" driver=%s", base ? base + 1 : drv);
                } else {
                    printf(" driver=<missing>");
                }
            }
            snprintf(carrier_path, sizeof(carrier_path), "%s/%s/carrier",
                     net, de->d_name);
            n = fact_read_file(carrier_path, g_prop, 1);
            if (n == 1)
                printf(" carrier=%c\n", g_prop[0] == '1' ? '1' : '0');
            else
                printf(" carrier=<%s>\n",
                       n < 0 ? fact_error_name(fact_classify_errno((int)-n))
                             : "missing");
            break;
        }
    }
    closedir(d);
}

/* --- entry point --------------------------------------------------------- */

int main(void)
{
    const char *sysfs_root = "/sys/firmware/devicetree/base";
    const char *procfs_root = "/proc/device-tree";
    const char *source;
    long probe;

    probe = fact_read_prop(sysfs_root, "compatible", g_prop, 1);
    if (probe >= 0) {
        snprintf(g_dt_root, sizeof(g_dt_root), "%s", sysfs_root);
        source = "sysfs";
    } else {
        probe = fact_read_prop(procfs_root, "compatible", g_prop, 1);
        if (probe < 0) {
            printf("MS09_FACTS_BEGIN\n");
            printf("MS09_FACTS_ERROR: item=device_tree_root result=%s\n",
                   fact_error_name(fact_classify_errno((int)-probe)));
            printf("MS09_FACTS_END\n");
            return 0;
        }
        snprintf(g_dt_root, sizeof(g_dt_root), "%s", procfs_root);
        source = "procfs";
    }

    printf("MS09_FACTS_BEGIN\n");
    printf("MS09_FACTS_ROOT: source=%s root=%s\n", source, g_dt_root);

    fact_report_root(g_dt_root);

    g_nodes_seen = 0;
    fact_collect_phandles(g_dt_root, "/", 0);
    g_nodes_seen = 0;
    fact_walk_report(g_dt_root, "/", "", "", 2, 1, 0);

    fact_report_cpu_sys();
    fact_report_netdevs();

    printf("MS09_FACTS_FIRMWARE_ONLY: item=u_boot_relocation note=需启动阶段核对\n");
    printf("MS09_FACTS_FIRMWARE_ONLY: item=safe_ram_region note=需启动阶段核对\n");
    printf("MS09_FACTS_FIRMWARE_ONLY: item=firmware_harts note=需启动阶段核对\n");

    printf("MS09_FACTS_END\n");
    return 0;
}

#endif /* MS09_BOARD_FACTS_TESTING */
