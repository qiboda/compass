#!/usr/bin/env bash
# Issue #283 D3/D4 — 板块数据源战略调整数据清理（一次性迁移，执行后删除本脚本）
#
# 删除：
#   1. index_daily 中全部 index_type='concept' 行
#   2. index_basic 中全部 index_type='concept' 行
#   3. index_basic 中东财 BK 行业名称行（BK + 4 位，旧 496 行业；无行情数据
#      ——index_daily 中 industry 行原本为 0，删除不损失行情）
#   4. concept_member 表（SEPA 概念主题数据源，70460 行；题材已改行业板块聚合）
#   5. _tmp_name_en 残留临时表（#266 name_en 导入遗留，import 会重建）
#   6. final_score.theme_score 列（issue #283 D6）
#   7. csv/index_basic.csv 旧镜像（含已删的东财 BK/concept 名称行；不删会让
#      下一次增量 import 用 INSERT IGNORE merge 把已删行全部复活 —— 已发生
#      一次，见 fetch_index_daily._persist_outputs 注释。fetch 现已每次重建）
#
# 用法：scripts/cleanup-concept-data.sh
# 前置：/data/compass-data/compass_data 仓库存在；执行后 dolt commit + push（见末尾）。
set -euo pipefail

DOLT_DIR="${COMPASS_DATA_DIR:-/data/compass-data/compass_data}"
cd "$DOLT_DIR"

echo "== 清理前计数 =="
dolt sql -q "SELECT index_type, COUNT(*) c FROM index_basic GROUP BY index_type"
dolt sql -q "SELECT index_type, COUNT(*) c FROM index_daily GROUP BY index_type"
dolt sql -q "SELECT COUNT(*) c FROM concept_member" 2>/dev/null || true

echo "== 执行删除 =="
dolt sql -q "DELETE FROM index_daily WHERE index_type = 'concept'"
dolt sql -q "DELETE FROM index_basic WHERE index_type = 'concept'"
dolt sql -q "DELETE FROM index_basic WHERE index_type = 'industry' AND symbol LIKE 'BK%' AND LENGTH(symbol) = 6"
dolt sql -q "DROP TABLE IF EXISTS concept_member"
dolt sql -q "DROP TABLE IF EXISTS _tmp_name_en"
dolt sql -q "ALTER TABLE final_score DROP COLUMN theme_score"
# CSV 镜像同步清理：旧 index_basic.csv 含已删名称行，残留会让下次 import 复活
rm -f "${COMPASS_DATA_DIR:-/data/compass-data}/csv/index_basic.csv"

echo "== 清理后验证 =="
dolt sql -q "SELECT index_type, COUNT(*) c FROM index_basic GROUP BY index_type"
dolt sql -q "SELECT index_type, COUNT(*) c FROM index_daily GROUP BY index_type"
dolt sql -q "SHOW TABLES"

echo "== 提交 =="
dolt add -A
dolt commit -m "chore: remove concept boards + EastMoney BK industries, drop concept_member, drop theme_score column (issue #283)"
dolt push origin main
dolt status
