# 增量融合流程

## 适用范围

- 将多个 async-uart 相关优化 commit 从临时分支合入主干
- 不适用于：单 commit 简单修复、无副作用的格式化/typo 变更

## 为什么必须增量

Q15 核心教训（L205）：**一次性 apply 全部 M4+ 优化代码 → 64B write+tcdrain 退化 73.9x（406µs → 29.99ms）**。改为按依赖排序 + 每步 Gate 后，5 天完成 + 无退化。

## 前置条件

- 临时分支（如 `feat/uart-16550-async-temp`）保留完整，不删除
- QEMU 环境已按 `qemu-build.md` 准备就绪
- 已有当前基线的 benchmark 数据作为对照

## 操作步骤

### 1. 依赖排序

按"基线能力 → 修复 → 契约"层次拆分子集，而不是按 commit 编号或时间顺序：

```
M0: 见证层 — 添加测量/诊断能力，不改变行为。提供基准线。
M1/M2: 修复 — 修正已知问题，改动最小。
M4: 规范化 — 接口/契约收敛，依赖 M1/M2 稳定的内部行为。
M3: 边界契约 — 只改 trait/API 签名，不碰驱动内部，放最后。
```

### 2. 逐步 apply

```bash
# 对每个 milestone 子集：
git cherry-pick <子集 commits>    # 或 git apply <patch>
cargo check --features qemu       # 编译检查
cargo clippy --features qemu      # lint 检查
make build && make run            # QEMU boot
./benchmark                       # 性能 Gate (见下表)
```

### 3. Gate 表

每个 milestone apply 后必须通过：

| Gate | 命令 | 通过条件 |
|---|---|---|
| 编译 | `cargo check --features qemu` | exit 0 |
| Lint | `cargo clippy --features qemu` | exit 0 |
| Boot | `make run` | 进入 shell |
| 1B 延迟 | benchmark S20 avg | 与基线差异 <10% |
| 64B 吞吐 | benchmark S10 64B KB/s | 与基线差异 <10% |
| FIONBIO | benchmark S30 | 双 PASS |

### 4. 退化处理

如某步 Gate 失败（退化 >10%）：
1. 停止后续 apply
2. 检查该 milstone 的 commit 是否引入回归
3. 必要时将问题 commit 拆分为更小单元重新 apply
4. 记录退化指标和根因（后续可作为 K/D 条目）

## 验证

- 全部 milestone apply 后，完整 benchmark 通过
- 与 baseline 对比，所有关键指标无退化
- `git log` 显示各 milestone 独立可回滚

## 注意事项

- **禁止一次 apply**：即使看起来"所有 commit 都相关"，也必须拆分验证。73.9x 退化在一次性 apply 后不可定位到具体 commit。
- **禁止删除 temp 分支**：临时分支保留作为增量融合的参考基线和回退点。
- **有依赖时放后面**：M0 提供 benchmark 能力后才能测 M1/M2 的效果 → M0 必须最前。
- **无依赖时放前面**：纯 trait 签名变更（M3）不改变内部行为 → 可以最后做，因为它需要先确认内部行为稳定。
- **bottoms-up 不是 iron law**：有依赖关系的必须按拓扑序；无依赖的可并行，但建议串行以隔离问题。
