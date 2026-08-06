# N00 attempt 1

- Source: user-submitted QEMU serial excerpt, 2026-08-05
- Topology: QEMU user-net、guest `10.0.2.15`、host HTTP `10.0.2.2:18765`
- Result: FAIL before workload execution

Observed layers:

1. QEMU boot、VirtIO-MMIO net/block、eth0、UART IRQ 10 和 net IRQ 7 signatures PASS。
2. Host HTTP server returned `200` for `GET /network_benchmark`.
3. Guest `mkdir -p /root/ms16` failed with `mkdir: can't create directory '/': Invalid argument`.
4. `wget` then failed to open `/root/ms16/network_benchmark`; `chmod`、hash 和 workload commands were downstream failures.
5. A multi-line paste lost whitespace and joined commands. R44/R45 require one command per prompt.

Earliest failing layer: guest payload destination setup。

Recovery: use the R44/R45 verified `/tmp/network_benchmark` path for N00-N03 and enter one command only after each new `starry:~#` prompt.
