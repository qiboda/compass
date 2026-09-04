#!/usr/bin/env bash
# =============================================================================
# prune-actions-caches.sh — 自动清理失效的 GitHub Actions rust-cache（issue #353）
#
# 背景：仓库把保存缓存的权限限制在 master（save-if: master）。当 rustc 工具链
# 升级（dtolnay/rust-toolchain@v1 的 stable 未锁定）时 rust-cache 的 envHash
# 变化，旧 key 的缓存永远不会被任何 run 命中，却在 10GB 配额内占空间。
# 本脚本在 master 的 CI run（或本地）执行：列出该仓库全部缓存，对每个
# "group"（key 第一个 "-" 之前的 token，如 rust / bench-check，对应 CI job）
# 保留 created_at 最新的一份，删除其余 —— 即 "cache miss 后提前清理旧 key"。
#
# 只处理 ref == "refs/heads/master" 的缓存；其它分支/PR 的缓存永不触碰
# （避免误删别的 run 正在使用的缓存）。
#
# 用法（仓库根）:
#   scripts/prune-actions-caches.sh              # 正常模式：列出并 DELETE
#   DRY_RUN=1 scripts/prune-actions-caches.sh    # 演练模式：只打印不删除
#
# 环境变量:
#   GITHUB_REPOSITORY   owner/repo，缺省 qiboda/compass（CI 自动注入）
#   DRY_RUN             非空即认为 DRY_RUN（含 "0"/"false"，见对抗契约 M7）
#
# 依赖: gh CLI（已认证，需要 actions:write 权限）、jq。
# 可 source 复用: source scripts/prune-actions-caches.sh 后调用
#   select_deletions "<caches-json-array>"  -> stdout 每行一个 cache id（升序）
# 配套测试: scripts/tests/prune-actions-caches-test.sh（验收）
#           scripts/tests/prune-actions-caches-adversarial-test.sh（对抗）
# =============================================================================

# ----------------------------------------------------------------------------
# select_deletions <caches_json_array>
#   输入: JSON 数组字符串，元素形如 {"id": N, "ref": "...", "key": "...",
#         "created_at": "ISO-8601 UTC", ...}
#   输出: 每个应删除的 cache id 一行（数字升序、去重）；无删除为 0。
#   规则:
#     - 只处理 ref == "refs/heads/master" 的条目
#     - group = key 中第一个 "-" 之前的 token（无 "-" 时整个 key）
#     - 组内保留 created_at 最大者（ISO 字典序）；tie 保留 id 最大者
#     - 其余条目 -> 删除
#   错误语义（对抗契约）:
#     - 顶层 JSON 畸形 / 非数组      -> 非零退出（显式失败，不静默）
#     - 逐条畸形（缺/非数字 id、null 或缺 ref/key/created_at、非 ISO 时间、
#       非对象元素）                -> 跳过该条目；绝不含任何未知 id，
#                                    绝不使合法最新条目被挤出删除集
# ----------------------------------------------------------------------------
select_deletions() {
    local json="$1" out rc

    out="$(
        jq -r '
            def ok_entry:
                . as $e |
                if ($e | type) != "object" then false
                elif ($e.ref | type) != "string" then false
                elif $e.ref != "refs/heads/master" then false
                elif ($e.key | type) != "string" then false
                elif ($e.created_at | type) != "string" then false
                elif ($e.created_at | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$") | not) then false
                elif ($e.id | type) != "number" then false
                elif (($e.id | floor) != $e.id) then false
                else true end;

            if type != "array" then
                error("top-level value is not an array")
            else
                [ .[] | select(ok_entry) ]
                | group_by(.key | split("-")[0])
                | map(
                      sort_by(.created_at, .id) | last as $keep
                      | [ .[] | select(. != $keep) ]
                  )
                | flatten
                | map(.id)
                | unique
                | .[]
            end
        ' 2>/dev/null <<<"$json"
    )" || {
        echo "prune-actions-caches: invalid caches JSON data (refusing to proceed)" >&2
        return 1
    }

    if [ -n "$out" ]; then
        printf '%s\n' "$out"
    fi
    return 0
}

# ----------------------------------------------------------------------------
# main — 分页列出缓存 -> select -> 逐个 DELETE（DRY_RUN 时只打印）
#   失败语义: list 失败 = 立即非零退出且零 DELETE（未知状态绝不删除）；
#             DELETE 失败 = 打印错误继续，最终非零退出。
# ----------------------------------------------------------------------------
main() {
    local owner_repo="${GITHUB_REPOSITORY:-qiboda/compass}"
    local owner repo
    owner="${owner_repo%%/*}"
    repo="${owner_repo#*/}"
    if [ -z "$owner" ] || [ -z "$repo" ] || [ "$owner" = "$owner_repo" ]; then
        echo "prune-actions-caches: invalid GITHUB_REPOSITORY: '$owner_repo' (expected owner/repo)" >&2
        exit 1
    fi

    local base="/repos/$owner/$repo/actions/caches"
    local page=1 resp arr all='[]' total=0 collected=0 ids id key fail=0

    while :; do
        resp="$(gh api "$base?per_page=100&page=$page")" || {
            echo "prune-actions-caches: failed to list caches (page $page)" >&2
            exit 1
        }
        arr="$(jq -c 'if (.actions_caches | type) == "array" then .actions_caches else null end' \
                <<<"$resp" 2>/dev/null)" || {
            echo "prune-actions-caches: malformed list response from GitHub API" >&2
            exit 1
        }
        if [ "$arr" = "null" ]; then
            echo "prune-actions-caches: malformed list response (actions_caches missing)" >&2
            exit 1
        fi
        total="$(jq -r '.total_count // 0' <<<"$resp" 2>/dev/null)" || total=0

        if [ "$all" = '[]' ]; then
            all="$arr"
        else
            all="$(jq -c --argjson a "$all" --argjson b "$arr" '$a + $b' <<<'null' 2>/dev/null)" || {
                echo "prune-actions-caches: failed to merge paginated caches" >&2
                exit 1
            }
        fi
        collected="$(jq -r 'length' <<<"$all" 2>/dev/null)" || collected=0

        # 空页或已收满 total_count -> 停止（对抗契约 M2/R3：防止死循环）
        if [ "$arr" = '[]' ] || [ "$collected" -ge "$total" ]; then
            break
        fi
        page=$((page + 1))
    done

    ids="$(select_deletions "$all")" || {
        echo "prune-actions-caches: cache selection failed" >&2
        exit 1
    }

    while IFS= read -r id; do
        [ -z "$id" ] && continue
        key="$(jq -r --argjson id "$id" '.[] | select(.id == $id) | .key' \
                <<<"$all" 2>/dev/null | head -n1)"
        if [ -n "${DRY_RUN:-}" ]; then
            echo "[DRY-RUN] would delete $id $key"
            continue
        fi
        if gh api -X DELETE "$base/$id" >/dev/null 2>&1; then
            echo "[DELETE] $id $key"
        else
            echo "prune-actions-caches: failed to delete cache $id" >&2
            fail=1
        fi
    done <<< "$ids"

    exit "$fail"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
