# 回归验证 Gate

## 适用范围

- 每次 Phase/change 完成前声明"无退化"时的标准化验证链
- 适用于：Q 系列 Phase 收尾、bugfix 后验证、重构后对比
- 不适用于：开发中快速迭代 (用增量融合流程 `incremental-merge.md`)

## 操作步骤

### 第一层：编译与 Lint

```bash
# 三模式 cargo check
cargo check --features qemu --target riscv64gc-unknown-none-elf
cargo check --features lichee-d1 --target riscv64gc-unknown-none-elf
cargo check --features lichee-d1-kbench --target riscv64gc-unknown-none-elf

# 三模式 clippy
cargo clippy --features qemu --target riscv64gc-unknown-none-elf
cargo clippy --features lichee-d1 --target riscv64gc-unknown-none-elf
cargo clippy --features lichee-d1-kbench --target riscv64gc-unknown-none-elf

# 单元测试 (crate 级别)
cd crates/uart_16550 && cargo test && cd -
cd crates/uart_16550 && cargo test --doc && cd -

# host-test (kernel 纯逻辑)
make host-test
```

三模式全部通过 + tests 通过后才能声明第一层就绪。

### 第二层：QEMU boot

```bash
make build                    # exit 0
make run                      # 进入 shell，确认 /dev/console 可用
```

### 第三层：QEMU benchmark

```bash
# QEMU 内
./benchmark                   # 确认执行到 Done.
```

**判据**（相对对比——与上阶段 QEMU 基线比，不是与 D1 比）：
- 1B latency avg 差异 <10%
- 64B TX throughput 差异 <10%
- S30 FIONBIO 双 PASS
- drain_errors 全 0

### 第四层：D1 构建

```bash
# 确认 D1 build 产物正常
make lichee-fullbench-command
python3 tools/android_boot_image.py inspect starry-lichee-fullbench-command-boot.img
# 必须: magic ANDROID!, kernel_addr 0x40200000, page_size 2048
ls -lh starry-lichee-fullbench-command-boot.img   # << 10MB
```

### 第五层：D1 benchmark

```bash
# 烧录并运行 (详见 d1-build-and-flash.md)
# 确认 benchmark 执行到 Done.
# 进程退出码 0
```

**判据**（绝对对比——D1 物理线速）：
- S10 各尺寸 line_rate_pct ≥93%
- S11 short_writes 收敛（对比上阶段 D1 基线）
- S30 双 PASS
- S40 `slow_poll_exh=0, yield_exh=0`
- drain_errors 全 0

## 通过条件

| 层 | 条件 | 阻塞级 |
|---|---|---|
| 编译+Lint | 三模式 check+clippy 全 0 | BLOCK |
| 单元测试 | crate tests + doc tests + host-test 全通过 | BLOCK |
| QEMU boot | `make run` 进入 shell | BLOCK |
| QEMU benchmark | Done. + 相对退化 <10% | BLOCK |
| D1 构建 | boot image inspect 通过 | BLOCK |
| D1 benchmark | Done. + line_rate_pct ≥93% | BLOCK (缺硬件时为 ENV BLOCK) |

## ENV BLOCK

当硬件不可用时（如当前 Q24 VisionFive2 未到位）：
- D1 benchmark 标记为 ENV BLOCK，不阻塞 Phase 声明
- 但必须在声明中明确标注"本次声明不含 D1 真板验证"
- ENV BLOCK 解除后必须补跑 D1 benchmark 并追加 evidence

## 注意事项

- **三模式必须全部通过**：只跑 QEMU 模式不够——D1 smoke/kbench 的 feature gate 差异可能掩盖链接错误。
- **QEMU benchmark 只看相对退化**：QEMU 的绝对吞吐不可信。QEMU 阶段的目的是确认"代码没变得更糟"。
- **D1 benchmark 才是绝对判据**：物理线速声明必须以 D1 数据为准。
- **每次 Phase 收尾重新采集**：不能用旧 baseline 声明新 Phase 无退化。
- **记录原始证据**：QEMU log 和 D1 serial log 保存到 `.claude/analysis/<phase>-evidence/`，登记 R。
