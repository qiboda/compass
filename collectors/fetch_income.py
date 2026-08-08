#!/usr/bin/env python3
"""A-share income statement collector (利润表).

Independent module — uses common.py for shared infrastructure.

Report:  RPT_F10_FINANCE_GINCOME, 203 fields, filter column REPORT_DATE.
Default: 2020 onwards, all four quarterly periods.
"""

import asyncio
import sys
from datetime import datetime
from pathlib import Path

from common import (
    AsyncSession,
    Throttle,
    build_dates,
    csv_dir,
    fetch_paginated,
    import_replace_table,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_F10_FINANCE_GINCOME"
FILTER_COLUMN = "REPORT_DATE"
DOLT_TABLE = "fin_income"
START_YEAR = 2020

DDL = """\
CREATE TABLE IF NOT EXISTS fin_income (
    symbol              VARCHAR(20) NOT NULL,
    report_date         DATE NOT NULL,
    SECUCODE                   VARCHAR(100),
    SECURITY_CODE              VARCHAR(100),
    SECURITY_NAME_ABBR         VARCHAR(100),
    ORG_CODE                   VARCHAR(100),
    ORG_TYPE                   VARCHAR(100),
    REPORT_TYPE                VARCHAR(100),
    REPORT_DATE_NAME           VARCHAR(100),
    SECURITY_TYPE_CODE         VARCHAR(100),
    NOTICE_DATE                VARCHAR(100),
    UPDATE_DATE                VARCHAR(100),
    CURRENCY                   VARCHAR(100),
    TOTAL_OPERATE_INCOME       DOUBLE,
    TOTAL_OPERATE_INCOME_YOY   DOUBLE,
    OPERATE_INCOME             DOUBLE,
    OPERATE_INCOME_YOY         DOUBLE,
    INTEREST_INCOME            DOUBLE,
    INTEREST_INCOME_YOY        DOUBLE,
    EARNED_PREMIUM             DOUBLE,
    EARNED_PREMIUM_YOY         DOUBLE,
    FEE_COMMISSION_INCOME      DOUBLE,
    FEE_COMMISSION_INCOME_YOY  DOUBLE,
    OTHER_BUSINESS_INCOME      DOUBLE,
    OTHER_BUSINESS_INCOME_YOY  DOUBLE,
    TOI_OTHER                  DOUBLE,
    TOI_OTHER_YOY              DOUBLE,
    TOTAL_OPERATE_COST         DOUBLE,
    TOTAL_OPERATE_COST_YOY     DOUBLE,
    OPERATE_COST               DOUBLE,
    OPERATE_COST_YOY           DOUBLE,
    INTEREST_EXPENSE           DOUBLE,
    INTEREST_EXPENSE_YOY       DOUBLE,
    FEE_COMMISSION_EXPENSE     DOUBLE,
    FEE_COMMISSION_EXPENSE_YOY DOUBLE,
    RESEARCH_EXPENSE           DOUBLE,
    RESEARCH_EXPENSE_YOY       DOUBLE,
    SURRENDER_VALUE            DOUBLE,
    SURRENDER_VALUE_YOY        DOUBLE,
    NET_COMPENSATE_EXPENSE     DOUBLE,
    NET_COMPENSATE_EXPENSE_YOY DOUBLE,
    NET_CONTRACT_RESERVE       DOUBLE,
    NET_CONTRACT_RESERVE_YOY   DOUBLE,
    POLICY_BONUS_EXPENSE       DOUBLE,
    POLICY_BONUS_EXPENSE_YOY   DOUBLE,
    REINSURE_EXPENSE           DOUBLE,
    REINSURE_EXPENSE_YOY       DOUBLE,
    OTHER_BUSINESS_COST        DOUBLE,
    OTHER_BUSINESS_COST_YOY    DOUBLE,
    OPERATE_TAX_ADD            DOUBLE,
    OPERATE_TAX_ADD_YOY        DOUBLE,
    SALE_EXPENSE               DOUBLE,
    SALE_EXPENSE_YOY           DOUBLE,
    MANAGE_EXPENSE             DOUBLE,
    MANAGE_EXPENSE_YOY         DOUBLE,
    ME_RESEARCH_EXPENSE        DOUBLE,
    ME_RESEARCH_EXPENSE_YOY    DOUBLE,
    FINANCE_EXPENSE            DOUBLE,
    FINANCE_EXPENSE_YOY        DOUBLE,
    FE_INTEREST_EXPENSE        DOUBLE,
    FE_INTEREST_EXPENSE_YOY    DOUBLE,
    FE_INTEREST_INCOME         DOUBLE,
    FE_INTEREST_INCOME_YOY     DOUBLE,
    ASSET_IMPAIRMENT_LOSS      DOUBLE,
    ASSET_IMPAIRMENT_LOSS_YOY  DOUBLE,
    CREDIT_IMPAIRMENT_LOSS     DOUBLE,
    CREDIT_IMPAIRMENT_LOSS_YOY DOUBLE,
    TOC_OTHER                  DOUBLE,
    TOC_OTHER_YOY              DOUBLE,
    FAIRVALUE_CHANGE_INCOME    DOUBLE,
    FAIRVALUE_CHANGE_INCOME_YOY DOUBLE,
    INVEST_INCOME              DOUBLE,
    INVEST_INCOME_YOY          DOUBLE,
    INVEST_JOINT_INCOME        DOUBLE,
    INVEST_JOINT_INCOME_YOY    DOUBLE,
    NET_EXPOSURE_INCOME        DOUBLE,
    NET_EXPOSURE_INCOME_YOY    DOUBLE,
    EXCHANGE_INCOME            DOUBLE,
    EXCHANGE_INCOME_YOY        DOUBLE,
    ASSET_DISPOSAL_INCOME      DOUBLE,
    ASSET_DISPOSAL_INCOME_YOY  DOUBLE,
    ASSET_IMPAIRMENT_INCOME    DOUBLE,
    ASSET_IMPAIRMENT_INCOME_YOY DOUBLE,
    CREDIT_IMPAIRMENT_INCOME   DOUBLE,
    CREDIT_IMPAIRMENT_INCOME_YOY DOUBLE,
    OTHER_INCOME               DOUBLE,
    OTHER_INCOME_YOY           DOUBLE,
    OPERATE_PROFIT_OTHER       DOUBLE,
    OPERATE_PROFIT_OTHER_YOY   DOUBLE,
    OPERATE_PROFIT_BALANCE     DOUBLE,
    OPERATE_PROFIT_BALANCE_YOY DOUBLE,
    OPERATE_PROFIT             DOUBLE,
    OPERATE_PROFIT_YOY         DOUBLE,
    NONBUSINESS_INCOME         DOUBLE,
    NONBUSINESS_INCOME_YOY     DOUBLE,
    NONCURRENT_DISPOSAL_INCOME DOUBLE,
    NONCURRENT_DISPOSAL_INCOME_YOY DOUBLE,
    NONBUSINESS_EXPENSE        DOUBLE,
    NONBUSINESS_EXPENSE_YOY    DOUBLE,
    NONCURRENT_DISPOSAL_LOSS   DOUBLE,
    NONCURRENT_DISPOSAL_LOSS_YOY DOUBLE,
    EFFECT_TP_OTHER            DOUBLE,
    EFFECT_TP_OTHER_YOY        DOUBLE,
    TOTAL_PROFIT_BALANCE       DOUBLE,
    TOTAL_PROFIT_BALANCE_YOY   DOUBLE,
    TOTAL_PROFIT               DOUBLE,
    TOTAL_PROFIT_YOY           DOUBLE,
    INCOME_TAX                 DOUBLE,
    INCOME_TAX_YOY             DOUBLE,
    EFFECT_NETPROFIT_OTHER     DOUBLE,
    EFFECT_NETPROFIT_OTHER_YOY DOUBLE,
    EFFECT_NETPROFIT_BALANCE   DOUBLE,
    EFFECT_NETPROFIT_BALANCE_YOY DOUBLE,
    UNCONFIRM_INVEST_LOSS      DOUBLE,
    UNCONFIRM_INVEST_LOSS_YOY  DOUBLE,
    NETPROFIT                  DOUBLE,
    NETPROFIT_YOY              DOUBLE,
    PRECOMBINE_PROFIT          DOUBLE,
    PRECOMBINE_PROFIT_YOY      DOUBLE,
    CONTINUED_NETPROFIT        DOUBLE,
    CONTINUED_NETPROFIT_YOY    DOUBLE,
    DISCONTINUED_NETPROFIT     DOUBLE,
    DISCONTINUED_NETPROFIT_YOY DOUBLE,
    PARENT_NETPROFIT           DOUBLE,
    PARENT_NETPROFIT_YOY       DOUBLE,
    MINORITY_INTEREST          DOUBLE,
    MINORITY_INTEREST_YOY      DOUBLE,
    DEDUCT_PARENT_NETPROFIT    DOUBLE,
    DEDUCT_PARENT_NETPROFIT_YOY DOUBLE,
    NETPROFIT_OTHER            DOUBLE,
    NETPROFIT_OTHER_YOY        DOUBLE,
    NETPROFIT_BALANCE          DOUBLE,
    NETPROFIT_BALANCE_YOY      DOUBLE,
    BASIC_EPS                  DOUBLE,
    BASIC_EPS_YOY              DOUBLE,
    DILUTED_EPS                DOUBLE,
    DILUTED_EPS_YOY            DOUBLE,
    OTHER_COMPRE_INCOME        DOUBLE,
    OTHER_COMPRE_INCOME_YOY    DOUBLE,
    PARENT_OCI                 DOUBLE,
    PARENT_OCI_YOY             DOUBLE,
    MINORITY_OCI               DOUBLE,
    MINORITY_OCI_YOY           DOUBLE,
    PARENT_OCI_OTHER           DOUBLE,
    PARENT_OCI_OTHER_YOY       DOUBLE,
    PARENT_OCI_BALANCE         DOUBLE,
    PARENT_OCI_BALANCE_YOY     DOUBLE,
    UNABLE_OCI                 DOUBLE,
    UNABLE_OCI_YOY             DOUBLE,
    CREDITRISK_FAIRVALUE_CHANGE DOUBLE,
    CREDITRISK_FAIRVALUE_CHANGE_YOY DOUBLE,
    OTHERRIGHT_FAIRVALUE_CHANGE DOUBLE,
    OTHERRIGHT_FAIRVALUE_CHANGE_YOY DOUBLE,
    SETUP_PROFIT_CHANGE        DOUBLE,
    SETUP_PROFIT_CHANGE_YOY    DOUBLE,
    RIGHTLAW_UNABLE_OCI        DOUBLE,
    RIGHTLAW_UNABLE_OCI_YOY    DOUBLE,
    UNABLE_OCI_OTHER           DOUBLE,
    UNABLE_OCI_OTHER_YOY       DOUBLE,
    UNABLE_OCI_BALANCE         DOUBLE,
    UNABLE_OCI_BALANCE_YOY     DOUBLE,
    ABLE_OCI                   DOUBLE,
    ABLE_OCI_YOY               DOUBLE,
    RIGHTLAW_ABLE_OCI          DOUBLE,
    RIGHTLAW_ABLE_OCI_YOY      DOUBLE,
    AFA_FAIRVALUE_CHANGE       DOUBLE,
    AFA_FAIRVALUE_CHANGE_YOY   DOUBLE,
    HMI_AFA                    DOUBLE,
    HMI_AFA_YOY                DOUBLE,
    CASHFLOW_HEDGE_VALID       DOUBLE,
    CASHFLOW_HEDGE_VALID_YOY   DOUBLE,
    CREDITOR_FAIRVALUE_CHANGE  DOUBLE,
    CREDITOR_FAIRVALUE_CHANGE_YOY DOUBLE,
    CREDITOR_IMPAIRMENT_RESERVE DOUBLE,
    CREDITOR_IMPAIRMENT_RESERVE_YOY DOUBLE,
    FINANCE_OCI_AMT            DOUBLE,
    FINANCE_OCI_AMT_YOY        DOUBLE,
    CONVERT_DIFF               DOUBLE,
    CONVERT_DIFF_YOY           DOUBLE,
    ABLE_OCI_OTHER             DOUBLE,
    ABLE_OCI_OTHER_YOY         DOUBLE,
    ABLE_OCI_BALANCE           DOUBLE,
    ABLE_OCI_BALANCE_YOY       DOUBLE,
    OCI_OTHER                  DOUBLE,
    OCI_OTHER_YOY              DOUBLE,
    OCI_BALANCE                DOUBLE,
    OCI_BALANCE_YOY            DOUBLE,
    TOTAL_COMPRE_INCOME        DOUBLE,
    TOTAL_COMPRE_INCOME_YOY    DOUBLE,
    PARENT_TCI                 DOUBLE,
    PARENT_TCI_YOY             DOUBLE,
    MINORITY_TCI               DOUBLE,
    MINORITY_TCI_YOY           DOUBLE,
    PRECOMBINE_TCI             DOUBLE,
    PRECOMBINE_TCI_YOY         DOUBLE,
    EFFECT_TCI_BALANCE         DOUBLE,
    EFFECT_TCI_BALANCE_YOY     DOUBLE,
    TCI_OTHER                  DOUBLE,
    TCI_OTHER_YOY              DOUBLE,
    TCI_BALANCE                DOUBLE,
    TCI_BALANCE_YOY            DOUBLE,
    ACF_END_INCOME             DOUBLE,
    ACF_END_INCOME_YOY         DOUBLE,
    OPINION_TYPE               VARCHAR(100),
    PRIMARY KEY (symbol, report_date)
)"""

COLS = (
    "SECUCODE, SECURITY_CODE, SECURITY_NAME_ABBR, ORG_CODE, ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, SECURITY_TYPE_CODE, NOTICE_DATE, UPDATE_DATE, CURRENCY, TOTAL_OPERATE_INCOME, TOTAL_OPERATE_INCOME_YOY, OPERATE_INCOME, OPERATE_INCOME_YOY, INTEREST_INCOME, INTEREST_INCOME_YOY, EARNED_PREMIUM, EARNED_PREMIUM_YOY, FEE_COMMISSION_INCOME, FEE_COMMISSION_INCOME_YOY, OTHER_BUSINESS_INCOME, OTHER_BUSINESS_INCOME_YOY, TOI_OTHER, TOI_OTHER_YOY, TOTAL_OPERATE_COST, TOTAL_OPERATE_COST_YOY, OPERATE_COST, OPERATE_COST_YOY, INTEREST_EXPENSE, INTEREST_EXPENSE_YOY, FEE_COMMISSION_EXPENSE, FEE_COMMISSION_EXPENSE_YOY, RESEARCH_EXPENSE, RESEARCH_EXPENSE_YOY, SURRENDER_VALUE, SURRENDER_VALUE_YOY, NET_COMPENSATE_EXPENSE, NET_COMPENSATE_EXPENSE_YOY, NET_CONTRACT_RESERVE, NET_CONTRACT_RESERVE_YOY, POLICY_BONUS_EXPENSE, POLICY_BONUS_EXPENSE_YOY, REINSURE_EXPENSE, REINSURE_EXPENSE_YOY, OTHER_BUSINESS_COST, OTHER_BUSINESS_COST_YOY, OPERATE_TAX_ADD, OPERATE_TAX_ADD_YOY, SALE_EXPENSE, SALE_EXPENSE_YOY, MANAGE_EXPENSE, MANAGE_EXPENSE_YOY, ME_RESEARCH_EXPENSE, ME_RESEARCH_EXPENSE_YOY, FINANCE_EXPENSE, FINANCE_EXPENSE_YOY, FE_INTEREST_EXPENSE, FE_INTEREST_EXPENSE_YOY, FE_INTEREST_INCOME, FE_INTEREST_INCOME_YOY, ASSET_IMPAIRMENT_LOSS, ASSET_IMPAIRMENT_LOSS_YOY, CREDIT_IMPAIRMENT_LOSS, CREDIT_IMPAIRMENT_LOSS_YOY, TOC_OTHER, TOC_OTHER_YOY, FAIRVALUE_CHANGE_INCOME, FAIRVALUE_CHANGE_INCOME_YOY, INVEST_INCOME, INVEST_INCOME_YOY, INVEST_JOINT_INCOME, INVEST_JOINT_INCOME_YOY, NET_EXPOSURE_INCOME, NET_EXPOSURE_INCOME_YOY, EXCHANGE_INCOME, EXCHANGE_INCOME_YOY, ASSET_DISPOSAL_INCOME, ASSET_DISPOSAL_INCOME_YOY, ASSET_IMPAIRMENT_INCOME, ASSET_IMPAIRMENT_INCOME_YOY, CREDIT_IMPAIRMENT_INCOME, CREDIT_IMPAIRMENT_INCOME_YOY, OTHER_INCOME, OTHER_INCOME_YOY, OPERATE_PROFIT_OTHER, OPERATE_PROFIT_OTHER_YOY, OPERATE_PROFIT_BALANCE, OPERATE_PROFIT_BALANCE_YOY, OPERATE_PROFIT, OPERATE_PROFIT_YOY, NONBUSINESS_INCOME, NONBUSINESS_INCOME_YOY, NONCURRENT_DISPOSAL_INCOME, NONCURRENT_DISPOSAL_INCOME_YOY, NONBUSINESS_EXPENSE, NONBUSINESS_EXPENSE_YOY, NONCURRENT_DISPOSAL_LOSS, NONCURRENT_DISPOSAL_LOSS_YOY, EFFECT_TP_OTHER, EFFECT_TP_OTHER_YOY, TOTAL_PROFIT_BALANCE, TOTAL_PROFIT_BALANCE_YOY, TOTAL_PROFIT, TOTAL_PROFIT_YOY, INCOME_TAX, INCOME_TAX_YOY, EFFECT_NETPROFIT_OTHER, EFFECT_NETPROFIT_OTHER_YOY, EFFECT_NETPROFIT_BALANCE, EFFECT_NETPROFIT_BALANCE_YOY, UNCONFIRM_INVEST_LOSS, UNCONFIRM_INVEST_LOSS_YOY, NETPROFIT, NETPROFIT_YOY, PRECOMBINE_PROFIT, PRECOMBINE_PROFIT_YOY, CONTINUED_NETPROFIT, CONTINUED_NETPROFIT_YOY, DISCONTINUED_NETPROFIT, DISCONTINUED_NETPROFIT_YOY, PARENT_NETPROFIT, PARENT_NETPROFIT_YOY, MINORITY_INTEREST, MINORITY_INTEREST_YOY, DEDUCT_PARENT_NETPROFIT, DEDUCT_PARENT_NETPROFIT_YOY, NETPROFIT_OTHER, NETPROFIT_OTHER_YOY, NETPROFIT_BALANCE, NETPROFIT_BALANCE_YOY, BASIC_EPS, BASIC_EPS_YOY, DILUTED_EPS, DILUTED_EPS_YOY, OTHER_COMPRE_INCOME, OTHER_COMPRE_INCOME_YOY, PARENT_OCI, PARENT_OCI_YOY, MINORITY_OCI, MINORITY_OCI_YOY, PARENT_OCI_OTHER, PARENT_OCI_OTHER_YOY, PARENT_OCI_BALANCE, PARENT_OCI_BALANCE_YOY, UNABLE_OCI, UNABLE_OCI_YOY, CREDITRISK_FAIRVALUE_CHANGE, CREDITRISK_FAIRVALUE_CHANGE_YOY, OTHERRIGHT_FAIRVALUE_CHANGE, OTHERRIGHT_FAIRVALUE_CHANGE_YOY, SETUP_PROFIT_CHANGE, SETUP_PROFIT_CHANGE_YOY, RIGHTLAW_UNABLE_OCI, RIGHTLAW_UNABLE_OCI_YOY, UNABLE_OCI_OTHER, UNABLE_OCI_OTHER_YOY, UNABLE_OCI_BALANCE, UNABLE_OCI_BALANCE_YOY, ABLE_OCI, ABLE_OCI_YOY, RIGHTLAW_ABLE_OCI, RIGHTLAW_ABLE_OCI_YOY, AFA_FAIRVALUE_CHANGE, AFA_FAIRVALUE_CHANGE_YOY, HMI_AFA, HMI_AFA_YOY, CASHFLOW_HEDGE_VALID, CASHFLOW_HEDGE_VALID_YOY, CREDITOR_FAIRVALUE_CHANGE, CREDITOR_FAIRVALUE_CHANGE_YOY, CREDITOR_IMPAIRMENT_RESERVE, CREDITOR_IMPAIRMENT_RESERVE_YOY, FINANCE_OCI_AMT, FINANCE_OCI_AMT_YOY, CONVERT_DIFF, CONVERT_DIFF_YOY, ABLE_OCI_OTHER, ABLE_OCI_OTHER_YOY, ABLE_OCI_BALANCE, ABLE_OCI_BALANCE_YOY, OCI_OTHER, OCI_OTHER_YOY, OCI_BALANCE, OCI_BALANCE_YOY, TOTAL_COMPRE_INCOME, TOTAL_COMPRE_INCOME_YOY, PARENT_TCI, PARENT_TCI_YOY, MINORITY_TCI, MINORITY_TCI_YOY, PRECOMBINE_TCI, PRECOMBINE_TCI_YOY, EFFECT_TCI_BALANCE, EFFECT_TCI_BALANCE_YOY, TCI_OTHER, TCI_OTHER_YOY, TCI_BALANCE, TCI_BALANCE_YOY, ACF_END_INCOME, ACF_END_INCOME_YOY, OPINION_TYPE"
)


async def run(
    years: list[int] | None = None,
    periods: str = "Q1,Q2,Q3,FY",
    page_size: int = 100,
) -> Path:
    if years is None:
        years = list(range(START_YEAR, datetime.now().year + 1))

    output_path = csv_dir() / f"{REPORT_NAME}.csv"
    period_list = [p.strip() for p in periods.split(",")]
    all_dates = build_dates(years, period_list)

    since = last_report_date(DOLT_TABLE)
    if since:
        print(f"Last report date in Dolt: {since}, fetching only newer periods", file=sys.stderr)
        all_dates = [d for d in all_dates if d >= since]
        if not all_dates:
            print("No new report periods to fetch.", file=sys.stderr)
            return output_path

    print(f"Report: {REPORT_NAME}", file=sys.stderr)
    print(
        f"Periods: {len(all_dates)} ({periods}, "
        f"{all_dates[0] if all_dates else 'none'}..{all_dates[-1] if all_dates else 'none'})",
        file=sys.stderr,
    )
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    total_records = 0
    first_write = True

    async with AsyncSession(impersonate="chrome142") as session:
        for i, report_date in enumerate(all_dates):
            print(
                f"[{i + 1}/{len(all_dates)}] {report_date} ...",
                file=sys.stderr, end=" ", flush=True,
            )
            try:
                records = await fetch_paginated(
                    session, throttle, REPORT_NAME, FILTER_COLUMN, report_date, page_size,
                )
            except Exception as e:
                print(f"FAILED: {e}", file=sys.stderr)
                continue

            if records:
                write_csv(records, output_path, append=not first_write)
                first_write = False
                print(f"{len(records)} records", file=sys.stderr)
            else:
                print("empty", file=sys.stderr)
            total_records += len(records)

    print(f"\nDone: {total_records} records → {output_path.resolve()}", file=sys.stderr)
    return output_path


# Wide temp-table schema: Dolt's `-c` CSV import inference caps row size at
# 65504 bytes, which a 203-column CSV overflows — create the temp table
# explicitly and import with `-u` (see common.py dolt_table_import).
_TMP_INC_DDL = """\
CREATE TABLE _tmp_inc (
    SECUCODE                                   VARCHAR(100),
    SECURITY_CODE                              VARCHAR(100),
    SECURITY_NAME_ABBR                         VARCHAR(100),
    ORG_CODE                                   VARCHAR(100),
    ORG_TYPE                                   VARCHAR(100),
    REPORT_DATE                                VARCHAR(100),
    REPORT_TYPE                                VARCHAR(100),
    REPORT_DATE_NAME                           VARCHAR(100),
    SECURITY_TYPE_CODE                         VARCHAR(100),
    NOTICE_DATE                                VARCHAR(100),
    UPDATE_DATE                                VARCHAR(100),
    CURRENCY                                   VARCHAR(100),
    TOTAL_OPERATE_INCOME                       DOUBLE,
    TOTAL_OPERATE_INCOME_YOY                   DOUBLE,
    OPERATE_INCOME                             DOUBLE,
    OPERATE_INCOME_YOY                         DOUBLE,
    INTEREST_INCOME                            DOUBLE,
    INTEREST_INCOME_YOY                        DOUBLE,
    EARNED_PREMIUM                             DOUBLE,
    EARNED_PREMIUM_YOY                         DOUBLE,
    FEE_COMMISSION_INCOME                      DOUBLE,
    FEE_COMMISSION_INCOME_YOY                  DOUBLE,
    OTHER_BUSINESS_INCOME                      DOUBLE,
    OTHER_BUSINESS_INCOME_YOY                  DOUBLE,
    TOI_OTHER                                  DOUBLE,
    TOI_OTHER_YOY                              DOUBLE,
    TOTAL_OPERATE_COST                         DOUBLE,
    TOTAL_OPERATE_COST_YOY                     DOUBLE,
    OPERATE_COST                               DOUBLE,
    OPERATE_COST_YOY                           DOUBLE,
    INTEREST_EXPENSE                           DOUBLE,
    INTEREST_EXPENSE_YOY                       DOUBLE,
    FEE_COMMISSION_EXPENSE                     DOUBLE,
    FEE_COMMISSION_EXPENSE_YOY                 DOUBLE,
    RESEARCH_EXPENSE                           DOUBLE,
    RESEARCH_EXPENSE_YOY                       DOUBLE,
    SURRENDER_VALUE                            DOUBLE,
    SURRENDER_VALUE_YOY                        DOUBLE,
    NET_COMPENSATE_EXPENSE                     DOUBLE,
    NET_COMPENSATE_EXPENSE_YOY                 DOUBLE,
    NET_CONTRACT_RESERVE                       DOUBLE,
    NET_CONTRACT_RESERVE_YOY                   DOUBLE,
    POLICY_BONUS_EXPENSE                       DOUBLE,
    POLICY_BONUS_EXPENSE_YOY                   DOUBLE,
    REINSURE_EXPENSE                           DOUBLE,
    REINSURE_EXPENSE_YOY                       DOUBLE,
    OTHER_BUSINESS_COST                        DOUBLE,
    OTHER_BUSINESS_COST_YOY                    DOUBLE,
    OPERATE_TAX_ADD                            DOUBLE,
    OPERATE_TAX_ADD_YOY                        DOUBLE,
    SALE_EXPENSE                               DOUBLE,
    SALE_EXPENSE_YOY                           DOUBLE,
    MANAGE_EXPENSE                             DOUBLE,
    MANAGE_EXPENSE_YOY                         DOUBLE,
    ME_RESEARCH_EXPENSE                        DOUBLE,
    ME_RESEARCH_EXPENSE_YOY                    DOUBLE,
    FINANCE_EXPENSE                            DOUBLE,
    FINANCE_EXPENSE_YOY                        DOUBLE,
    FE_INTEREST_EXPENSE                        DOUBLE,
    FE_INTEREST_EXPENSE_YOY                    DOUBLE,
    FE_INTEREST_INCOME                         DOUBLE,
    FE_INTEREST_INCOME_YOY                     DOUBLE,
    ASSET_IMPAIRMENT_LOSS                      DOUBLE,
    ASSET_IMPAIRMENT_LOSS_YOY                  DOUBLE,
    CREDIT_IMPAIRMENT_LOSS                     DOUBLE,
    CREDIT_IMPAIRMENT_LOSS_YOY                 DOUBLE,
    TOC_OTHER                                  DOUBLE,
    TOC_OTHER_YOY                              DOUBLE,
    FAIRVALUE_CHANGE_INCOME                    DOUBLE,
    FAIRVALUE_CHANGE_INCOME_YOY                DOUBLE,
    INVEST_INCOME                              DOUBLE,
    INVEST_INCOME_YOY                          DOUBLE,
    INVEST_JOINT_INCOME                        DOUBLE,
    INVEST_JOINT_INCOME_YOY                    DOUBLE,
    NET_EXPOSURE_INCOME                        DOUBLE,
    NET_EXPOSURE_INCOME_YOY                    DOUBLE,
    EXCHANGE_INCOME                            DOUBLE,
    EXCHANGE_INCOME_YOY                        DOUBLE,
    ASSET_DISPOSAL_INCOME                      DOUBLE,
    ASSET_DISPOSAL_INCOME_YOY                  DOUBLE,
    ASSET_IMPAIRMENT_INCOME                    DOUBLE,
    ASSET_IMPAIRMENT_INCOME_YOY                DOUBLE,
    CREDIT_IMPAIRMENT_INCOME                   DOUBLE,
    CREDIT_IMPAIRMENT_INCOME_YOY               DOUBLE,
    OTHER_INCOME                               DOUBLE,
    OTHER_INCOME_YOY                           DOUBLE,
    OPERATE_PROFIT_OTHER                       DOUBLE,
    OPERATE_PROFIT_OTHER_YOY                   DOUBLE,
    OPERATE_PROFIT_BALANCE                     DOUBLE,
    OPERATE_PROFIT_BALANCE_YOY                 DOUBLE,
    OPERATE_PROFIT                             DOUBLE,
    OPERATE_PROFIT_YOY                         DOUBLE,
    NONBUSINESS_INCOME                         DOUBLE,
    NONBUSINESS_INCOME_YOY                     DOUBLE,
    NONCURRENT_DISPOSAL_INCOME                 DOUBLE,
    NONCURRENT_DISPOSAL_INCOME_YOY             DOUBLE,
    NONBUSINESS_EXPENSE                        DOUBLE,
    NONBUSINESS_EXPENSE_YOY                    DOUBLE,
    NONCURRENT_DISPOSAL_LOSS                   DOUBLE,
    NONCURRENT_DISPOSAL_LOSS_YOY               DOUBLE,
    EFFECT_TP_OTHER                            DOUBLE,
    EFFECT_TP_OTHER_YOY                        DOUBLE,
    TOTAL_PROFIT_BALANCE                       DOUBLE,
    TOTAL_PROFIT_BALANCE_YOY                   DOUBLE,
    TOTAL_PROFIT                               DOUBLE,
    TOTAL_PROFIT_YOY                           DOUBLE,
    INCOME_TAX                                 DOUBLE,
    INCOME_TAX_YOY                             DOUBLE,
    EFFECT_NETPROFIT_OTHER                     DOUBLE,
    EFFECT_NETPROFIT_OTHER_YOY                 DOUBLE,
    EFFECT_NETPROFIT_BALANCE                   DOUBLE,
    EFFECT_NETPROFIT_BALANCE_YOY               DOUBLE,
    UNCONFIRM_INVEST_LOSS                      DOUBLE,
    UNCONFIRM_INVEST_LOSS_YOY                  DOUBLE,
    NETPROFIT                                  DOUBLE,
    NETPROFIT_YOY                              DOUBLE,
    PRECOMBINE_PROFIT                          DOUBLE,
    PRECOMBINE_PROFIT_YOY                      DOUBLE,
    CONTINUED_NETPROFIT                        DOUBLE,
    CONTINUED_NETPROFIT_YOY                    DOUBLE,
    DISCONTINUED_NETPROFIT                     DOUBLE,
    DISCONTINUED_NETPROFIT_YOY                 DOUBLE,
    PARENT_NETPROFIT                           DOUBLE,
    PARENT_NETPROFIT_YOY                       DOUBLE,
    MINORITY_INTEREST                          DOUBLE,
    MINORITY_INTEREST_YOY                      DOUBLE,
    DEDUCT_PARENT_NETPROFIT                    DOUBLE,
    DEDUCT_PARENT_NETPROFIT_YOY                DOUBLE,
    NETPROFIT_OTHER                            DOUBLE,
    NETPROFIT_OTHER_YOY                        DOUBLE,
    NETPROFIT_BALANCE                          DOUBLE,
    NETPROFIT_BALANCE_YOY                      DOUBLE,
    BASIC_EPS                                  DOUBLE,
    BASIC_EPS_YOY                              DOUBLE,
    DILUTED_EPS                                DOUBLE,
    DILUTED_EPS_YOY                            DOUBLE,
    OTHER_COMPRE_INCOME                        DOUBLE,
    OTHER_COMPRE_INCOME_YOY                    DOUBLE,
    PARENT_OCI                                 DOUBLE,
    PARENT_OCI_YOY                             DOUBLE,
    MINORITY_OCI                               DOUBLE,
    MINORITY_OCI_YOY                           DOUBLE,
    PARENT_OCI_OTHER                           DOUBLE,
    PARENT_OCI_OTHER_YOY                       DOUBLE,
    PARENT_OCI_BALANCE                         DOUBLE,
    PARENT_OCI_BALANCE_YOY                     DOUBLE,
    UNABLE_OCI                                 DOUBLE,
    UNABLE_OCI_YOY                             DOUBLE,
    CREDITRISK_FAIRVALUE_CHANGE                DOUBLE,
    CREDITRISK_FAIRVALUE_CHANGE_YOY            DOUBLE,
    OTHERRIGHT_FAIRVALUE_CHANGE                DOUBLE,
    OTHERRIGHT_FAIRVALUE_CHANGE_YOY            DOUBLE,
    SETUP_PROFIT_CHANGE                        DOUBLE,
    SETUP_PROFIT_CHANGE_YOY                    DOUBLE,
    RIGHTLAW_UNABLE_OCI                        DOUBLE,
    RIGHTLAW_UNABLE_OCI_YOY                    DOUBLE,
    UNABLE_OCI_OTHER                           DOUBLE,
    UNABLE_OCI_OTHER_YOY                       DOUBLE,
    UNABLE_OCI_BALANCE                         DOUBLE,
    UNABLE_OCI_BALANCE_YOY                     DOUBLE,
    ABLE_OCI                                   DOUBLE,
    ABLE_OCI_YOY                               DOUBLE,
    RIGHTLAW_ABLE_OCI                          DOUBLE,
    RIGHTLAW_ABLE_OCI_YOY                      DOUBLE,
    AFA_FAIRVALUE_CHANGE                       DOUBLE,
    AFA_FAIRVALUE_CHANGE_YOY                   DOUBLE,
    HMI_AFA                                    DOUBLE,
    HMI_AFA_YOY                                DOUBLE,
    CASHFLOW_HEDGE_VALID                       DOUBLE,
    CASHFLOW_HEDGE_VALID_YOY                   DOUBLE,
    CREDITOR_FAIRVALUE_CHANGE                  DOUBLE,
    CREDITOR_FAIRVALUE_CHANGE_YOY              DOUBLE,
    CREDITOR_IMPAIRMENT_RESERVE                DOUBLE,
    CREDITOR_IMPAIRMENT_RESERVE_YOY            DOUBLE,
    FINANCE_OCI_AMT                            DOUBLE,
    FINANCE_OCI_AMT_YOY                        DOUBLE,
    CONVERT_DIFF                               DOUBLE,
    CONVERT_DIFF_YOY                           DOUBLE,
    ABLE_OCI_OTHER                             DOUBLE,
    ABLE_OCI_OTHER_YOY                         DOUBLE,
    ABLE_OCI_BALANCE                           DOUBLE,
    ABLE_OCI_BALANCE_YOY                       DOUBLE,
    OCI_OTHER                                  DOUBLE,
    OCI_OTHER_YOY                              DOUBLE,
    OCI_BALANCE                                DOUBLE,
    OCI_BALANCE_YOY                            DOUBLE,
    TOTAL_COMPRE_INCOME                        DOUBLE,
    TOTAL_COMPRE_INCOME_YOY                    DOUBLE,
    PARENT_TCI                                 DOUBLE,
    PARENT_TCI_YOY                             DOUBLE,
    MINORITY_TCI                               DOUBLE,
    MINORITY_TCI_YOY                           DOUBLE,
    PRECOMBINE_TCI                             DOUBLE,
    PRECOMBINE_TCI_YOY                         DOUBLE,
    EFFECT_TCI_BALANCE                         DOUBLE,
    EFFECT_TCI_BALANCE_YOY                     DOUBLE,
    TCI_OTHER                                  DOUBLE,
    TCI_OTHER_YOY                              DOUBLE,
    TCI_BALANCE                                DOUBLE,
    TCI_BALANCE_YOY                            DOUBLE,
    ACF_END_INCOME                             DOUBLE,
    ACF_END_INCOME_YOY                         DOUBLE,
    OPINION_TYPE                               VARCHAR(100)
)"""


def import_to_dolt(csv_path: Path | None = None) -> int:
    """Import the fetched CSV into Dolt fin_income (replace semantics).

    The whole table is atomically rebuilt from the CSV on every import
    (full-refetch contract for the F10 schema, ref #202): the old table is
    renamed aside, a fresh table is created with the 203-field DDL and
    filled via INSERT SELECT; any failure rolls back to the previous data.
    """
    csv_path = csv_path or csv_dir() / f"{REPORT_NAME}.csv"
    print("[import income]", file=sys.stderr)

    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_inc",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} (symbol, report_date, {COLS})
            SELECT
                CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
                CAST(REPORT_DATE AS DATE), {COLS}
            FROM _tmp_inc
            WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
                  IN (SELECT symbol FROM stock_basic)
        """,
        create_sql=_TMP_INC_DDL,
        dolt_table=DOLT_TABLE,
        source_label=f"EastMoney datacenter {REPORT_NAME}",
        last_report_expr="MAX(report_date)",
        merge=False,
    )


if __name__ == "__main__":
    import argparse

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share income statement")
        p.add_argument("--years", default="")
        p.add_argument("--periods", default="Q1,Q2,Q3,FY")
        args = p.parse_args()
        await run(
            years=[int(y) for y in args.years.split(",") if y] or None,
            periods=args.periods,
        )

    asyncio.run(_main())
