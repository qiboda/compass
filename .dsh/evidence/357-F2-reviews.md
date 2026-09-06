# F2 审查—issue #357（每 commit → review，全部结论处理）

| 审查对象 | 子代理 | 结论 | 处理 |
|---|---|---|---|
| 8a5ea19 阶段 0 | 41c49a34 | APPROVE-WITH-NITS（P2×2 断言弱） | requirement P2-1/P2-2 断言限定 tabs.rs（ca5d480 前奏） |
| ca5d480 阶段 1 | 168c2914 | REQUEST-CHANGES（P1-1 citizen_id Option 契约；P2×2 文本/时点） | dd66ac6（P1）+ 6374bd8（P2-2）；P2-1（plan/design 文本勘误）→ F4 #1 |
| 12901bb 2a | b284f5eb | APPROVE-WITH-NITS（P2-1 图例守卫） | 3d3a162 修复 + cd4fddf 注释 trivial |
| 3d3a162 2a 修复 | 29090927 | APPROVE-WITH-NITS（P3 一项一行） | cd4fddf 顺手修 |
| df44802 2b | 70ebfccf | REQUEST-CHANGES（P1-1 Panel id clash；P1-2/1-3 设计裁决；P2×3） | 0bb2212（P1-1/P2-2/P2-3/P3-2/3-3）；P1-2 列宽暂缓+c8f03d6（500/(486,640) 按 designer 裁决） |
| 0bb2212+c8f03d6 | 71915ac2 | APPROVE-WITH-NITS（P2-1 doc-sync 待办） | 阶段 5 doc-sync 覆盖；P3 注释修复 |
| 5341806 2c | 015c0914 | APPROVE-WITH-NITS（P2-1 接线 kittest 缺） | b8b1ce2 修复 |
| 1dd8239 2d | 748669ff | APPROVE-WITH-NITS（P3×4，全处理） | 7dd10d9 + P3-3 行号交 designer（f7144a7c 校正） |
| 1640e40 2e | d1c6f951 | APPROVE | — |
| f19de7f 2f | f08681fd | APPROVE（P2-1 阶段 3 前不可达/中间态） | 6197dfb 后恢复 |
| 6197dfb/cb3fc1b/41923b0 阶段 3 | 94e1992b | 修改后通过（P1 requirement_index_market 转红；P2 add_editor 落点；P3×6） | 130b4bc 修复全部 |
| f33ab39 升级 | d4d76dca | APPROVE-WITH-NITS（P2 DockArea 盐/fork pin/KB） | 130b4bc（盐+pin）+KB 留 doc-sync |
| 130b4bc | 26b1a84c | APPROVE-WITH-NITS（P2-1 初始焦点残留） | 055b970（focus_main_leaf+落地叶守卫） |
| 055b970 | 28e54c4f | APPROVE（P3×5，P3-2 改 assert_eq 落地叶） | 1e2b203 |
| 64dcae2 阶段 4 | 41e74cc0 | REQUEST-CHANGES（P1-1 active_screen 越界；P2-1 级联回退/设计张力；P2-2 kittest；P3×8） | 3c8ba99 修复 P1/P2-①(P2-2 kittest×2)/P3-3/4/7；P2-1 设计勘误→designer v7（双层语义+13 决策 22 行）；P3-2 写盘形状注记 v7；P3-5/6/8 接受记录 |
| 3c8ba99+f0cce32 | 6757f35b | REQUEST-CHANGES（P1-1 测试写真实 config；P2×2 双判定/Ctrl+K 门控；P3×5） | 4ed2bdc 修复 P1-1（layout_fp 预置）+P2-1/P2-2（chart_editor_active/watchlist_leaf_open）+P3-1/2/3；P3-4 行号交 designer v8；P3-5 取舍入设计 §13 |
| 496fc60+4ed2bdc | 37c81bfe | REQUEST-CHANGES（P1-1 .max(80) 守卫；P1-2 恢复路径焦点回归；P1-3 save 围栏不完整；P2×2 测试弱断言；P3×3） | 4588a97 修复全部（P1-1 守卫+测试；P1-2 resolve 侧 focus_main_leaf+回归测试；P1-3 cfg(test) 围栏+opt-in；P2 宽度/事件断言；P3 separator/len==2/注释） |
| 4588a97 | 81e84e15 | REQUEST-CHANGES（P0-1 save 围栏未落地；P1-1 debug_assert panic 面；P2×3；P3×1） | 4de9ab8 修复全部（真实 fence+opt-in 补全；fail-soft+无 main kind 测试；预算公式断言；narrow 底板断言；dots 注释） |
| 4de9ab8 | 复核委派中 | — | — |
