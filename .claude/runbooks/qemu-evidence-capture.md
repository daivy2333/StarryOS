# QEMU/手工证据采集统一命令行模式

- Status: active
- Last validated: 2026-08-27（依据 MS05 Iteration 011/004-rework 证据采集实跑、R44 证据精简原则）
- Environment: 任意 QEMU `-nographic` 手工验证（RISC-V virt；单 hart；user-net）；宿主 Linux shell
- Source: `ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/004-rework/`（`script`/`tee` 实跑记录）；R44 `qemu-network-testing.md`

## 适用范围

任何需要为 OpenSpec change 采集 QEMU 手工运行证据的操作：

- guest 完整串口（boot 签名 + 逐条输入 + 终态 PASS/FAIL marker）——用 `script`。
- host 侧命令输出（stimulus 脚本、ping、top、nc、编译）——用 `tee`。
- 二进制证据（`filter-dump` pcap 等）由工具直接产出后收集。

**不适用**：

- 自动驱动 QEMU guest shell——R44 硬性政策禁止，一律手工输入。
- 真板、SMP、PCI、性能基线的证据（使用各自 Runbook）。
- 与判定无关的批量长日志入库。完整串口是证明 boot、逐条输入和终态顺序的单一必要原始证据；
  其余日志只保存与判定有关的命令、输出和 marker 摘录。

## 前置条件

- 已确认目标 change 与 Iteration/Cycle 的 Evidence 路径
  （`openspec/changes/<change>/evidence/<iteration>/<cycle>`）。
- QEMU 命令与依据对应 Runbook（R44/R45/R48/R51/R56）已验证可启动到 `starry:~#`。
- `script` 与 `tee` 在宿主可用（coreutils/util-linux 自带）。
- 终端宽度足够，避免长路径被自动换行拆断（这也是 `$EV` 短变量的原因）。
- 无须 `sudo`（QEMU 与 HTTP server 均普通用户；TAP/mount 例外见对应 Runbook）。

## 操作步骤

### 1. 建立 Evidence 目标目录与短变量

每个需要采集证据的终端先执行：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/<change>/evidence/<iteration>/<cycle>
mkdir -p "$EV"
```

`<change>` 与 `<iteration>` 按当前 Cycle 实际路径填写；`$EV` 为当前终端会话内的短变量，
新开终端需重新设置。目标目录不存在时先 `mkdir`，避免 `script`/`tee` 报 no such file。

### 2. 录制 guest 完整串口：`script -q -e -f`

```bash
script -q -e -f "$EV/qemu-serial.log" -c 'make ARCH=riscv64 run'
```

要点：

- `-q`：安静，不打印 `Script started/exit` 噪声；`-e`：传播QEMU子进程退出码；`-f`：实时
  flush，日志边录边写可见。
- `-c '...'`：QEMU 命令行整体作为字符串传入；内部不再换行（避免多行拼接错误）。
- 使用 `make run`（默认 `LOG=warn`）启动，串口不出现 info/debug 调试信息刷屏；需要
  info/debug 分层诊断时单独按 R55 显式构建并恢复冻结镜像，不要把诊断镜像当采集基线。
- 必须**从启动开始录制**：只录 workload 摘录不能补证 boot 签名。
- QEMU 参数按各 Runbook 实际要求增删（hostfwd、filter-dump、tap 等），本模式只关心
  `script` 包裹方式，不固定 QEMU 参数内容。
- 任何 boot 签名 / probe / 回归都在这一个 session 内完成，串口即完整证据，天然对齐
  guest 侧执行顺序。

### 3. 采集 host 命令输出：`tee`

host 侧命令（stimulus、ping、top、nc 结果）需要留档时加管道：

```bash
set -o pipefail
python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-snapshot-host.log"
top -b -d 1 -n 30 -p <QEMU_PID> | tee "$EV/idle-cpu.txt"
```

要点：

- `tee` 同时显示到终端并落盘；必须先启用`pipefail`，否则pipeline只返回`tee`的状态，可能把前置
  stimulus失败误报为exit 0。判定时记录pipeline exit，必要时同时记录`${PIPESTATUS[0]}`。
- 一次命令一个文件；文件命名 `<模式>-host.log` 与 guest marker 一一对应，便于核对。
- 二进制输出（pcap 等）不用 `tee`（会损坏），由 QEMU `-object filter-dump` 等工具
  直接产出后再 `cp`/`mv` 到 `$EV/`。

### 4. 验证与摘录

```bash
rg -n 'MS05 (PASS|FAIL)|MARKER|exit' "$EV/qemu-serial.log"
rg -n 'PASS|FAIL|received=' "$EV/ms05-snapshot-host.log"
ls -l "$EV"
```

- 每个模式只能有一个终态 marker，且与对应 host 日志的共享计数一致。
- 按 R44 证据精简原则：Evidence 只保存能证明行为的命令、关键输出、marker 和退出码；
  不保存几百个日志、几万行原始日志或 hash 值。原始长日志留在外部并按需引用路径。
- 仅当该 change 明确声明 exact-binary 或要求机器可审计 provenance 时，才补充
  `stat -c '%y %s %n'`（记录 size/mtime）或 `sha256sum`。

## 验证

- `$EV/qemu-serial.log` 从 `starry:~#` 出现前的 boot 开始，含全部逐条输入与终态 marker。
- 每个 host 命令有对应 `$EV/*-host.log`，退出码与输出内容符合对应 Runbook 通过条件。
- `ls -l "$EV"` 文件齐全且非空；同一运行下 guest/host 日志可互相追溯。
- `script -e`传播的是QEMU进程退出码，只用于识别启动失败、崩溃或异常终止；它不等于guest workload
  退出码。workload以guest显式`PASS/FAIL`与`*_EXIT` marker为判据，两类状态都必须记录且不得互相替代。

## 失败处理

| 症状 | 原因 | 解决 |
|------|------|------|
| `script: ... : No such file or directory` | `$EV` 目录不存在 | 先 `mkdir -p "$EV"` |
| `tee: ... : No such file or directory` | 同上 | 同上 |
| `EV: command not found` | 新终端没重设 `$EV` | 每终端独立执行步骤 1 |
| 长路径被终端换行拆断 | 终端宽度不足 | 始终用 `$EV` 短变量；保留意外文件，核对后合并，避免重跑覆盖首次失败 |
| 录制的串口缺 boot 签名 | `script` 在 shell 出现后才启动 | 必须从 QEMU 启动开始录制，重开 session |
| 命令里 `-c '...'` 含单引号冲突 | 引号嵌套 | 外层用单引号时内部不出现单引号；必要时改用双引号并转义 |

## 回滚

- 本模式只生成 `$EV/` 内日志与 `/tmp` guest 文件，不修改产品源码。
- guest `/tmp` payload 随 QEMU 退出丢失，无需清理。
- 退出QEMU用`Ctrl-A X`，停止host命令用`Ctrl-C`。若怀疑进程残留，先用`pgrep -af
  qemu-system-riscv64`或`pgrep -af <stimulus>`核对完整命令，再对确认的单个PID执行`kill <PID>`；
  不使用可能误杀其他会话的宽泛`pkill -f`。
- Evidence 文件误生成时直接删除对应 `$EV/` 文件；不影响已冻结的既有 Evidence。

## 证据

- 模式来源：`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/004-rework/`
  （`qemu-serial.log` + 各 `*-host.log` 实跑记录，六模式全部 PASS）。
- 已采用本模式的既有 Runbook：R51（ms04）、R56（ms05）；本模式同步推广到
  R44（qemu-network-testing）、R45（ms02）、R48（ms03）。
- 精简原则：R44 `qemu-network-testing.md`「证据精简原则」（2026-08-19 起）。
- 适用限制：本模式只规定证据/日志的采集命令行形式，不改变对应 Runbook 的功能
  判据、QEMU 参数或 R44 手工政策。
