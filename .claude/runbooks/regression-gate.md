# 回归验证 Gate

- Status: active
- Last validated: 2026-08-23
- Environment: StarryOS `net-k3`；agent workspace-write sandbox 与用户正常宿主环境
- Source: MS06 Cycle `000-resident-stack-runner/000-initial.md` blocked Act Response、用户宿主 `make build` exit 0、R44 历史构建分类证据

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

#### 构建失败分类

构建 Gate 必须先固定产品命令的 target、feature、平台配置和构建入口。默认 `make build`
构建 QEMU 产品，不能替代 `lichee-d1`、VF2 或其他平台 Gate；缺少目标架构的 Cargo
命令也不能作为该平台的有效见证。

按以下顺序判断，不能因准备阶段出现安装或联网报错就立即阻塞：

1. 记录完整命令、最终退出码和预期产物。
2. 找到最早决定最终结果的失败层：命令选择、依赖探测、Rust/C 编译、链接、objcopy
   或产物检查。
3. 最终 exit 0 且预期产物由实际工具生成时，Gate 为 PASS；此前 Cargo home 只读、联网
   失败或自动安装告警属于环境噪声。
4. 最终非零且出现 Rust/C 编译、链接或测试诊断时，Gate 为产品 FAIL，即使更早还出现
   sandbox 告警。
5. 最终非零且只出现权限、网络、缺硬件或 sandbox syscall 拒绝，没有进入产品编译失败
   层时，才记 `ENV BLOCK` 并请求用户在正常宿主复跑同一命令。
6. target/feature/平台配置不匹配时，结果记为 invalid witness；改用项目支持的产品命令，
   不把它计为 PASS、产品 FAIL 或连续修复失败。

工具可用性以实际调用优先，包管理器登记只作辅助。例如 `cargo install --list` 在只读
Cargo home 中可能失败并让 Makefile 误判 `cargo-binutils` 未安装；若 `rust-objcopy` 随后
成功执行并生成 binary，则不得再把 `cargo-binutils` 记为缺失依赖。

```bash
# 工具实际可调用性
command -v rust-objcopy
rust-objcopy --version

# 默认 QEMU 产品
make build

# D1 产品；不能用默认 make build 替代
make lichee

# D1 编译隔离见证
cargo check --locked --offline \
  --target riscv64gc-unknown-none-elf \
  --features lichee-d1
```

用户宿主复跑只解除对应环境疑点。若用户执行的是不同产品命令，例如 agent 的 D1 Gate
失败而用户只验证默认 QEMU `make build`，只能证明工具链与 QEMU 构建正常，D1 产品失败
仍须单独处理。

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

当硬件不可用或命令只因环境能力限制而无法执行时（如当前 VisionFive2 未到位）：
- 真板 benchmark 标记为 ENV BLOCK，不阻塞 Phase 声明
- 但必须在声明中明确标注"本次声明不含真板验证"
- ENV BLOCK 解除后必须补跑真板 benchmark 并追加 evidence
- 若同一日志还包含产品编译或链接错误，以产品错误为准，不得用 ENV BLOCK 覆盖

## 注意事项

- **全部平台必须通过**：只跑 QEMU 不够——不同平台的 feature gate 差异可能掩盖链接错误。
- **QEMU benchmark 只看相对退化**：QEMU 绝对吞吐不可信。QEMU 阶段目的是确认"代码没变得更糟"。
- **真板 benchmark 才是绝对判据**：物理线速声明必须以真板数据为准。
- **每次 Phase 收尾重新采集**：不能用旧 baseline 声明新 Phase 无退化。
- **记录原始证据**：QEMU log 和真板 serial log 保存到 change evidence 目录，登记 R。

## 失败处理

| 症状 | 分类 | 处理 |
|---|---|---|
| `cargo install --list` 报 Cargo home 只读，随后 `rust-objcopy` 成功且最终 exit 0 | 环境噪声，Gate PASS | 记录最终 exit 和产物，不安装、不阻塞 |
| `sbi-rt` 报 `invalid register a0/a7`，命令未指定 RISC-V target | invalid witness | 补正确 target 后重跑，不计产品失败次数 |
| 正确 target 下出现本仓库 `E0432/E0433`、链接错误或测试失败 | 产品 FAIL | 按首个产品失败层定位；环境告警不能覆盖 |
| 命令因只读、网络、`EPERM` 或缺硬件终止，未进入产品失败层 | ENV BLOCK | 保留命令和退出码，交给用户同命令复跑 |
| 用户宿主运行不同平台命令成功 | 只解除共享工具链疑点 | 原平台 Gate 仍需运行 |

## 回滚

本分类流程不修改产品代码。诊断中不得为绕过 sandbox 修改依赖版本、平台 feature 或
产品实现；误记的 blocker 在当前 Cycle 的 Act Response 或后继 Review 中更正，保留原始
命令和结论变化。

## 证据

- 2026-08-09 R44：同一次 `make LOG=info build` 出现 Cargo home 只读、联网失败，随后
  已安装的 `rust-objcopy` 生成镜像，最终 exit 0。
- 2026-08-23 MS06 Cycle 000：agent sandbox 的依赖探测误判 `cargo-binutils` 缺失；用户
  正常宿主执行默认 QEMU `make build`，release build 与 objcopy 均成功。
- 同一 Cycle 的正确 D1 target 仍在 `kernel/src/lib.rs` 报 `E0432/E0433`，证明环境噪声与
  产品 feature 回归必须分层记录。
