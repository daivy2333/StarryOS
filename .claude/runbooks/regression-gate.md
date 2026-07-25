# 回归验证 Gate

## 适用范围

- 每次 Phase/change 完成前声明"无退化"时的标准化验证链
- 适用于：Phase 收尾、bugfix 后验证、重构后对比
- 不适用于：开发中快速迭代 (用增量融合流程 `incremental-merge.md`)

## 操作步骤

### 第一层：编译与 Lint

```bash
# 按目标平台执行 check 和 clippy
cargo check --features <platform> --target <target-triple>
cargo clippy --features <platform> --target <target-triple>

# 单元测试
cargo test && cargo test --doc
```

目标平台全部通过 + tests 通过后才能声明第一层就绪。

### 第二层：QEMU boot

```bash
make build                    # exit 0
make run                      # 进入 shell，确认 I/O 设备可用
```

### 第三层：QEMU benchmark

```bash
# QEMU 内
./benchmark                   # 确认执行到 Done.
```

**判据**（相对对比——与上阶段 QEMU 基线比，不是与真板比）：
- 1B latency avg 差异 <10%
- 64B TX throughput 差异 <10%
- Nonblocking I/O 双 PASS
- drain_errors 全 0

### 第四层：真板构建

确认真板 build 产物正常：boot image magic/address/size 正确，尺寸 << boot 分区容量。

### 第五层：真板 benchmark

**判据**（绝对对比——真板物理线速）：
- line_rate_pct ≥93%（各尺寸）
- Nonblocking I/O 双 PASS
- 无异常耗尽计数
- drain_errors 全 0

## 通过条件

| 层 | 条件 | 阻塞级 |
|---|---|---|
| 编译+Lint | 全部平台 check+clippy 全 0 | BLOCK |
| 单元测试 | crate tests + doc tests 全通过 | BLOCK |
| QEMU boot | `make run` 进入 shell | BLOCK |
| QEMU benchmark | Done. + 相对退化 <10% | BLOCK |
| 真板构建 | boot image inspect 通过 | BLOCK |
| 真板 benchmark | Done. + line_rate_pct ≥93% | BLOCK (缺硬件时为 ENV BLOCK) |

## ENV BLOCK

当硬件不可用时（如当前 VisionFive2 未到位）：
- 真板 benchmark 标记为 ENV BLOCK，不阻塞 Phase 声明
- 但必须在声明中明确标注"本次声明不含真板验证"
- ENV BLOCK 解除后必须补跑真板 benchmark 并追加 evidence

## 注意事项

- **全部平台必须通过**：只跑 QEMU 不够——不同平台的 feature gate 差异可能掩盖链接错误。
- **QEMU benchmark 只看相对退化**：QEMU 绝对吞吐不可信。QEMU 阶段目的是确认"代码没变得更糟"。
- **真板 benchmark 才是绝对判据**：物理线速声明必须以真板数据为准。
- **每次 Phase 收尾重新采集**：不能用旧 baseline 声明新 Phase 无退化。
- **记录原始证据**：QEMU log 和真板 serial log 保存到 change evidence 目录，登记 R。
