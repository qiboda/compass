# 工具链问题排查卡

本文件记录执行过程中遇到并解决的问题，按**问题排查卡**格式沉淀可复用的
排查路径（而非事件流水账）。每条卡片的「排查路径」是核心价值——下次遇到
同类问题直接照做。

> 触发来源：AGENTS.md 品质准则「问题处理闭环」——执行中任何异常禁止静默
> 绕行，必须 感知 → 诊断 → 处理 → 记录（ref #159）。每条记录对应一次闭环。

## 使用方式

- 遇到新问题：先走问题处理闭环，处理完按下方格式追加卡片
- 遇到疑似已知问题：先在本文件搜索症状关键词，命中则直接照「排查路径」复现诊断
- 卡片按类别分组；同类问题多了再考虑归纳为 skill/脚本（内容多了再升级位置）

## 卡片格式

```
### [类别] 一句话问题摘要
- **症状**: 现象（报错信息/异常行为）
- **根因**: 定位到的根本原因（含 why）
- **排查路径**: 可复用的诊断步骤（命令级，本次怎么查出来的）
- **修复**: 改了什么
- **验证**: 怎么确认修好了（复现命令/预期输出）
```

---

## 工具链（MCP / 外部服务）

### [工具链] MCP github server 401 "Requires authentication"

- **症状**: 调用 MCP github 工具（如 create_issue）报
  `GitHubAuthenticationError: Requires authentication` (status 401)；
  但 gh CLI 同操作成功
- **根因**: server-github v0.6.2（npm 2025.4.8）的 `dist/common/utils.js`
  认证逻辑**只读取 `GITHUB_PERSONAL_ACCESS_TOKEN`** 环境变量；opencode 配置
  设的是 `GITHUB_TOKEN` → Authorization header 从未注入 → GitHub 返回 401。
  stdio 握手（initialize）成功 ≠ 认证可用——401 只在真实工具调用时暴露
- **排查路径**:
  1. curl 直连验证 token 本身有效（排除 token 失效）：
     `curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $(cat ~/.config/opencode/github-token)" https://api.github.com/user` → 200
  2. 检查 MCP 进程实际注入的环境变量（排除配置展开问题）：
     `cat /proc/<pid>/environ | tr '\0' '\n' | grep GITHUB` → 确认变量名与值
  3. 读 server 源码确认读取哪个变量：
     `grep -n "process.env.GITHUB" <npm-cache>/@modelcontextprotocol/server-github/dist/common/utils.js`
  4. 用正确变量名 stdio 直测 MCP 调用复现/验证（见「验证」）
- **修复**: `~/.config/opencode/opencode.json` 中 github MCP environment
  变量名 `GITHUB_TOKEN` → `GITHUB_PERSONAL_ACCESS_TOKEN`
- **验证**: `GITHUB_PERSONAL_ACCESS_TOKEN=<token> npx @modelcontextprotocol/server-github`
  后发 `tools/call create_issue` → 返回 issue 对象（而非 401 错误）。注意：
  本机 opencode 需重启才重新注入环境变量
