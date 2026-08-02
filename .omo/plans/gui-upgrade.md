# GUI 全局升级实现计划 — epic #119

> **Epic**: #119 — gui: 全局界面升级 — 专业金融终端风格（设计先行）
> **Design**: `.omo/designs/gui-upgrade.md`（v2，Momus CONDITIONAL PASS 后修正，决策 D1-D12 + Q1-Q13 已锁定）
> **Worktree**: `.worktrees/gui-upgrade`（branch `feat/gui-upgrade`）
> **Status**: pending · 创建日期：2026-08-01
>
> **范围外**（已建独立 issue）：Q9 第三主题 compass_blue / Q10 Screener 重置按钮（#121）/ Q11 opener 打开目录（#122）。
> **锁定量**：compass-ui 独立 crate（16 atoms + 8 molecules，3 迁移）；theme 自主化（token→Visuals 直构，chart 薄封装）；
> egui_dock 0.20 `dock_style()` 深度定制；字体全内嵌（SourceHanSansCN Regular+Bold + JetBrainsMono Regular，+17.3MB）；
> 三栏布局（Sidebar 240px / DockArea / StatusBar 26px，工具栏 40px 四组）；Modal 三真实场景；窗口 1440×900；
> Tab 中文标题+图标；红涨绿跌（#EF5350/#26A69A）；`hline_below_active_tab_name`；`/` `Ctrl+Enter` `Ctrl+K`；200ms 重绘保持。

## 执行规则

- 批次切换：完成当前批次全部子 issue → 计划表状态更新 → 报告用户 → 用户确认后进入下一批次
- 并行：批次内无依赖子 issue 可并行（子 agent）；同一 worktree 一次一个 in_progress（S7→S8 同改 main.rs 强制串行）
- 每子 issue：测试先行（RED→GREEN）→ 实现 → commit（`ref #<sub-N>`）→ `/review-work`（最多 2 轮修复）
- 每批次结束：`/reflect` 追加 reflections.md（至少 epic 收尾一次）
- Push：用户明确要求后；push 前 `git fetch origin master` + rebase；push 成功后追加完成 comment 并关闭对应 issue
- 关闭：PR 合并 master 后 → 关子 issue（逐个 comment "Fixed by #<PR-N>"）→ epic 总结 comment → 关 epic

## Tasks

### Batch 1（P0 地基）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | | #123 | S1: workspace 挂载 + compass-ui crate 骨架 + 六类 design token + check-coverage.sh/testing.md/AGENTS.md 接线 | — |

### Batch 2（P0 静态层 — 三路并行）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | | #124 | S2: fonts.rs — 思源黑体 + JetBrains Mono 全内嵌注册（+17.3MB） | S1 |
| done | | #126 | S3: theme 自主化（token→egui::Visuals 直构 + chart 薄封装 apply_to_config + crosshair）+ dock_style 构建器（egui_dock 0.20 全字段覆写） | S1 |
| done | | #125 | S4: 基础组件 16 个（atoms：button/icon_button/input/dropdown/checkbox/tag/badge/status_dot/tooltip/empty_state/card/divider/label/price_text/segmented/section_title） | S1 |

### Batch 3（P1 复合层 — 两路并行）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | | #127 | S5: 迁移并增强 SearchableDropdown（键盘导航/空态）/ Toast（token 色+入场出场动画）/ Modal（动画+closing 状态机+Danger）至 compass-ui，bin 测试随迁（~39 断言） | S4 |
| done | | #128 | S6: 新复合组件 MultiSelect / DataTable（sort_rows 迁移）/ Toolbar / Sidebar / StatusBar（纯 UI，零业务依赖） | S4 |

### Batch 4（P2 集成 — 串行）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | | #130 | S7: 三栏式布局接线（main.rs ui()/render_toolbar 重写 + 1440×900 + fonts/dock_style 接线 + tab 中文标题图标 + 快捷键 + kittest 同步） | S2, S3, S4, S5, S6 |
| done | | #131 | S8: Modal 三真实场景 + watchlist 持久化（WatchlistConfig + save_watchlist_config）+ Chart 空态/symbol 回填 + Logger 导出 + Screener DataTable/MultiSelect 化 + 文档同步（gui.md/config.md/architecture.md）+ 反思 | S5, S6, S7 |

## 子 issue 明细

### S1 — compass-ui crate 骨架 + token 系统 + coverage 接线
- **文件**: `Cargo.toml`（members + workspace.dependencies: egui_dock="0.20", egui-phosphor="0.13", emath="0.35", egui_kittest dev）；`crates/compass-ui/Cargo.toml`；`crates/compass-ui/src/lib.rs`（`#![warn(missing_docs)]`）；`crates/compass-ui/src/tokens/{mod,color,spacing,typography,radius,shadow,motion}.rs`（`ThemeTokens` + `dark()/light()`，值逐项对齐设计 §4）；`scripts/check-coverage.sh`（+compass-ui）；`AGENTS.md` + `kb/dev/testing.md` 覆盖率清单
- **验收**: `cargo build/test -p compass-ui` 绿；token 测试断言 100% 字段；check-coverage.sh 输出含 compass-ui；clippy/rustdoc 干净
- **测试先行**: token 值断言（RED=编译失败）→ GREEN
- **依赖**: —

### S2 — 字体系统（fonts.rs）
- **文件**: `crates/compass-ui/assets/fonts/{SourceHanSansCN-Regular.otf, SourceHanSansCN-Bold.otf, JetBrainsMono-Regular.ttf}`（入库 +17.3MB）；`crates/compass-ui/src/fonts.rs`（`setup_fonts(ctx)`：Proportional=[思源,思源Bold,Default]+phosphor，Monospace=[JetBrainsMono,思源]；`include_bytes!` 内嵌，无运行期路径探测）
- **验收**: 字体族顺序断言测试绿；三字体入库；bin 不动（S7 接线）
- **测试先行**: 字体族断言（RED）→ GREEN
- **依赖**: S1

### S3 — theme 自主化 + dock_style
- **文件**: `crates/compass-ui/src/theme/{mod,apply}.rs`（CompassTheme{name,tokens}，接口兼容 theme.rs:24-109；`apply_theme` 直构 `egui::Visuals`+`egui::Style`，**不再调 apply_to_egui**；`apply_to_chart` 走 ChartSemanticTokens 覆写 + `apply_to_config` + crosshair）；`crates/compass-ui/src/dock_style.rs`（`dock_style(&ThemeTokens) -> egui_dock::Style`，值按设计 §6.1 表含 `hline_below_active_tab_name=true`）；`crates/compass/src/theme.rs` → `pub use compass_ui::theme::CompassTheme;`（测试迁入 compass-ui）
- **验收**: 色值/crosshair/dock_style 全断言绿；grep 无 `apply_to_egui` 调用；bin 编译过
- **测试先行**: 设计值断言（RED）→ GREEN
- **依赖**: S1

### S4 — 基础组件 16 个（atoms）
- **文件**: `crates/compass-ui/src/widgets/{mod,button,icon_button,input,dropdown,checkbox,tag,badge,status_dot,tooltip,empty_state,card,divider,label,price_text,segmented,section_title}.rs`；组件首参统一 `&ThemeTokens`；规格逐项按设计 §5.1
- **验收**: 每组件 ≥1 逻辑测试 + ≥1 kittest 渲染测试；零业务依赖（grep）；覆盖率 ≥80%
- **测试先行**: 逐组件 RED→GREEN
- **依赖**: S1

### S5 — 迁移 SearchableDropdown / Toast / Modal
- **文件**: compass-ui `widgets/{searchable_dropdown,toast,modal}.rs`（增强：键盘导航/空态；token 色+入场 150ms 出场 100ms 动画+左侧色条+280px；backdrop 120ms+panel scale 150ms+closing 状态机+Danger）；bin `widgets/mod.rs` 删除三模块；main.rs:28-30 import 改 compass-ui；~39 断言随迁
- **验收**: 两 crate 测试全绿；bin widgets/ 无残留；动画状态机测试过
- **测试先行**: 新动画/导航 API 断言（RED）→ GREEN；旧断言随迁
- **依赖**: S4

### S6 — 新复合组件 5 个
- **文件**: compass-ui `widgets/{multi_select,data_table,toolbar,sidebar,status_bar}.rs`（API 与数据模型见计划正文；DataTable 含 `sort_rows` 纯函数迁移；Sidebar/StatusBar 数据由调用方传入，零 chrono/零业务依赖）
- **验收**: 排序纯逻辑测试覆盖 screener 语义；交互事件（Select/DeleteRequest）kittest；覆盖率 ≥80%
- **测试先行**: RED→GREEN
- **依赖**: S4

### S7 — 三栏式布局接线
- **文件**: `crates/compass/src/main.rs`（setup_cjk_fonts 删除→compass_ui::fonts；viewport 1440×900；dock_style() 替换 L141/L371；ui() 三栏重构 L339-412；render_toolbar 四组重写 L443-537：Segmented/Dropdown/IconButton/Fetch Primary+spinner；快捷键 `/` `Ctrl+Enter` `Ctrl+K`；主题切换 toast）；`crates/compass/src/tabs.rs`（中文标题+图标 L57-63/L130）；kittest 同步（L905-1037）
- **验收**: 三栏 kittest 断言 + tab 中文 queryable + 快捷键仿真 + 1440×900 断言；全 workspace 测试/clippy 绿；真机冒烟（用户）
- **测试先行**: 新布局断言先 RED（旧测试改）→ GREEN
- **依赖**: S2, S3, S4, S5, S6

### S8 — Modal 三场景 + watchlist + 业务升级 + 文档
- **文件**: `crates/compass-core/src/model.rs`（WatchlistConfig）；`crates/compass/src/state.rs`（watchlist: Dynamic<Vec<String>>）；`main.rs`（save_watchlist_config 复刻 L263-288、启动引导 Modal、日志导出 file_dialog.save_file→写 state.log.logs、Sidebar 删除确认 Danger Modal、Sidebar 分组接线）；`citizens/chart.rs`（空态 EmptyState + symbol 回填）；`citizens/logger.rs`（SectionTitle+导出 IconButton）；`citizens/screener.rs`（Card 分区 + MultiSelect×3 + DataTable + 间距 token 化）；`kb/user/gui.md`（重写）、`kb/user/config.md`（[watchlist]）、`kb/design/architecture.md`（crate 图 + compass-ui）；`kb/dev/reflections.md`（/reflect）
- **验收**: 三场景 kittest + watchlist 往返测试；`cargo test`/clippy/fmt 全绿；coverage 含 compass-ui ≥80%；kb 同步提交；真机视觉确认（用户）
- **测试先行**: RED→GREEN
- **依赖**: S5, S6, S7

## 提交策略（Commit Strategy）

- 每个子 issue 一条 commit 链，链内按逻辑步原子提交，消息含 `ref #<sub-N>`（epic 子 issue 引用）
- 建议链内拆分（示例）：S4 = 每 3-4 个组件一个 commit；S7 = fonts/dock_style 接线 → 三栏骨架 → 工具栏 → 快捷键 → 测试迁移
- commit 后立即 `/review-work`（5 并行 agent），问题修复重 commit（最多 2 轮）
- 合并：epic 单 PR（base master），PR 合并后按 issue-workflow 阶段 4 批量关闭

## 文档同步清单（docs skill）

| 文件 | 变更 | 归属 |
|---|---|---|
| `AGENTS.md` | 覆盖率行加 compass-ui | S1 |
| `kb/dev/testing.md` | 覆盖率清单 L222-235 + compass-ui kittest 说明 | S1 |
| `kb/user/gui.md` | 全量重写（布局/字体/快捷键/Modal 场景/watchlist/红涨绿跌） | S8 |
| `kb/user/config.md` | `[watchlist]` 节 | S8 |
| `kb/design/architecture.md` | crate 图 L39-62 + 依赖方向 compass→compass-ui | S8 |
| `kb/dev/reflections.md` | 事后反思（/reflect，每批次或 epic 收尾） | 各批次末 |

## 成功标准（Success Criteria）

- `cargo build` / `cargo test`（workspace 全量）/ `cargo clippy` / `cargo fmt --check` 全绿
- `cargo llvm-cov --json --summary-only` + `scripts/check-coverage.sh 80` 通过（6 crate 含 compass-ui 均 ≥80%）
- `cargo doc --no-deps` 无 missing_docs 警告
- 真机 1440×900 冒烟：三栏布局、中文字形（思源）、mono 数字对齐（JetBrains Mono）、红涨绿跌、tab 中文+图标、四组工具栏、快捷键、三 Modal 场景、watchlist 持久化
- kb 五文件同步提交；reflections.md 反思条目存在
- PR 合并后：8 子 issue 关闭 + epic #119 关闭（含总结 comment）
