## 1. Source Baseline

- [x] 1.1 在 `historical-carriers.sha256` 登记历史 MIG/ARC 文件路径和 SHA-256，以便后续检测来源变化；预期所有历史 carrier 保持不可变
- [x] 1.2 在 `active-originals/` 与 `active-sources.sha256` 完整保存 `openspec/project.md` 和旧 tasks，并记录 CLAUDE/SNAPSHOT 排除项及用户未跟踪目录边界；预期迁移范围可审计

## 2. Unit Migration

- [x] 2.1 全文读取历史原文与 carrier，按 design 的单元边界生成 coverage checklist；预期每个单元都有定位锚点和 hash
- [x] 2.2 为每个来源单元建立 M/D/K/R/I/MS/T/文档目标和 Legacy ID 映射；预期 `unmapped = 0`、`skipped = 0`
- [x] 2.3 合并当前权威目标并更新活动旧路径/旧编号引用；预期不覆盖目标独有内容

## 3. Framework Upgrade

- [x] 3.1 合并 OpenSpec config 和五类 spec，重建 SNAPSHOT/CLAUDE/iteration 模板并合并 tasks/AGENTS；预期符合最新角色与 Gate
- [x] 3.2 配置 Claude Code、Codex、OpenCode 共用技能入口；预期技能正文只有一份且 frontmatter 仅含 name/description

## 4. Verification and Archive

- [x] 4.1 执行正向/反向核对、来源 hash 复算、OpenSpec validate 和结构 Gate；预期 source=mapped=verified 且全部验证通过
- [x] 4.2 将完整统计、编号映射、验证证据和恢复入口写回 carrier；预期 Archivist 可独立核验
- [x] 4.3 固化唯一 OpenSpec archive 命令与归档后退出/扫描协议；实际 Archive 是本 change 的终止动作，成功后按 proposal 的 Active Exit List 精准完成
