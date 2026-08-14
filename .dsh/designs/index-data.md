# 指数数据 GUI 展示方案（大盘 · 板块）

**状态**：过程归档（ui-designer 产出，待用户确认；确认后要点同步至 `kb/design/ui.md`）
**日期**：2026-08-13
**关联**：worktree `add-index-data`（handoff 已锁定 9 项决策：三类标的、本地 Parquet、
日线+周/月聚合、GUI 大盘/板块展示 + 板块轮动 + SEPA 评分 + 回测基准）

---

## 目标

为 compass GUI 增加**指数数据展示**能力，覆盖四项核心用途中的 GUI 侧：

1. **大盘指数展示**：上证指数 / 深证成指等官方指数走势，可在现有 K 线图表中查看
2. **板块展示**：概念板块 / 行业板块（约 500 只）的**行情排序列表**（板块轮动视角）
   + 任意板块的**板块指数 K 线**
3. **导航与搜索**：从股票平滑切换到指数/板块，可搜索全部 500 个指数/板块标的
4. **交互一致性**：hover / 日周月切换 / 行点击联动与现有股票图表完全统一

约束（沿用项目铁律）：GUI 只读本地 Parquet（DuckDB 查询），**无在线回退**；
不新增 UI 依赖；不重排现有布局；全部视觉值来自 design token。

---

## 现状（代码依据）

### 布局与数据流

- **三栏布局**（`crates/compass/src/main.rs`、`kb/design/ui.md`）：工具栏 40px（标的
  `StockPicker` / 周期 `Segmented` 1d|1w|1M + 前复权 Tag / Fetch / 显示）/ 侧栏 240px
  （自选股）/ `DockArea`（顶层 leaf：图表+东方SEPA 双 tab；下：日志、选股器）/ 状态栏 26px
- **数据通道**（`backend.rs` / `dispatcher.rs` / `messages.rs`）：三条
  citizen→Signal→AsyncDispatcher 通道——`FetchRequest`（图表）/ `RunScreenerRequest` /
  `RunSepaRequest`。图表 fetch 由 `DuckDbProvider::fetch_bars` 处理（`read_parquet`
  直读 `stock_daily.parquet`，1w/1M 用 `date_trunc` 聚合，**前复权缩放**）。
  `dispatcher::handle` 固定 365 天范围。
- **符号体系**（`symbols.md`、`compass-core/src/data/symbol.rs`）：Dolt-native
  前缀格式 `SH600519`/`SZ000001`/`BJ830799`；`validate_symbol` 只接受
  SH/SZ/BJ + 6 位数字；`strip_exchange_prefix` 只剥 SH/SZ/BJ；`sync_picker_from_symbol`
  只同步 SH/SZ/BJ + 6 位数字。ref #201 已把 6 个指数从 `stock_daily.parquet` **剔除**。
- **组件**（`compass-ui`）：`SearchableDropdown`（泛型 + `StockProjection`
  symbol/name/exchange 三投影，弹窗 22px 行、↑↓/Enter、无匹配空态）、`Sidebar`
  （分组 + 行 28px + hover 删除 + 2px accent 选中条）、`DataTable`（可排序、
  numeric 右对齐、PriceText 红涨绿跌、Score/Rank 变体）、`Card`/`Segmented`/
  `Tag`/`EmptyState`/`Button`/`Toast`。
- **i18n**（`compass-i18n` locales）：键 = 模块前缀点分小写 snake_case，zh/en
  键集必须对称。

### 与本 feature 直接相关的三个阻塞点

| 阻塞点 | 位置 | 影响 |
|---|---|---|
| `validate_symbol` 拒绝 `BK0475` 等板块代码 | `compass-core/src/data/parquet.rs:34` | 板块符号无法进入数据层 |
| `strip_exchange_prefix` / `normalize_query` 不认 `BK` | `compass-ui/src/widgets/searchable_dropdown.rs:80` | 搜索「0475」匹配不到 `BK0475` |
| `sync_picker_from_symbol` 只接受 SH/SZ/BJ+6 位 | `crates/compass/src/main.rs:897` | 行点击联动后 picker 不回显板块符号 |
| `DuckDbProvider::fetch_bars` 只读 `stock_daily.parquet` | `compass-core/src/data/duckdb.rs` | 指数数据独立文件无法读取 |

---

## 设计方案

### 总览：三入口各司其职

| 入口 | 形态 | 承担用途 |
|---|---|---|
| 工具栏标的搜索器 | 合并标的列表（股票 + 官方指数 + 板块） | 精确查找任意标的（D11 搜索语义复用） |
| 新 dock tab「大盘」 | 报告型面板（核心指数 Card + 板块排序表） | 大盘概览 + 板块轮动浏览 |
| 侧栏自选 | 支持加入指数/板块（可选增强） | 长期关注标的 |

图表 tab 本身**零改动升级为通用 K 线**——它只消费 `Vec<Bar>`，不关心标的是股是指。

---

### 一、符号体系扩展（数据层前置，GUI 依赖）

| 类别 | 规范符号 | 示例 | 前缀规则 |
|---|---|---|---|
| 官方指数 | 复用 `SH`/`SZ` + 6 位 | `SH000001`（上证指数）、`SZ399001`（深证成指）、`SH000300`（沪深300） | 与股票同格式；000xxx（沪）/399xxx（深）代码段与股票天然不重叠，无歧义 |
| 行业板块 | `BK` + 4 位 | `BK0475`（行业） | 新前缀命名空间，保留东财原始 BK 代码，采集器零转换 |
| 概念板块 | `BK` + 4 位 | `BK1169`（概念） | 同上 |

**类别来源**：`index_daily.parquet` 新增 `index_type` 列（`official` / `concept` / `industry`），
另需一个**名称表**（`index_basic.parquet`：`symbol, name, index_type`，由 import-compass
导出）供 picker 与板块列表使用。

**必须同步扩展的消费点**（本设计给出契约，实现落在数据/核心层）：
- `validate_symbol`：接受 `BK` + 4 位数字
- `parse_explicit_prefix` / `strip_exchange_prefix` / `normalize_query`：识别 `bk` 前缀
- `sync_picker_from_symbol`：接受 `BK` + 4 位
- `exchange_of_symbol`：`BK` 前缀原样返回（驱动 Tag 显示）

> **说明**：指数无复权概念（东财 `fqt=0` 不复权拉取）。`index_daily.parquet`
> 建议导出 `adjclose = close` 占位列，使现有 DuckDbProvider 查询/聚合代码零改动
> （前复权缩放 factor=1.0 恒等）。此为数据管线约束，本设计仅声明。

---

### 二、数据流：双 Parquet 路由（核心设计决策）

**不改消息契约**，在 `DuckDbProvider::fetch_bars` 内做**双文件 fallback**：

```
FetchRequest { symbol, timeframe, range }  （原样）
   └─ DuckDbProvider::fetch_bars
        ├─ 查 stock_daily.parquet  （现有路径，股票）
        └─ 结果为空 → 查 index_daily.parquet  （新路径，指数/板块）
             └─ 1w/1M 聚合复用现有 date_trunc 逻辑（index_daily 亦为日线）
```

**为什么安全**：ref #201 已把 6 个指数从 `stock_daily.parquet` 剔除 → 指数代码在
stock 文件必然查不到，fallback 到 index 文件是**确定性路由**而非碰运气；反向股票
代码在 index 文件也必然查不到（index 文件只含指数/板块）。两个文件互斥，顺序
fallback 语义精确。**零消息契约改动、零 GUI 调用方改动**，数据管线只新增一个
parquet 文件。

**板块行情快照**（市场 tab 的列表数据）：新增第四条通道
`RunIndexSnapshotRequest → RunIndexSnapshotResponse`（SEPA 通道同构：
`crates/compass/src/backend.rs` 再加一个 `AsyncDispatcher`；handler 用
`ParquetReader` 直读 `index_daily.parquet`，窗口函数取**每个标的最后两根日线**
算点位 + 涨跌幅，一次查询返回约 500 行）：

```sql
SELECT symbol, name, index_type, tradedate, close, prev_close, amount
FROM (
  SELECT ..., ROW_NUMBER() OVER (PARTITION BY symbol ORDER BY tradedate DESC) rn
  FROM read_parquet('index_daily.parquet')
  JOIN index_basic ON ...
)
WHERE rn <= 2
```

共享状态新增 `index_snapshot: Dynamic<Option<IndexSnapshot>>` /
`index_snapshot_loading` / `index_snapshot_error`（镜像 sepa_* 三件套）。

---

### 三、工具栏：合并标的搜索

- **标的列表** = `stock_list`（上市 A 股，ref #71 过滤后）+ `index_list`
  （index_basic 全量 ~500）。`StockPicker` 已是泛型 + 投影，传入合并列表即可；
  懒解析（首帧查名字）与 D11 过滤逻辑复用。
- **显示格式**：`SH | 000001 | 上证指数`、`BK | 0475 | 半导体`（复用
  `format_display` 三段式；strip_exchange_prefix 扩展后无前缀重复）。
- **歧义变 feature**：输入 `000001` 同时匹配 `SZ000001`（平安银行）与
  `SH000001`（上证指数）——正好落在 symbols.md 记录的经典歧义场景，弹窗两行
  候选，用户按需选择，显式前缀语义自然呈现。
- **「前复权」Tag 动态化**：当前标的为指数/板块时隐藏（指数不复权）；股票时
  保持显示。判断依据：合并列表中符号是否带 `BK` 前缀或属于官方指数段
  （`index_type` 非空即非股票）。
- 工具栏空间：标的组现有宽度足够（picker 弹窗 min_width 320px，行数 max 12，
  6500 行过滤按需 O(n)，仅在输入变化时 refilter，无性能问题）。

---

### 四、新 tab「大盘」（市场总览面板）

dock 顶层 leaf 扩为三 tab：`[图表 | 大盘 | 东方SEPA]`（SEPA 先例证明每 leaf
多 tab 已支持）。`TabKind` 新增 `Market` 变体（icon：`TRENDING_UP`；
citizen id `market`；`dispatcher.rs::register_citizens` 同步注册）。

面板结构（自上而下，SEPA 报告型同构）：

```
┌─ ① 核心指数 Card（恒显示，横向排布）─────────────────────────────────────┐
│   上证指数 3223.45 +0.82% │ 深证成指 ... │ 创业板指 │ 沪深300 │ 中证500 │ 中证1000 │
│   （每项：名称 caption + mono 点位 + 涨跌幅 PriceText；hover 高亮；点击联动图表） │
├─ ② 工具条：计数「共 N 个 · 日期」+ Segmented [行业板块|概念板块|官方指数] + 刷新 ┐
├─ ③ 板块/指数列表 DataTable ───────────────────────────────────────────────┤
│   名称 | 代码 | 最新 | 涨跌幅 | 成交额(亿)      （默认按涨跌幅降序 = 板块轮动）  │
│   行点击 → 图表联动（与 SEPA 一致，不切 tab）                                │
└──────────────────────────────────────────────────────────────────────────┘
```

- **① 核心指数 Card**：官方指数**核心白名单**（6 只：上证指数/深成指/创业板指/
  沪深300/中证500/中证1000）固定展示；其余官方指数 + 全部板块在 ③ 列表按
  Segmented 切换可达，与锁定决策「全部官方指数」不冲突（列表全量）。
- **② Segmented 三段**：行业板块（默认）/ 概念板块 / 官方指数。切换仅重查
  index_type 过滤（本地快照内存过滤，不重新 fetch——与 SEPA TOP N 本地截断
  同原则）。
- **③ DataTable 列**：名称 `Text` / 代码 `Text`(mono) / 最新 `Price` /
  涨跌幅 `PriceText::percent_only()` / 成交额 `Count`（亿元，整数）。
  默认排序 = 涨跌幅降序（板块轮动视角）；表头点击切换列与升降序（DataTable
  内建）。官方指数段成交额列仍显示（沪市指数成交额有语义）。
- **刷新按钮**：手动触发（`RunIndexSnapshotRequest`），loading 禁用 + spinner
  ——与 SEPA「纯手动、无自动」一致。
- **空态**：`index_daily.parquet` 缺失 → `EmptyState`「暂无指数数据 / 请先导入
  index_daily」；个别板块无数据 → 行内 `—`。

### 五、侧栏自选（可选增强，建议本迭代后置 V2）

自选股机制天然接受任意符号字符串（`add_to_watchlist` 无前缀校验），指数/板块
可直接加入；仅 `Sidebar` 组件对 `BK` 前缀的交易所 Tag 需扩展配色（现有
`TagVariant::Exchange` 只认 SH/SZ/BJ 三色）。**列入 V2**，本迭代侧栏不动——
降低改动面，聚焦大盘 tab 与搜索。

---

## 交互效果

| 触发 | 表现 | 规格 |
|---|---|---|
| 核心指数块 hover | 块底 `bg_hover` 填充 | 100ms（`motion.fast`，Sidebar 行先例） |
| 核心指数块点击 | `dispatch_symbol_fetch`（symbol + fetch）+ **不切 tab**（与 SEPA 行点击一致） | 瞬时 |
| 板块行 hover | DataTable 行 hover 填充（内建） | 内建 |
| 板块行点击 | `dispatch_symbol_fetch(symbol)` → 图表加载板块指数 K 线 | 瞬时 |
| 周期切换 | 复用工具栏 1d/1w/1M + 快捷键 1/2/3，立即重载（ref #218 语义） | 瞬时 |
| 刷新 | Button loading 禁用 + spinner；成功 toast「指数数据已更新 · N 个」 | toast 3s |
| 空态 | EmptyState 引导（缺 parquet 时） | — |
| 前复权 Tag | 标的为指数/板块时隐藏 | 瞬时 |
| 主题/语言切换 | 全 token/i18n 驱动，随全局即时生效 | 瞬时 |

动画遵循设计系统克制原则（ref #245「动画范围」决策：全部瞬时，仅组件内建
hover/press），**不引入自定义布局动画**。

---

## 与现有设计系统的兼容性

- **布局**：不重排三栏；新增 tab 叠入现有 dock 顶层 leaf（SEPA 先例），零布局
  结构变更。
- **组件**：全部复用现有 24 组件（Card/Segmented/DataTable/PriceText/Tag/
  EmptyState/Button/SearchableDropdown），**不新增组件**；DataTable 列型
  （Text/Price/Count）与 SEPA 表格同源。
- **视觉**：红涨绿跌 token、mono 点位（format_price 规则：≥100 → 2 位小数，
  适配指数 3000+ 点位）、caption/body 字号层级——全部既有 token，无新色值。
- **数据**：`DuckDbProvider` 双文件 fallback + 第四条 AsyncDispatcher 通道，
  与现有三通道同构；`SharedState` 新增字段镜像 sepa_* 命名。
- **i18n**：新增 `index.*` 命名空间键（tab / 面板 / 列头 / 空态 / toast），
  zh/en 对称（键完整性测试强制）。
- **测试锚点**（供实现 agent 参考）：`validate_symbol` BK 用例、D11 搜索
  「0475」→ BK0475、`sync_picker_from_symbol` BK 回显、双 parquet fallback
  路由（指数代码查 stock 空 → index 命中）、市场 tab kittest（三 tab 渲染 /
  Segmented 切换 / 行点击联动 / 空态）。

---

## 待确认

1. **tab 命名**：推荐「大盘」；备选「指数」「市场」——影响 `tab.*` 键与用户心智
2. **行点击是否切图表 tab**：推荐**不切**（与 SEPA/选股器完全一致，统一性优先）；
   若用户希望点板块直接看图，则需操作 dock_state 激活 Chart tab（轻微差异）
3. **周期范围**：`dispatcher::handle` 现固定 365 天——月线仅 12 根，指数长趋势
   明显不足。推荐按 timeframe 派生（1d=365 天 / 1w=5 年 / 1M=全量，股票同步受益）。
   是否纳入本迭代？
4. **核心指数白名单**：6 只（上证/深成/创业板/沪深300/中证500/中证1000）是否满足？
   还是需要更多（如中证800/科创50/北证50）？
5. **侧栏自选支持指数/板块**：建议 V2（需扩展 Sidebar 的 BK Tag 配色），是否同意？
6. **index_daily.parquet 列布局**：建议含 `adjclose = close` 占位列以零改动复用
   现有 DuckDbProvider 查询——需与数据管线实现确认 schema 契约

---

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 板块符号命名空间 | `BK`+4 位新前缀 / 复用 SH/SZ / 自定义 `IDX` 段 | `BK`+4 位（保留东财 BK 代码） | 采集器零转换；BK 前缀与 SH/SZ/BJ 无碰撞；validate_symbol 只需加一个分支 | 复用 SH/SZ 会把板块混进股票段制造歧义；IDX 段需采集器映射新代码 |
| 指数数据路由 | 双 parquet fallback / FetchRequest 加 domain 字段 / 独立 IndexProvider | DuckDbProvider 双文件 fallback（stock→index） | 零消息契约改动、零 GUI 调用方改动；ref #201 剔除使路由确定性成立；1w/1M 聚合逻辑直接复用 | domain 字段扩大消息契约波及三处消费点；独立 provider 重复 fetch/聚合逻辑 |
| 大盘展示入口 | 新 dock tab「大盘」/ 仅增强 picker / 侧栏分组 | 新 dock tab（报告型面板） | 板块轮动需要「排序列表 + 概览」这类浏览场景，picker 是精确查找不是浏览；与 SEPA 报告型先例同构 | 仅 picker 无法承载板块排序列表；侧栏分组把 500 行塞进 240px 侧栏，浏览效率差 |
| 板块列表默认排序 | 名称升序 / 涨跌幅降序 | 涨跌幅降序（板块轮动视角） | 核心用途「板块轮动」即找当日强势/弱势板块，默认排序直击场景；表头可改 | 名称升序是中性默认但无信息价值 |
| 行点击联动 | 切图表 tab / 不切（与 SEPA 一致） | 不切 tab | 与 SEPA/选股器行点击行为完全统一（ref #152 先例）；用户可连续点行对比 | 切 tab 打断连续浏览，且引入 dock_state 操作复杂度 |
| 前复权 Tag | 指数也显示 / 按类型隐藏 | 标的为指数/板块时隐藏 | 指数不复权（fqt=0），显示「前复权」是错误信息 | 统一显示会误导用户 |
| 官方指数展示 | 全量官方指数进 Card / 核心白名单 Card + 全量进列表 | 白名单 Card（6 只）+ 全量进 Segmented 列表 | Card 横向空间有限，白名单保证概览可读；全量仍可经列表/搜索可达，不违背「全部官方指数」锁定决策 | 全量进 Card 横向溢出、可读性差 |
| 板块快照通道 | 复用 FetchRequest 逐标的拉 / 新 RunIndexSnapshotRequest 批量 | 新第四条通道批量快照 | 500 标的逐次 fetch 是 500 次异步往返；批量一次查询毫秒级；与 SEPA 通道同构 | 复用 FetchRequest 需逐标的循环、状态管理复杂 |
| 指数数据列布局 | 不含 adjclose / 含 adjclose=close | 含 `adjclose = close` 占位列 | 现有 DuckDbProvider 查询/前复权/聚合代码零改动（factor=1.0 恒等） | 缺列需改查询 SQL 与映射，波及共享路径 |
