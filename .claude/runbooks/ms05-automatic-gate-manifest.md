# MS05 Automatic Gate Manifest 管线

- Status: active
- Last validated: 2026-08-15
- Environment: Linux sandbox；Python 3.10；cargo（`--locked --offline`）；riscv64-linux-musl-gcc（`/opt/musl/riscv64-linux-musl-cross/bin`）；openspec CLI；git `net-k3` @ `8dc3ef7d`
- Source: Iteration 010 / Cycle 000 Act Response（`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/iterations/010-deadline-closed-probe-and-evidence-pipeline/000-initial.md`，status `reported`）与 `evidence/010-deadline-closed-probe-and-evidence-pipeline/000-initial/`

## 适用范围

用于 MS05（QEMU 有界双向数据面）在 Iteration 010 及之后的自动产品 Gate 与资格证据生成：`ms05_evidence_capture.py` 以 literal argv 运行完整 Gate 集合并生成版本化 manifest，`ms05_evidence_audit.py` 校验 manifest 并绑定 qualification。适用对象是后续 change 复跑自动 Gate 或需要机器生成、可审计的 runtime Evidence 的场景。

不适用于：手工 QEMU runtime（Iteration 011 任务 6.1-6.3）、性能 benchmark（使用 MS16 `network_benchmark_*` 管线）、或需要修改产品 kernel/driver/ABI/wire 的场合（本管线明确不改产品接口）。

## 前置条件

- 目标 change 的 `Plan Context` 为 `ready`，`Persisted Evidence` 为 `required`。
- 工具链可用：`make`、`cargo`（offline 缓存含全部依赖）、`rustfmt`、`openspec`、RISC-V musl 交叉工具链（`BENCH_CC`）。
- 源文件已最终定稿并 `git add`（`git diff --cached --check` 必须 exit 0，staged 缺陷会使 `diff-cached-check` Gate 失败）。
- Evidence root 目录不存在或可安全覆盖（capture 会重建 `manifest.json`、`logs/`、`artifacts.sha256`）。
- 产品代码与 source freeze 文件在 capture 运行期间不被编辑。

## 操作步骤

1. **source 定稿并 stage**：`git add` 全部 change-owned 源文件（probe、test、scripts、Makefile），确认 `git diff --check` 与 `git diff --cached --check` 均 exit 0。
2. **focused suite（source freeze 前）**：运行 Plan `Verification` 列出的定向套件——strict C syntax、harness、Python self/loopback、`unittest tests.test_ms05_evidence_tools`、capture/audit `--self-test`、`make host-test`、静态 payload build。
3. **自动 manifest**：
   ```bash
   python3 scripts/ms05_evidence_capture.py --run automatic \
     --root openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/010-deadline-closed-probe-and-evidence-pipeline/000-initial
   ```
   runner 按 `GATES` 声明顺序以 literal argv 执行每个 Gate，写 `logs/<gate>.log`（stdout+stderr + `# ms05 capture exit=N` trailer），100× Gate 写 `logs/<gate>/NNNN.log` 子记录，source freeze 在 Gate 前记录，六个 artifact 记录 `file/stat/sha256sum`。
4. **写 qualification**：
   ```bash
   python3 scripts/ms05_evidence_audit.py --root <root> --write-qualification
   ```
   运行 14 个负向 fixtures（每个必须返回精确错误码）后做正向 audit，写 `evidence-audit.log`、`qualification.json`、`env-blocked.json`。
5. **验证绑定**：`python3 scripts/ms05_evidence_audit.py --root <root> --verify-qualification`。
6. **artifact 复读**：`sha256sum -c <root>/artifacts.sha256`。
7. **收尾 Gate**：`openspec validate <change> --strict`；`git diff --check -- . ':(exclude).../evidence/**'` 与 `git diff --cached --check -- . ':(exclude).../evidence/**'`。
8. **生成派生索引**：从 `manifest.json` 派生 `README.md`；人工写 `red/`（pre-fix RED 记录）与 `review.md`（specs-vs-code + full diff review）。

## 验证

| 判据 | 命令 | 通过条件 |
|---|---|---|
| 自动 manifest | capture `--run automatic` | 所有 record `classification == "pass"`；3×100× Gate 各有 100 个 child 且 exit 0 |
| 负向 fixtures | audit `--self-test` / `--write-qualification` | 14 个 fixture 各返回命名错误码（`MISSING_LOG`、`EMPTY_LOG`、`LOG_HASH_MISMATCH`、`MISSING_ARGV`、`MISSING_TIME`、`MISSING_EXIT`、`UNSUPPORTED_CLASSIFICATION`、`INCOMPLETE_CHILD_SET`、`SOURCE_AFTER_FREEZE`、`ARTIFACT_MISMATCH`、`D1_DIAGNOSTIC_COUNT`、`D1_UNCLASSIFIED_ERROR`、`REQUIRED_GATE_MISSING`、`GATE_ORDER`） |
| qualification 绑定 | `--verify-qualification` | 输出 `qualification binding VERIFIED` |
| artifacts | `sha256sum -c artifacts.sha256` | 6/6 `OK` |
| D1 Gate | manifest 中 `kernel-lichee-d1-check` | exit 101 且恰好 20 个 `error[E0432]` + 5 个 `error[E0433]`，无未分类 error |
| source freeze | audit 正向检查 | frozen 文件 hash 零漂移 |
| 最终 diff | 双 diff check | 均 exit 0 |

## 失败处理

- **audit 报 `EMPTY_LOG`**：静默成功的 Gate（如 `rustc --test` compile）原本产生空日志；capture 的 `write_log` 已追加 trailer 保证非空。若仍触发，检查 capture 是否用了旧版（无 trailer）。
- **artifact mtime drift**：任何在 capture 之后运行的 `make -B`（如 focused suite 重建 payload）会改变 artifact mtime，使 `audit_artifacts` 失败。修复：所有构建完成后**重跑一次 capture + qualification**，使 manifest 记录与磁盘一致。内容 hash 不变不代表 mtime 一致。
- **source-freeze 漂移**：capture 之后编辑任何 frozen 源文件会使 audit 报 `SOURCE_AFTER_FREEZE`。修复：重新 `git add` 并重跑 capture。
- **D1 计数不匹配**：`kernel-lichee-d1-check` 依赖 `lichee-d1` feature 的既有诊断分布；若 kernel 代码变化导致计数漂移，属产品 Gate 失败，需返回对应产品任务，不得归为环境。
- **负向 fixture 返回错误 code**：audit 修改后自检必须仍通过；fixture 必须精确命中预期错误码，任何无关 AuditFailure 都算失败。
- **`git diff --cached --check` 失败**：staged 内容有 whitespace 缺陷（历史案例：probe 尾部空行）；重新 `git add` 修正后的文件。

## 回滚

- Evidence 是执行产物：删除或重建 `<root>` 目录（`manifest.json`、`logs/`、`artifacts.sha256` 由 capture 重生成；`evidence-audit.log`、`qualification.json`、`env-blocked.json` 由 audit 重生成）即可重跑，不修改产品代码。
- 产品代码如需回退：`git checkout -- <path>` 恢复工作树，随后重跑 source freeze 与整条管线。
- `qualification.json` 一旦写入不可手工覆盖；任何 audit 重跑都会重新生成绑定，必须重新 `--verify-qualification`。

## 证据

- Act Response：`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/iterations/010-deadline-closed-probe-and-evidence-pipeline/000-initial.md`（reported，2026-08-15）。
- Evidence：`evidence/010-deadline-closed-probe-and-evidence-pipeline/000-initial/`（`manifest.json` 44 records / 6 artifacts、`logs/`、`red/` 4 条 RED、`artifacts.sha256`、`evidence-audit.log`、`qualification.json` verdict PASS、`README.md`、`env-blocked.json`、`review.md`）。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件/设备模型；不构成 SMP、真板、DMA/cache 或性能证据。
