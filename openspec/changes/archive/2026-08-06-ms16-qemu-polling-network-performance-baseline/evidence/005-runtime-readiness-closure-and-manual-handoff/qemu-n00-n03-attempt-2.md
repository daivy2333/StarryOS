# N00-N03 attempt 2

- Source: user-submitted QEMU and host output, 2026-08-05
- Guest artifact: SHA-256 `8eabc02c7be50fc13ab09ca22956f1bd120faa020f5335983d9a9ebd3f084389`

| Check | Result |
|---|---|
| Guest payload hash | PASS |
| Workload self-test | PASS、exit 0 |
| Monotonic calibration | PASS、1000 samples、1100/1266/21200 ns min/mean/max |
| `/proc/instret` | PASS、available、begin/end/overhead present |
| TCP bidi 2-flow loopback | PASS、双端 358400 bytes、256 packets、ledger closed |
| UDP bidi 2-flow loopback | FAIL contract：data ledger closed but `udp_offered=0` and `udp_accepted=0` |
| Guest interface command | SKIPPED：`ip addr` requires unsupported AF_NETLINK |
| Boot interface witness | PASS：eth0 `10.0.2.15/24` in serial log |
| Host ping | PASS：3/3、0% loss |

The UDP metric failure produced a permanent RED witness. `simulate_direction` now increments offered and accepted for every simulated UDP TX packet. Host GREEN covers TCP/UDP × TX/RX/bidi × 1/2/4/8 flows.

The guest artifact from this attempt is stale after the fix and must not enter formal smoke.
