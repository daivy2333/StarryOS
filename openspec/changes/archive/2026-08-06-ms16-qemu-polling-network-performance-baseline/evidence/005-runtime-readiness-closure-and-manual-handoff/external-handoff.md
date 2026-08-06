# External and manual handoff

用户在 sandbox 外重新构建 guest workload。共享文件系统独立复核通过。

| Fact | Result |
|---|---|
| Build exit | 0 |
| File | RISC-V 64-bit static-pie |
| SHA-256 | `b863b060500c3a0977102e840d2d7160d75d7ea899567ab38cc72891ad5f1eb3` |
| Source | 2026-08-05 17:34:21 +0800、47792 bytes |
| Binary | 2026-08-05 17:40:09 +0800、160112 bytes |
| Kernel build exit | 0 |
| User preflight exit | 0 |
| Agent preflight exit | 0 |
| Host peer SHA-256 | `f533f606429874328fce53d3c5b84b9e455e849e4787860a72a2a9b597122e3a` |
| Kernel SHA-256 | `0582eba52e00dc332f03562465b9ce423aff512606405f8c3ad1deaaa37d5277` |
| Rootfs SHA-256 | `1ef940d0f9f3a7129ed0572a01fe9e3ebad1dc24e2118e83f0f1e14b55881249` |
| QEMU | 7.0.0、`virt`、1 vCPU、1024 MiB、`icount=n` |

旧 hash `68a628b0431cfa01a37810d46c9231b7ae29b283910895aaa31a453a52da82d3` 和 `8eabc02c7be50fc13ab09ca22956f1bd120faa020f5335983d9a9ebd3f084389` 已失效，不得用于本轮 manifest。

等待用户按 `manual-calibration.md` 提交 user-net/TAP Evidence。Iteration 005 保持 `pending`。
