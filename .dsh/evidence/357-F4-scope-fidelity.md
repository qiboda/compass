# F4 范围保真 — issue #357（设计 §11 组 A-E 逐组核对 + 实现偏差记录）

证据时点：2026-09-06（阶段 0-5 + review 修复全部落地后）

## 设计契约核验（design v7 vs 实现）

| 组 | 契约 | 实现位置 | 状态 |
|---|---|---|---|
| A 类型骨架 | EditorKind 6 变体 serde snake_case/EDITOR_REGISTRY 6 条/EditorLayout（Header 恒有/Sidebar 恰 2=Chart+Screener/Toolbar None）/WorkspaceId 3 变体/Workspace/Workspaces+switch+default_layout/EditorView/EditorCtx/EditorFrame/EditorInstances/WatchlistEditor | editor/mod.rs；requirement 33/0（含 registry 契约测试） | ✅ |
| B Workspace 容器/Topbar | Segmented 3 段/⋮ 添加编辑器（visible_kinds 缺失列表）/主题语言最右/picker 中/tab 栏=编辑器切换器 | main.rs topbar + cb3fc1b；kittest toolbar_workspace_switch/add_editor_menu | ✅ |
| C 每编辑器迁移 | 2a-2f 六编辑器 header/sidebar/body 拆解（Chart 周期/复权/指标 + Fetch 右端 + 显示选项；Screener 条件构建器 sidebar 独立 500/(486,640)；SEPA/Market 计数+Segmented+刷新+⋮重置排序；Logger SectionTitle+导出；Watchlist 搜索+列表） | chart/screener/sepa/market/logger/watchlist 各 impl EditorView；kittest 每编辑器 ≥2 | ✅ |
| D N 键 | Key::N+editing_text 守卫+focused_leaf→last_interacted_kind 回落+EDITOR_REGISTRY sidebar 判定（Some 翻转/None 无操作）+Display Options 双向入口 | handle_shortcuts + sidebar_visibility + tabs.rs 消费 + chart display_options_menu | ✅ |
| E 注释/规范 | doc 注释/决策记录 | 各 commit | ✅ |

## 已知偏差 / 文档勘误记录（均已归档）

1. **split fraction 语义**：egui_dock 的 fraction = 左/上（首子）节点份额（show/mod.rs 实测）；plan §0.3 A2/§3.1 文本「0.75」与 design §5.1「split_right 镜像法」表述与实现相反——实现正确（split_left(root, 0.25, [Watchlist])；split_below(root, 0.75, [Logger]) 的 0.75=Chart 区域高度份额）。design v5+ 业已含 A2 correction 记录与 §9.1 fraction=a 份额定稿（v7 无变化）；**plan 文本未勘误**（plan 文本归 subagent_architect——F4 遗留：plan §0.3/§3.1 数字待 architect 勘误）
2. **egui_dock#197**：0.20.1/0.21.1 直接 DockState<Tab> serde round-trip 均崩（Rect::NOTHING ±inf → null；上游 node/mod.rs:145，0.21.1 未修）→ 持久化走本项目 DockTopology 自定义拓扑（design §9.1 v6 裁决：升级后实测仍崩，结论维持）。serde feature 保留（TabPath 类型依赖 + 未来退路）。phase 0 单测 + zeroth-ignored 验证绿
3. **toml 0.8 嵌套数组限制**：[layout] 的 [[layout.workspaces]] 嵌套 array-of-tables 无法经 toml::from_str 反序列化为 Vec 字段 → load_config 改 Value 层解析 + layout_section_from_doc 手动提取；保存器输出内联数组（读回两种形状兼容）——design v7「存储格式注记」+ §13 决策记录已含
4. **EditorFrame::show 第三参 impl→dyn**：trait object 接收形态（EditorView trait 4 方法形状未变）；设计 §4.3 回写交 designer（6ad85e19 v6 已处理）
5. **TabKind 删除契约 vacate**：A3 收官（41923b0）；对抗测试 tabkind_serde_snake_case_guard 按其注释 vacate（无 TabKind 时 pass）；requirement_index_market tab_kind_gains_market_variant 双形态断言（tab.market||editor.market）+supersede 注释（SHA 41923b0）
6. **requirement topbar 键断言放宽**：键绑定在 WorkspaceId::title_key()（editor 模块），main+editor 源并集断言（合理性：键字符串在 editor/mod.rs）
7. **eframe harness pointer click 不可靠**（部分 widget）→ 统一 click_accesskit（注明于测试注释；startup modal 等既有 pointer click 先例说明是组合问题）
8. **Sidebar widget 层 240px**：design v7 裁决 2 = Sidebar::show() 完整复合容器默认 240 保留（内容可读性下限）；Q6「无固定宽度」= 布局层无初始默认宽（Watchlist dock leaf 宽度由 split 比例持久化）——非偏差，裁决已落档
9. **设计文档行号漂移**：design v6/v7 已多次校正（f7144a7c 40 处 + v7 §9.1/§9.2）；§6/§13 后续漂移按 v6 总注「修订时点锚定」口径管理（doc-sync 收尾再核对一轮，designer）
10. **范围裁剪**：列宽重置（DataTable 无列宽 API）暂缓——designer 裁决明确（P1-2 暂缓+随列宽功能上线）；Screener/SEPA/Market ⋮ 菜单动作以「重置排序/清除结果」落地（裁决 183698df/015c0914）
11. **egui 0.36 全栈升级**：用户授权（fork_push=两个 fork 推 origin：qiboda/egui-charts@b0ed1a3、qiboda/egui-phosphor@b6f9ff6 已推 origin 验证）；作为 #357 一部分独立 commit f33ab39
12. **测试构建器 layout_fp 预置（review 6757f35b P1-1）**：全 harness kittest 首帧 None != Some(指纹) 误判 dock 编辑，把 [layout] 写进真实 ~/.config/compass/config.toml（3c8ba99 的 startup_restores 测试曾把 active_workspace="sepa" 持久化）→ 4ed2bdc 修复 = build_compass_app 预置 layout_fp（与生产 main() 一致）+ layout_fingerprint 改 pub(crate)；真实 config.toml 已修复（损坏尾巴+污染 [layout] 移除，备份 .bak-357）；**P1 类测试侧缺陷，非产品偏差**
13. **plan §7.1 双判定补实现**：1/2/3 原实现仅 workspace 判定——补 chart_editor_active()（Chart workspace AND focused kind == Chart），Ctrl+K 补 watchlist_leaf_open() 门控（plan 契约落实，非勘误）
14. **Screener ⋮ 补侧栏切换项**（design §8.2 鼠标/键盘双入口补全——原仅 Chart 落地；4ed2bdc）
15. **sidebar widget min_width 参数化**：design v7 裁决 2 标注「未落地」→ 496fc60 落地（A 选项：search_row/show_list 显式 min_width，WatchlistEditor 传 ui.available_width()；ui.md 决策记录行同步更新待 designer 收尾）
16. **review 37c81bfe 修复落位**：P1-1 input `.max(80.0)` 守卫（design 契约明文）；P1-2 恢复焦点 = resolve 侧按 workspace id 聚焦（dock_state_from_topology 保持纯重建——首个 pre-order leaf 是 Watchlist 非主编辑器，语义归调用方）；P1-3 save_layout_config cfg(test) 围栏 + 持久化测试 opt-in（COMPASS_TEST_PERSIST_LAYOUT）——**测试侧防护，非产品偏差**
