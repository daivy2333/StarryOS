## 1. Q19C M2 语义收口

- [x] 1.1 修改 Q19C proposal/design/spec：M2 必达目标为 `lichee-memory-root-command`
- [x] 1.2 将 true shell path 标为 future optional，不再作为 Q19C board gate
- [x] 1.3 保留 `shell_status=SKIPPED` 作为 M2 command-entry 的合法证据
- [x] 1.4 明确不得把 command-entry 记录为 shell-launched benchmark success
- [x] 1.5 更新 Q19C Requirements Traceability Matrix，标出 true shell 简化项已由用户批准

## 2. Feature 模式互斥

- [x] 2.1 列出当前 Lichee fullbench mode feature：M1 path (`lichee-d1-fullbench`)、M2 command (`lichee-d1-fullbench-command`)、M3 rootfs-probe (`lichee-d1-rootfs-probe`)
- [x] 2.2 为不兼容 feature 组合增加 `compile_error!` 或等价构建保护 → 3 对互斥 guard 已加入 `kernel/src/lib.rs`
- [x] 2.3 验证单模式 cargo check 仍通过 → M2 ✅、M3 ✅、M1(Q19B regression) ✅
- [x] 2.4 验证不兼容组合给出明确错误 → fullbench+command ✅、fullbench+probe ✅、command+probe ✅（均以 `compile_error!` 为第一错误）

## 3. 任务状态修正

- [x] 3.1 修正 `.claude/docs/tasks.md`：Q19C.10 拆为 10a(host)✅ + 10b(acceptance)✅ + 10c(board)⬜；Q19C.11 拆为 11a(host)✅ + 11b(acceptance)✅ + 11c(board)⬜
- [x] 3.2 保留 host gate 已完成证据：cargo check、image build、kernel_size
- [x] 3.3 保留 board gate pending 证据模板：M2 UART log、M3 UART log

## 4. M2 证据口径

- [x] 4.1 检查 M2 日志：包含 mode label、shell skipped blocker、entry、argv/envp summary、stdio marker
- [x] 4.2 不修改 benchmark payload，文档写成 kernel-side argv/envp construction proof → entry.rs 已添加 `argv_evidence=kernel-side-construction` 和 `note=user-observed-argv-not-claimed`
- [x] 4.3 若要声明 user-observed argv/envp，则修改 payload → 当前 payload 不打印 argc/argv，user-observed 不声称
- [ ] 4.4 M2 board gate 只在 D1 真板日志出现 benchmark sections 和 exit code 0 后勾选

## 5. M3 证据口径

- [x] 5.1 检查 M3 日志：包含 `lichee-rootfs-probe`、known facts、TBD/blocker、`rootfs_init=NOT called`
- [x] 5.2 未实现 MMIO/register read，文档写为 TBD/SKIPPED，不写成 register probe success
- [x] 5.3 确认 rootfs-probe feature 不依赖 `axfs-ng::init_filesystems()` 空 block path → `lib.rs` 将 rootfs-probe 加入 module exclusion gates，无 `axfs`/`axfs-ng` 依赖
- [ ] 5.4 M3 board gate 只在 D1 真板日志出现 probe table 和无 panic 后勾选

## 6. 验证

- [x] 6.1 `openspec validate q19c-m2-m3-acceptance-alignment --strict` → valid ✅
- [x] 6.2 `openspec validate --changes` → 3 passed ✅
- [x] 6.3 `openspec validate --specs` → 16 passed ✅
- [x] 6.4 M2 command 单模式 cargo check 通过 ✅
- [x] 6.5 M3 rootfs-probe 单模式 cargo check 通过 ✅
- [x] 6.6 不兼容 feature 组合 negative check 通过（3/3 产生 compile_error!） ✅
- [x] 6.7 `make lichee-fullbench-command` 生成 starry-lichee-fullbench-command-boot.img (kernel_size=999616) ✅
- [x] 6.8 `make lichee-rootfs-probe` 生成 starry-lichee-rootfs-probe-boot.img (kernel_size=159936) ✅
