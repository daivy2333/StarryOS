#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdio.h>
#include <string.h>

#define BAIDU_PROBE_CLIENT_TESTING
#include "baidu_probe_client.c" /* decision core only (guards out main) */

static void expect(probe_result got, probe_result want, const char *label)
{
    if (got != want) {
        fprintf(stderr, "FAIL %s: got %d want %d\n", label, (int)got, (int)want);
        __builtin_abort();
    }
    printf("PASS %s\n", label);
}

int main(void)
{
    expect(probe_classify("BAIDU_PROBE_OK http=200 ping_rtt_ms=33.3 "
                          "total_ms=50.0"),
           PROBE_RESULT_OK, "ok-marker");
    expect(probe_classify("BAIDU_PROBE_ICMP_FAIL http=200 total_ms=50.0"),
           PROBE_RESULT_OK, "icmp-fail-reachable");
    expect(probe_classify("BAIDU_PROBE_HTTP_FAIL ping_rtt_ms=33.3 "
                          "total_ms=50.0"),
           PROBE_RESULT_OK, "http-fail-reachable");
    expect(probe_classify("BAIDU_PROBE_UNREACHABLE total_ms=50.0"),
           PROBE_RESULT_UNREACHABLE, "unreachable");
    expect(probe_classify(NULL), PROBE_RESULT_UNKNOWN, "null");
    expect(probe_classify("garbage line"), PROBE_RESULT_UNKNOWN, "garbage");
    expect(probe_classify("  BAIDU_PROBE_OK leading=1"), PROBE_RESULT_OK,
           "leading-space-ok");
    return 0;
}