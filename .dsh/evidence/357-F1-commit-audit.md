# F1 合规审计 — issue #357 编辑器架构重构

证据时点：2026-09-06（阶段 0-5 + 阶段 4/5 review 修复轮全部落地后）
分支：feat/editor-architecture；worktree .worktrees/editor-architecture；base 0182dcb（master @ Merge #356）
commit 总数：32（0182dcb..HEAD 逐条核实；表内 32 行与 git log --oneline 一致）；每 commit message 含独立成行 `ref #357`（逐条 grep 验证，无缺失）。

## Commit 清单（含 ref 校验）

| commit | 内容 | ref |
|---|---|---|
| 8a5ea19 | 阶段 0 类型骨架 + egui_dock serde feature + 对抗/需求测试 | ✅ |
| ca5d480 | 阶段 1 映射骨架（default_layout 三树） | ✅ |
| dd66ac6 | 阶段 1 review 修复（citizen_id Option 契约） | ✅ |
| 6374bd8 | 阶段 1 review 修复（phase3 guard 改名/时点） | ✅ |
| 12901bb | 2a Chart 迁移 | ✅ |
| 3d3a162 | 2a review 修复 | ✅ |
| cd4fddf | 2a review 注释 trivial | ✅ |
| df44802 | 2b Screener 迁移 | ✅ |
| 0bb2212 | 2b review 修复 | ✅ |
| c8f03d6 | Screener sidebar 500/(486,640)（designer 裁决落地） | ✅ |
| 5341806 | 2c SEPA 迁移 + 重置排序 | ✅ |
| b8b1ce2 | Screener ⋮ 重置排序 + 2c review 修复 | ✅ |
| 8888f44 | 设计 v4 归档（designer 裁决） | ✅ |
| 1dd8239 | 2d Market 迁移 | ✅ |
| 7dd10d9 | 2d review 修复 | ✅ |
| 1640e40 | 2e Logger 迁移 | ✅ |
| f19de7f | 2f Watchlist 迁移 | ✅ |
| 6197dfb | 阶段 3a Workspace 容器 | ✅ |
| cb3fc1b | 阶段 3b Topbar 改造 | ✅ |
| 41923b0 | 阶段 3c TabKind 删除 | ✅ |
| f33ab39 | egui 0.36 全栈升级（用户授权 #357 一部分） | ✅ |
| 130b4bc | 阶段 3 review + 升级 review 修复 | ✅ |
| 055b970 | 130b4bc review 修复（focus_main_leaf） | ✅ |
| 1e2b203 | 055b970 P3 清理 | ✅ |
| 64dcae2 | 阶段 4 [layout] 持久化（拓扑 serde） | ✅ |
| 3c8ba99 | 阶段 4 review（41e74cc0）修复 | ✅ |
| f0cce32 | 阶段 5 N 键/快捷键上下文化/i18n/Display Options 入口 | ✅ |
| 496fc60 | design §6 裁决 2 落地：Sidebar widget min_width 参数化（A 选项） | ✅ |
| 4ed2bdc | review 6757f35b 修复（P1-1 layout_fp 预置/P2 双判定/P3 步进与菜单） | ✅ |
| 4588a97 | review 37c81bfe 修复（P1-1 宽度守卫/P1-2 恢复焦点/P1-3 save 围栏 + P2/P3） | ✅ |
| 4de9ab8 | review 81e84e15 修复（真实 save 围栏/fail-soft 焦点/预算公式断言） | ✅ |
| deb3e2b | review 41bf85d2 清理（恒真断言替换/死代码删除；amend 修正 message） | ✅ |

## 流程合规

- **Never auto-push**：全程零 push（origin 无该分支更新；远端状态待用户"push"指令）
- **每 commit → review**：阶段 0-5 各轮 review 均已委派并处理（结论见 F2）；6757f35b REQUEST-CHANGES → 4ed2bdc 修复；37c81bfe REQUEST-CHANGES → 4588a97 修复；81e84e15 REQUEST-CHANGES → 4de9ab8 修复；41bf85d2（4de9ab8）**APPROVE**；deb3e2b 为 trivial 测试清理（跳过 review）
- **分支/worktree**：全部实现提交落在 feat/editor-architecture（worktree），master 无混入
- **test-first**：阶段 0/1 的对抗+需求测试先于实现提交（8a5ea19 含），逐阶段 RED→GREEN 演进；阶段 4/5 遗留 RED 均在对应阶段转 GREEN（requirement 33/0）
- **用户关键指令**：主 agent 未自行修改设计/plan 文本——设计文档修订均由 subagent_ui_designer 完成（v2/v3/v4/v5/v6/v7 记录），plan 文本由 subagent_architect（v2）；实现偏差（split fraction 语义、TabKind 删除契约 vacate、impl→dyn）归档 F4
