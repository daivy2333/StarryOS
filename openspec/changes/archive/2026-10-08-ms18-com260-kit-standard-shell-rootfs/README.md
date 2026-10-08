# ms18-com260-kit-standard-shell-rootfs

优先建立 K3 普通 /bin/sh 与 QEMU 同源 RAM rootfs 用户环境，保留临时启动及 RESET 回官方 Linux。用户已批准完整计划，原话“批准计划”；首轮执行契约 ready，尚未实施。

- 需求和场景：`proposal.md`、`specs/k3-com260-kit-standard-shell-rootfs/spec.md`
- 设计：`design.md`
- 任务分轮：`tasks.md`
- 当前执行契约：`iterations/000-ram-rootfs/000-initial.md`（ready）
- 调查正文：`/tmp/ms18-shell-rootfs-exploration.md`；环境拒绝 `.agents` 写入，未登记 R。

规划文档检查：`openspec validate ms18-com260-kit-standard-shell-rootfs --strict --no-interactive` 输出 `Change 'ms18-com260-kit-standard-shell-rootfs' is valid`，退出码0；`openspec status --change ms18-com260-kit-standard-shell-rootfs` 显示四项规划产物齐全，退出码0。这些结果只检查计划结构，不证明产品行为。
