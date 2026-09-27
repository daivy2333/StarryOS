# Git Stash 二分排查

- Status: active
- Last validated: 2026-08-10
- Environment: git worktree + cargo workspace；Rust nightly-2026-02-25；Makefile 构建
- Source: `ms04-qemu-async-rx-queue-baseline` iter 000 实施排查过程；用户手动 `make run` 验证

## 适用范围

大型改动的构建/测试失败，需要判断**失败是否由本次改动引入**时使用。典型场景：

- 一次大改动（多 crate patch、kernel 修改、新增文件）后构建失败，无法从错误信息直接定位。
- 需要回答"基线是否本来就能构建 / 是哪个改动块引入的失败"。
- 改动横跨多个独立子系统（依赖、内核、工具链配置），怀疑存在交互。

不适用于：单文件小改动（直接 revert 验证即可）、错误信息明确指向某文件（直接看该文件）。

## 前置条件

- 改动未提交或可安全 stash（工作区无不可丢失的未跟踪关键产物）。
- 确认**所有**要 stash 的文件已被 git 跟踪：未跟踪文件（新文件）默认不进 stash，必须 `git add -A` 后才能连同 stash。
- 记录 stash 前的 `git status --short` 完整快照，作为恢复对照基准。
- 如改动含新文件，stash 前确认没有同名 untracked 冲突。

## 操作步骤

### 1. 建立完整改动快照

```bash
git status --short          # 记录全部改动（含未跟踪）
git stash list              # 记录已有 stash，避免编号混乱
git add -A                  # 把未跟踪新文件也纳入暂存（关键！）
git stash push -m "ab-test" # 全部改动入 stash，工作区回到 HEAD
git status --short          # 确认工作区干净（只剩 HEAD 状态）
```

### 2. 验证基线（A 侧）

```bash
cargo clean                 # 清 target，排除增量缓存污染（关键！）
make LOG=info build         # 或对应构建命令
```

- 基线 PASS → 问题由改动引入，进入二分。
- 基线 FAIL → 先记录基线错误，再判断是否预存（仍需二分确认哪些改动无关）。

> ⚠️ **陷阱：target 缓存污染**。stash 后如果不 `cargo clean`，残留的增量编译产物
> （rlib/rmeta）可能与 HEAD 源码不匹配，导致基线 build 假失败（本次
> `__start_debug_abbrev` 误判为预存的直接原因）。`cargo clean` 是必须步骤。

### 3. 二分恢复（B 侧）

按依赖关系把改动分成互斥的块，逐块恢复并验证：

```bash
# 恢复第一块（例如仅 Cargo patch 相关）
git checkout stash@{0} -- Cargo.toml Cargo.lock crates/axdriver_net crates/axdriver_virtio crates/virtio-drivers
make LOG=info build        # 或对应验证
```

- 此块 PASS → 问题不在此块，恢复下一块。
- 此块 FAIL → 问题可能在此块，缩小块内范围继续。

### 4. 用户/独立验证交叉确认

agent 的构建结果可能与用户环境不同（权限、终端、缓存）。对疑似环境噪声的结论，用
**用户手动执行同一命令**（如 `make run` 看 QEMU 是否启动）做交叉确认。agent 的
build FAIL + 用户的 run PASS 组合 → 判定为环境/缓存噪声，非改动引入。

### 5. 收尾恢复

```bash
# 逐个恢复所有块
git checkout stash@{0} -- <块1路径> <块2路径> ...
# 校验 stash 里所有文件都已恢复（含新文件、symlink）
for f in $(git stash show stash@{0} --name-only); do
  [ -e "$f" ] || [ -L "$f" ] || echo "MISSING: $f"
done
git stash drop stash@{0}  # 确认全部恢复后丢弃
git status --short         # 与步骤 1 快照对比，确认内容一致
```

## 验证

| 判据 | 命令 | 通过条件 |
|---|---|---|
| 基线干净 | `git status --short`（步骤 1 后） | 无输出或只剩 HEAD 无关文件 |
| 基线真实 | `cargo clean && make build` | 结果可解释（PASS 或明确基线错误） |
| 二分隔离 | 每块恢复后单独 build | 每块结果独立可判 |
| 交叉确认 | 用户手动 `make run` | QEMU/产物正常启动 |
| 恢复完整 | 对比步骤 1 快照 | 文件集合与内容一致 |
| 无残留 | `git stash list` | 临时 stash 已 drop |

## 失败处理

- **stash 报 `paths did not match`**：路径含删除状态或不在 stash 中；用 `git stash show --name-only` 核对实际文件列表，逐个恢复。
- **stash drop 后才发现漏恢复**：立即 `git fsck --lost-found` 或 `git reflog` 找 dangling stash 对象；恢复前不要执行其他 git 写操作。
- **`git checkout stash@{0} -- dir` 中途失败**：一个路径失败会让整条命令不恢复任何文件；拆成单条路径逐个执行。
- **恢复后测试仍失败**：不要继续扩大猜测，回到步骤 2 重新 clean 基线，确认不是缓存叠加问题。
- **staged 状态干扰**：`git add -A` 会让恢复的文件处于 staged；`git status` 显示 `A`/`M` 是正常的，内容正确即可，提交前按意图重新分组 staging。

## 回滚

- 排查过程本身不改动 HEAD 源码（只改工作区/index）。
- 出现意外状态时：`git reset` 回退 index，`git checkout -- <路径>` 丢弃误恢复，或 `git stash pop` 立即还原全部改动。
- stash 被误 drop：`git fsck --lost-found` 找回，`git stash apply` 指定对象恢复。

## 证据

- 来源：`ms04-qemu-async-rx-queue-baseline` iter 000 排查（2026-08-10）。
- 关键事实：stash 全部改动后基线 `make LOG=info build` 假失败（`__start_debug_abbrev`
  undefined），因未 `cargo clean`；用户 `make run` 基线正常启动推翻预存判断；分块恢复
  Cargo patch 与 kernel 改动后用户 `make run` 均正常，确认失败为环境/缓存噪声。
- 适用限制：本流程用于判断"改动是否引入失败"，不替代真实调试；若二分的每一块单独都
  PASS 而组合 FAIL，是交互问题，需换维度（如 feature 组合）继续二分。
