/* Pure classification helpers for benchmark section validity.
   No I/O or hardware dependency — usable in host tests and runtime code.
   Included by benchmark.c and benchmark_classify_test.c. */

#ifndef BENCHMARK_CLASSIFY_H
#define BENCHMARK_CLASSIFY_H

#include <stddef.h>

/* Completion validity for write sections (S11, S41, S42).
   Returns 1 if all checks pass, 0 otherwise.
   completed, expected: byte counts
   incomplete_logical: count of writes that didn't finish
   final_drain_rc: result of final tcdrain (0 = ok)
   drain_errors: count of drain failures */
static inline int classify_write_completion(size_t completed, size_t expected,
                                            int incomplete_logical,
                                            int final_drain_rc, int drain_errors) {
    if (completed != expected) return 0;
    if (incomplete_logical != 0) return 0;
    if (final_drain_rc < 0) return 0;
    if (drain_errors != 0) return 0;
    return 1;
}

/* Overlap applicability check for S42.
   Returns 1 if the synchronous Console write allows overlap measurement,
   0 if the write blocked for the full line time (no overlap possible).
   write_dur_ns: duration of the write call in ns
   theoretical_line_time_ns: expected UART line time for this payload */
static inline int classify_overlap_applicable(long long write_dur_ns,
                                              long long theoretical_line_time_ns) {
    /* On synchronous Console, write_dur_ns >= theoretical_line_time_ns
       means the write blocked for the full transmission → zero overlap.
       This is valid data, not a failure. */
    return (write_dur_ns < theoretical_line_time_ns) ? 1 : 0;
}

/* S43 loaded group applicability.
   Returns 1 if the loaded write completed fast enough to leave an overlap
   window for timer observation, 0 if write blocked for the full line time
   (not-applicable for loaded overshoot measurement). */
static inline int classify_s43_loaded_applicable(long long write_dur_ns,
                                                  long long theoretical_line_time_ns) {
    if (write_dur_ns < 0) return 0; /* timer error */
    if (theoretical_line_time_ns <= 0) return 0;
    return (write_dur_ns < theoretical_line_time_ns) ? 1 : 0;
}

/* Overlap efficiency computation.
   Returns the ratio of useful iterations under UART TX load vs idle baseline.
   Zero overlap is valid (returns 0.0), idle_iters == 0 returns 0.0.
   No division by zero. */
static inline double classify_overlap_efficiency(unsigned long long uart_iters,
                                                  unsigned long long idle_iters) {
    if (idle_iters == 0) return 0.0;
    return (double)uart_iters / (double)idle_iters;
}

/* Counter capability state from ioctl result.
   Returns: 0 = not-available (ioctl failed), 1 = available, 2 = zero-bytes */
static inline int classify_counter_state(int ioctl_rc, size_t completed_bytes) {
    if (ioctl_rc < 0) return 0; /* not-available */
    if (completed_bytes == 0) return 2; /* zero-bytes */
    return 1; /* available */
}

#endif /* BENCHMARK_CLASSIFY_H */
