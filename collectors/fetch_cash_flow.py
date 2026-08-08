#!/usr/bin/env python3
"""A-share cash flow statement collector (现金流量表).

Independent module — uses common.py for shared infrastructure.

Report:  RPT_F10_FINANCE_GCASHFLOW, 254 fields, filter column REPORT_DATE.
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
    fetch_paginated,
    import_replace_table,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_F10_FINANCE_GCASHFLOW"
FILTER_COLUMN = "REPORT_DATE"
DOLT_TABLE = "fin_cash_flow"
START_YEAR = 2020

DDL = """\
CREATE TABLE IF NOT EXISTS fin_cash_flow (
    symbol                VARCHAR(20) NOT NULL,
    report_date           DATE NOT NULL,
    SECUCODE                           VARCHAR(100),
    SECURITY_CODE                      VARCHAR(100),
    SECURITY_NAME_ABBR                 VARCHAR(100),
    ORG_CODE                           VARCHAR(100),
    ORG_TYPE                           VARCHAR(100),
    REPORT_TYPE                        VARCHAR(100),
    REPORT_DATE_NAME                   VARCHAR(100),
    SECURITY_TYPE_CODE                 VARCHAR(100),
    NOTICE_DATE                        VARCHAR(100),
    UPDATE_DATE                        VARCHAR(100),
    CURRENCY                           VARCHAR(100),
    SALES_SERVICES                     DOUBLE,
    DEPOSIT_INTERBANK_ADD              DOUBLE,
    LOAN_PBC_ADD                       DOUBLE,
    OFI_BF_ADD                         DOUBLE,
    RECEIVE_ORIGIC_PREMIUM             DOUBLE,
    RECEIVE_REINSURE_NET               DOUBLE,
    INSURED_INVEST_ADD                 DOUBLE,
    DISPOSAL_TFA_ADD                   DOUBLE,
    RECEIVE_INTEREST_COMMISSION        DOUBLE,
    BORROW_FUND_ADD                    DOUBLE,
    LOAN_ADVANCE_REDUCE                DOUBLE,
    REPO_BUSINESS_ADD                  DOUBLE,
    RECEIVE_TAX_REFUND                 DOUBLE,
    RECEIVE_OTHER_OPERATE              DOUBLE,
    OPERATE_INFLOW_OTHER               DOUBLE,
    OPERATE_INFLOW_BALANCE             DOUBLE,
    TOTAL_OPERATE_INFLOW               DOUBLE,
    BUY_SERVICES                       DOUBLE,
    LOAN_ADVANCE_ADD                   DOUBLE,
    PBC_INTERBANK_ADD                  DOUBLE,
    PAY_ORIGIC_COMPENSATE              DOUBLE,
    PAY_INTEREST_COMMISSION            DOUBLE,
    PAY_POLICY_BONUS                   DOUBLE,
    PAY_STAFF_CASH                     DOUBLE,
    PAY_ALL_TAX                        DOUBLE,
    PAY_OTHER_OPERATE                  DOUBLE,
    OPERATE_OUTFLOW_OTHER              DOUBLE,
    OPERATE_OUTFLOW_BALANCE            DOUBLE,
    TOTAL_OPERATE_OUTFLOW              DOUBLE,
    OPERATE_NETCASH_OTHER              DOUBLE,
    OPERATE_NETCASH_BALANCE            DOUBLE,
    NETCASH_OPERATE                    DOUBLE,
    WITHDRAW_INVEST                    DOUBLE,
    RECEIVE_INVEST_INCOME              DOUBLE,
    DISPOSAL_LONG_ASSET                DOUBLE,
    DISPOSAL_SUBSIDIARY_OTHER          DOUBLE,
    REDUCE_PLEDGE_TIMEDEPOSITS         DOUBLE,
    RECEIVE_OTHER_INVEST               DOUBLE,
    INVEST_INFLOW_OTHER                DOUBLE,
    INVEST_INFLOW_BALANCE              DOUBLE,
    TOTAL_INVEST_INFLOW                DOUBLE,
    CONSTRUCT_LONG_ASSET               DOUBLE,
    INVEST_PAY_CASH                    DOUBLE,
    PLEDGE_LOAN_ADD                    DOUBLE,
    OBTAIN_SUBSIDIARY_OTHER            DOUBLE,
    ADD_PLEDGE_TIMEDEPOSITS            DOUBLE,
    PAY_OTHER_INVEST                   DOUBLE,
    INVEST_OUTFLOW_OTHER               DOUBLE,
    INVEST_OUTFLOW_BALANCE             DOUBLE,
    TOTAL_INVEST_OUTFLOW               DOUBLE,
    INVEST_NETCASH_OTHER               DOUBLE,
    INVEST_NETCASH_BALANCE             DOUBLE,
    NETCASH_INVEST                     DOUBLE,
    ACCEPT_INVEST_CASH                 DOUBLE,
    SUBSIDIARY_ACCEPT_INVEST           DOUBLE,
    RECEIVE_LOAN_CASH                  DOUBLE,
    ISSUE_BOND                         DOUBLE,
    RECEIVE_OTHER_FINANCE              DOUBLE,
    FINANCE_INFLOW_OTHER               DOUBLE,
    FINANCE_INFLOW_BALANCE             DOUBLE,
    TOTAL_FINANCE_INFLOW               DOUBLE,
    PAY_DEBT_CASH                      DOUBLE,
    ASSIGN_DIVIDEND_PORFIT             DOUBLE,
    SUBSIDIARY_PAY_DIVIDEND            DOUBLE,
    BUY_SUBSIDIARY_EQUITY              DOUBLE,
    PAY_OTHER_FINANCE                  DOUBLE,
    SUBSIDIARY_REDUCE_CASH             DOUBLE,
    FINANCE_OUTFLOW_OTHER              DOUBLE,
    FINANCE_OUTFLOW_BALANCE            DOUBLE,
    TOTAL_FINANCE_OUTFLOW              DOUBLE,
    FINANCE_NETCASH_OTHER              DOUBLE,
    FINANCE_NETCASH_BALANCE            DOUBLE,
    NETCASH_FINANCE                    DOUBLE,
    RATE_CHANGE_EFFECT                 DOUBLE,
    CCE_ADD_OTHER                      DOUBLE,
    CCE_ADD_BALANCE                    DOUBLE,
    CCE_ADD                            DOUBLE,
    BEGIN_CCE                          DOUBLE,
    END_CCE_OTHER                      DOUBLE,
    END_CCE_BALANCE                    DOUBLE,
    END_CCE                            DOUBLE,
    NETPROFIT                          DOUBLE,
    ASSET_IMPAIRMENT                   DOUBLE,
    FA_IR_DEPR                         DOUBLE,
    OILGAS_BIOLOGY_DEPR                DOUBLE,
    IR_DEPR                            DOUBLE,
    IA_AMORTIZE                        DOUBLE,
    LPE_AMORTIZE                       DOUBLE,
    DEFER_INCOME_AMORTIZE              DOUBLE,
    PREPAID_EXPENSE_REDUCE             DOUBLE,
    ACCRUED_EXPENSE_ADD                DOUBLE,
    DISPOSAL_LONGASSET_LOSS            DOUBLE,
    FA_SCRAP_LOSS                      DOUBLE,
    FAIRVALUE_CHANGE_LOSS              DOUBLE,
    FINANCE_EXPENSE                    DOUBLE,
    INVEST_LOSS                        DOUBLE,
    DEFER_TAX                          DOUBLE,
    DT_ASSET_REDUCE                    DOUBLE,
    DT_LIAB_ADD                        DOUBLE,
    PREDICT_LIAB_ADD                   DOUBLE,
    INVENTORY_REDUCE                   DOUBLE,
    OPERATE_RECE_REDUCE                DOUBLE,
    OPERATE_PAYABLE_ADD                DOUBLE,
    OTHER                              DOUBLE,
    OPERATE_NETCASH_OTHERNOTE          DOUBLE,
    OPERATE_NETCASH_BALANCENOTE        DOUBLE,
    NETCASH_OPERATENOTE                DOUBLE,
    DEBT_TRANSFER_CAPITAL              DOUBLE,
    CONVERT_BOND_1YEAR                 DOUBLE,
    FINLEASE_OBTAIN_FA                 DOUBLE,
    UNINVOLVE_INVESTFIN_OTHER          DOUBLE,
    END_CASH                           DOUBLE,
    BEGIN_CASH                         DOUBLE,
    END_CASH_EQUIVALENTS               DOUBLE,
    BEGIN_CASH_EQUIVALENTS             DOUBLE,
    CCE_ADD_OTHERNOTE                  DOUBLE,
    CCE_ADD_BALANCENOTE                DOUBLE,
    CCE_ADDNOTE                        DOUBLE,
    SALES_SERVICES_YOY                 DOUBLE,
    DEPOSIT_INTERBANK_ADD_YOY          DOUBLE,
    LOAN_PBC_ADD_YOY                   DOUBLE,
    OFI_BF_ADD_YOY                     DOUBLE,
    RECEIVE_ORIGIC_PREMIUM_YOY         DOUBLE,
    RECEIVE_REINSURE_NET_YOY           DOUBLE,
    INSURED_INVEST_ADD_YOY             DOUBLE,
    DISPOSAL_TFA_ADD_YOY               DOUBLE,
    RECEIVE_INTEREST_COMMISSION_YOY    DOUBLE,
    BORROW_FUND_ADD_YOY                DOUBLE,
    LOAN_ADVANCE_REDUCE_YOY            DOUBLE,
    REPO_BUSINESS_ADD_YOY              DOUBLE,
    RECEIVE_TAX_REFUND_YOY             DOUBLE,
    RECEIVE_OTHER_OPERATE_YOY          DOUBLE,
    OPERATE_INFLOW_OTHER_YOY           DOUBLE,
    OPERATE_INFLOW_BALANCE_YOY         DOUBLE,
    TOTAL_OPERATE_INFLOW_YOY           DOUBLE,
    BUY_SERVICES_YOY                   DOUBLE,
    LOAN_ADVANCE_ADD_YOY               DOUBLE,
    PBC_INTERBANK_ADD_YOY              DOUBLE,
    PAY_ORIGIC_COMPENSATE_YOY          DOUBLE,
    PAY_INTEREST_COMMISSION_YOY        DOUBLE,
    PAY_POLICY_BONUS_YOY               DOUBLE,
    PAY_STAFF_CASH_YOY                 DOUBLE,
    PAY_ALL_TAX_YOY                    DOUBLE,
    PAY_OTHER_OPERATE_YOY              DOUBLE,
    OPERATE_OUTFLOW_OTHER_YOY          DOUBLE,
    OPERATE_OUTFLOW_BALANCE_YOY        DOUBLE,
    TOTAL_OPERATE_OUTFLOW_YOY          DOUBLE,
    OPERATE_NETCASH_OTHER_YOY          DOUBLE,
    OPERATE_NETCASH_BALANCE_YOY        DOUBLE,
    NETCASH_OPERATE_YOY                DOUBLE,
    WITHDRAW_INVEST_YOY                DOUBLE,
    RECEIVE_INVEST_INCOME_YOY          DOUBLE,
    DISPOSAL_LONG_ASSET_YOY            DOUBLE,
    DISPOSAL_SUBSIDIARY_OTHER_YOY      DOUBLE,
    REDUCE_PLEDGE_TIMEDEPOSITS_YOY     DOUBLE,
    RECEIVE_OTHER_INVEST_YOY           DOUBLE,
    INVEST_INFLOW_OTHER_YOY            DOUBLE,
    INVEST_INFLOW_BALANCE_YOY          DOUBLE,
    TOTAL_INVEST_INFLOW_YOY            DOUBLE,
    CONSTRUCT_LONG_ASSET_YOY           DOUBLE,
    INVEST_PAY_CASH_YOY                DOUBLE,
    PLEDGE_LOAN_ADD_YOY                DOUBLE,
    OBTAIN_SUBSIDIARY_OTHER_YOY        DOUBLE,
    ADD_PLEDGE_TIMEDEPOSITS_YOY        DOUBLE,
    PAY_OTHER_INVEST_YOY               DOUBLE,
    INVEST_OUTFLOW_OTHER_YOY           DOUBLE,
    INVEST_OUTFLOW_BALANCE_YOY         DOUBLE,
    TOTAL_INVEST_OUTFLOW_YOY           DOUBLE,
    INVEST_NETCASH_OTHER_YOY           DOUBLE,
    INVEST_NETCASH_BALANCE_YOY         DOUBLE,
    NETCASH_INVEST_YOY                 DOUBLE,
    ACCEPT_INVEST_CASH_YOY             DOUBLE,
    SUBSIDIARY_ACCEPT_INVEST_YOY       DOUBLE,
    RECEIVE_LOAN_CASH_YOY              DOUBLE,
    ISSUE_BOND_YOY                     DOUBLE,
    RECEIVE_OTHER_FINANCE_YOY          DOUBLE,
    FINANCE_INFLOW_OTHER_YOY           DOUBLE,
    FINANCE_INFLOW_BALANCE_YOY         DOUBLE,
    TOTAL_FINANCE_INFLOW_YOY           DOUBLE,
    PAY_DEBT_CASH_YOY                  DOUBLE,
    ASSIGN_DIVIDEND_PORFIT_YOY         DOUBLE,
    SUBSIDIARY_PAY_DIVIDEND_YOY        DOUBLE,
    BUY_SUBSIDIARY_EQUITY_YOY          DOUBLE,
    PAY_OTHER_FINANCE_YOY              DOUBLE,
    SUBSIDIARY_REDUCE_CASH_YOY         DOUBLE,
    FINANCE_OUTFLOW_OTHER_YOY          DOUBLE,
    FINANCE_OUTFLOW_BALANCE_YOY        DOUBLE,
    TOTAL_FINANCE_OUTFLOW_YOY          DOUBLE,
    FINANCE_NETCASH_OTHER_YOY          DOUBLE,
    FINANCE_NETCASH_BALANCE_YOY        DOUBLE,
    NETCASH_FINANCE_YOY                DOUBLE,
    RATE_CHANGE_EFFECT_YOY             DOUBLE,
    CCE_ADD_OTHER_YOY                  DOUBLE,
    CCE_ADD_BALANCE_YOY                DOUBLE,
    CCE_ADD_YOY                        DOUBLE,
    BEGIN_CCE_YOY                      DOUBLE,
    END_CCE_OTHER_YOY                  DOUBLE,
    END_CCE_BALANCE_YOY                DOUBLE,
    END_CCE_YOY                        DOUBLE,
    NETPROFIT_YOY                      DOUBLE,
    ASSET_IMPAIRMENT_YOY               DOUBLE,
    FA_IR_DEPR_YOY                     DOUBLE,
    OILGAS_BIOLOGY_DEPR_YOY            DOUBLE,
    IR_DEPR_YOY                        DOUBLE,
    IA_AMORTIZE_YOY                    DOUBLE,
    LPE_AMORTIZE_YOY                   DOUBLE,
    DEFER_INCOME_AMORTIZE_YOY          DOUBLE,
    PREPAID_EXPENSE_REDUCE_YOY         DOUBLE,
    ACCRUED_EXPENSE_ADD_YOY            DOUBLE,
    DISPOSAL_LONGASSET_LOSS_YOY        DOUBLE,
    FA_SCRAP_LOSS_YOY                  DOUBLE,
    FAIRVALUE_CHANGE_LOSS_YOY          DOUBLE,
    FINANCE_EXPENSE_YOY                DOUBLE,
    INVEST_LOSS_YOY                    DOUBLE,
    DEFER_TAX_YOY                      DOUBLE,
    DT_ASSET_REDUCE_YOY                DOUBLE,
    DT_LIAB_ADD_YOY                    DOUBLE,
    PREDICT_LIAB_ADD_YOY               DOUBLE,
    INVENTORY_REDUCE_YOY               DOUBLE,
    OPERATE_RECE_REDUCE_YOY            DOUBLE,
    OPERATE_PAYABLE_ADD_YOY            DOUBLE,
    OTHER_YOY                          DOUBLE,
    OPERATE_NETCASH_OTHERNOTE_YOY      DOUBLE,
    OPERATE_NETCASH_BALANCENOTE_YOY    DOUBLE,
    NETCASH_OPERATENOTE_YOY            DOUBLE,
    DEBT_TRANSFER_CAPITAL_YOY          DOUBLE,
    CONVERT_BOND_1YEAR_YOY             DOUBLE,
    FINLEASE_OBTAIN_FA_YOY             DOUBLE,
    UNINVOLVE_INVESTFIN_OTHER_YOY      DOUBLE,
    END_CASH_YOY                       DOUBLE,
    BEGIN_CASH_YOY                     DOUBLE,
    END_CASH_EQUIVALENTS_YOY           DOUBLE,
    BEGIN_CASH_EQUIVALENTS_YOY         DOUBLE,
    CCE_ADD_OTHERNOTE_YOY              DOUBLE,
    CCE_ADD_BALANCENOTE_YOY            DOUBLE,
    CCE_ADDNOTE_YOY                    DOUBLE,
    OPINION_TYPE                       VARCHAR(100),
    OSOPINION_TYPE                     DOUBLE,
    MINORITY_INTEREST                  DOUBLE,
    MINORITY_INTEREST_YOY              DOUBLE,
    USERIGHT_ASSET_AMORTIZE            DOUBLE,
    USERIGHT_ASSET_AMORTIZE_YOY        DOUBLE,
    PRIMARY KEY (symbol, report_date)
)"""

COLS = (
    "SECUCODE, SECURITY_CODE, SECURITY_NAME_ABBR, ORG_CODE, "
    "ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, SECURITY_TYPE_CODE, "
    "NOTICE_DATE, UPDATE_DATE, CURRENCY, SALES_SERVICES, "
    "DEPOSIT_INTERBANK_ADD, LOAN_PBC_ADD, OFI_BF_ADD, RECEIVE_ORIGIC_PREMIUM, "
    "RECEIVE_REINSURE_NET, INSURED_INVEST_ADD, DISPOSAL_TFA_ADD, RECEIVE_INTEREST_COMMISSION, "
    "BORROW_FUND_ADD, LOAN_ADVANCE_REDUCE, REPO_BUSINESS_ADD, RECEIVE_TAX_REFUND, "
    "RECEIVE_OTHER_OPERATE, OPERATE_INFLOW_OTHER, OPERATE_INFLOW_BALANCE, TOTAL_OPERATE_INFLOW, "
    "BUY_SERVICES, LOAN_ADVANCE_ADD, PBC_INTERBANK_ADD, PAY_ORIGIC_COMPENSATE, "
    "PAY_INTEREST_COMMISSION, PAY_POLICY_BONUS, PAY_STAFF_CASH, PAY_ALL_TAX, "
    "PAY_OTHER_OPERATE, OPERATE_OUTFLOW_OTHER, OPERATE_OUTFLOW_BALANCE, TOTAL_OPERATE_OUTFLOW, "
    "OPERATE_NETCASH_OTHER, OPERATE_NETCASH_BALANCE, NETCASH_OPERATE, WITHDRAW_INVEST, "
    "RECEIVE_INVEST_INCOME, DISPOSAL_LONG_ASSET, DISPOSAL_SUBSIDIARY_OTHER, REDUCE_PLEDGE_TIMEDEPOSITS, "
    "RECEIVE_OTHER_INVEST, INVEST_INFLOW_OTHER, INVEST_INFLOW_BALANCE, TOTAL_INVEST_INFLOW, "
    "CONSTRUCT_LONG_ASSET, INVEST_PAY_CASH, PLEDGE_LOAN_ADD, OBTAIN_SUBSIDIARY_OTHER, "
    "ADD_PLEDGE_TIMEDEPOSITS, PAY_OTHER_INVEST, INVEST_OUTFLOW_OTHER, INVEST_OUTFLOW_BALANCE, "
    "TOTAL_INVEST_OUTFLOW, INVEST_NETCASH_OTHER, INVEST_NETCASH_BALANCE, NETCASH_INVEST, "
    "ACCEPT_INVEST_CASH, SUBSIDIARY_ACCEPT_INVEST, RECEIVE_LOAN_CASH, ISSUE_BOND, "
    "RECEIVE_OTHER_FINANCE, FINANCE_INFLOW_OTHER, FINANCE_INFLOW_BALANCE, TOTAL_FINANCE_INFLOW, "
    "PAY_DEBT_CASH, ASSIGN_DIVIDEND_PORFIT, SUBSIDIARY_PAY_DIVIDEND, BUY_SUBSIDIARY_EQUITY, "
    "PAY_OTHER_FINANCE, SUBSIDIARY_REDUCE_CASH, FINANCE_OUTFLOW_OTHER, FINANCE_OUTFLOW_BALANCE, "
    "TOTAL_FINANCE_OUTFLOW, FINANCE_NETCASH_OTHER, FINANCE_NETCASH_BALANCE, NETCASH_FINANCE, "
    "RATE_CHANGE_EFFECT, CCE_ADD_OTHER, CCE_ADD_BALANCE, CCE_ADD, "
    "BEGIN_CCE, END_CCE_OTHER, END_CCE_BALANCE, END_CCE, "
    "NETPROFIT, ASSET_IMPAIRMENT, FA_IR_DEPR, OILGAS_BIOLOGY_DEPR, "
    "IR_DEPR, IA_AMORTIZE, LPE_AMORTIZE, DEFER_INCOME_AMORTIZE, "
    "PREPAID_EXPENSE_REDUCE, ACCRUED_EXPENSE_ADD, DISPOSAL_LONGASSET_LOSS, FA_SCRAP_LOSS, "
    "FAIRVALUE_CHANGE_LOSS, FINANCE_EXPENSE, INVEST_LOSS, DEFER_TAX, "
    "DT_ASSET_REDUCE, DT_LIAB_ADD, PREDICT_LIAB_ADD, INVENTORY_REDUCE, "
    "OPERATE_RECE_REDUCE, OPERATE_PAYABLE_ADD, OTHER, OPERATE_NETCASH_OTHERNOTE, "
    "OPERATE_NETCASH_BALANCENOTE, NETCASH_OPERATENOTE, DEBT_TRANSFER_CAPITAL, CONVERT_BOND_1YEAR, "
    "FINLEASE_OBTAIN_FA, UNINVOLVE_INVESTFIN_OTHER, END_CASH, BEGIN_CASH, "
    "END_CASH_EQUIVALENTS, BEGIN_CASH_EQUIVALENTS, CCE_ADD_OTHERNOTE, CCE_ADD_BALANCENOTE, "
    "CCE_ADDNOTE, SALES_SERVICES_YOY, DEPOSIT_INTERBANK_ADD_YOY, LOAN_PBC_ADD_YOY, "
    "OFI_BF_ADD_YOY, RECEIVE_ORIGIC_PREMIUM_YOY, RECEIVE_REINSURE_NET_YOY, INSURED_INVEST_ADD_YOY, "
    "DISPOSAL_TFA_ADD_YOY, RECEIVE_INTEREST_COMMISSION_YOY, BORROW_FUND_ADD_YOY, LOAN_ADVANCE_REDUCE_YOY, "
    "REPO_BUSINESS_ADD_YOY, RECEIVE_TAX_REFUND_YOY, RECEIVE_OTHER_OPERATE_YOY, OPERATE_INFLOW_OTHER_YOY, "
    "OPERATE_INFLOW_BALANCE_YOY, TOTAL_OPERATE_INFLOW_YOY, BUY_SERVICES_YOY, LOAN_ADVANCE_ADD_YOY, "
    "PBC_INTERBANK_ADD_YOY, PAY_ORIGIC_COMPENSATE_YOY, PAY_INTEREST_COMMISSION_YOY, PAY_POLICY_BONUS_YOY, "
    "PAY_STAFF_CASH_YOY, PAY_ALL_TAX_YOY, PAY_OTHER_OPERATE_YOY, OPERATE_OUTFLOW_OTHER_YOY, "
    "OPERATE_OUTFLOW_BALANCE_YOY, TOTAL_OPERATE_OUTFLOW_YOY, OPERATE_NETCASH_OTHER_YOY, OPERATE_NETCASH_BALANCE_YOY, "
    "NETCASH_OPERATE_YOY, WITHDRAW_INVEST_YOY, RECEIVE_INVEST_INCOME_YOY, DISPOSAL_LONG_ASSET_YOY, "
    "DISPOSAL_SUBSIDIARY_OTHER_YOY, REDUCE_PLEDGE_TIMEDEPOSITS_YOY, RECEIVE_OTHER_INVEST_YOY, INVEST_INFLOW_OTHER_YOY, "
    "INVEST_INFLOW_BALANCE_YOY, TOTAL_INVEST_INFLOW_YOY, CONSTRUCT_LONG_ASSET_YOY, INVEST_PAY_CASH_YOY, "
    "PLEDGE_LOAN_ADD_YOY, OBTAIN_SUBSIDIARY_OTHER_YOY, ADD_PLEDGE_TIMEDEPOSITS_YOY, PAY_OTHER_INVEST_YOY, "
    "INVEST_OUTFLOW_OTHER_YOY, INVEST_OUTFLOW_BALANCE_YOY, TOTAL_INVEST_OUTFLOW_YOY, INVEST_NETCASH_OTHER_YOY, "
    "INVEST_NETCASH_BALANCE_YOY, NETCASH_INVEST_YOY, ACCEPT_INVEST_CASH_YOY, SUBSIDIARY_ACCEPT_INVEST_YOY, "
    "RECEIVE_LOAN_CASH_YOY, ISSUE_BOND_YOY, RECEIVE_OTHER_FINANCE_YOY, FINANCE_INFLOW_OTHER_YOY, "
    "FINANCE_INFLOW_BALANCE_YOY, TOTAL_FINANCE_INFLOW_YOY, PAY_DEBT_CASH_YOY, ASSIGN_DIVIDEND_PORFIT_YOY, "
    "SUBSIDIARY_PAY_DIVIDEND_YOY, BUY_SUBSIDIARY_EQUITY_YOY, PAY_OTHER_FINANCE_YOY, SUBSIDIARY_REDUCE_CASH_YOY, "
    "FINANCE_OUTFLOW_OTHER_YOY, FINANCE_OUTFLOW_BALANCE_YOY, TOTAL_FINANCE_OUTFLOW_YOY, FINANCE_NETCASH_OTHER_YOY, "
    "FINANCE_NETCASH_BALANCE_YOY, NETCASH_FINANCE_YOY, RATE_CHANGE_EFFECT_YOY, CCE_ADD_OTHER_YOY, "
    "CCE_ADD_BALANCE_YOY, CCE_ADD_YOY, BEGIN_CCE_YOY, END_CCE_OTHER_YOY, "
    "END_CCE_BALANCE_YOY, END_CCE_YOY, NETPROFIT_YOY, ASSET_IMPAIRMENT_YOY, "
    "FA_IR_DEPR_YOY, OILGAS_BIOLOGY_DEPR_YOY, IR_DEPR_YOY, IA_AMORTIZE_YOY, "
    "LPE_AMORTIZE_YOY, DEFER_INCOME_AMORTIZE_YOY, PREPAID_EXPENSE_REDUCE_YOY, ACCRUED_EXPENSE_ADD_YOY, "
    "DISPOSAL_LONGASSET_LOSS_YOY, FA_SCRAP_LOSS_YOY, FAIRVALUE_CHANGE_LOSS_YOY, FINANCE_EXPENSE_YOY, "
    "INVEST_LOSS_YOY, DEFER_TAX_YOY, DT_ASSET_REDUCE_YOY, DT_LIAB_ADD_YOY, "
    "PREDICT_LIAB_ADD_YOY, INVENTORY_REDUCE_YOY, OPERATE_RECE_REDUCE_YOY, OPERATE_PAYABLE_ADD_YOY, "
    "OTHER_YOY, OPERATE_NETCASH_OTHERNOTE_YOY, OPERATE_NETCASH_BALANCENOTE_YOY, NETCASH_OPERATENOTE_YOY, "
    "DEBT_TRANSFER_CAPITAL_YOY, CONVERT_BOND_1YEAR_YOY, FINLEASE_OBTAIN_FA_YOY, UNINVOLVE_INVESTFIN_OTHER_YOY, "
    "END_CASH_YOY, BEGIN_CASH_YOY, END_CASH_EQUIVALENTS_YOY, BEGIN_CASH_EQUIVALENTS_YOY, "
    "CCE_ADD_OTHERNOTE_YOY, CCE_ADD_BALANCENOTE_YOY, CCE_ADDNOTE_YOY, OPINION_TYPE, "
    "OSOPINION_TYPE, MINORITY_INTEREST, MINORITY_INTEREST_YOY, USERIGHT_ASSET_AMORTIZE, "
    "USERIGHT_ASSET_AMORTIZE_YOY"
)


async def run(
    years: list[int] | None = None,
    periods: str = "Q1,Q2,Q3,FY",
    page_size: int = 100,
) -> Path:
    if years is None:
        years = list(range(START_YEAR, datetime.now().year + 1))

    output_path = Path(f"{REPORT_NAME}.csv")
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
                file=sys.stderr,
                end=" ",
                flush=True,
            )
            try:
                records = await fetch_paginated(
                    session,
                    throttle,
                    REPORT_NAME,
                    FILTER_COLUMN,
                    report_date,
                    page_size,
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
# 65504 bytes, which a 254-column CSV overflows — create the temp table
# explicitly and import with `-u` (see common.py dolt_table_import).
_TMP_CF_DDL = """\
CREATE TABLE _tmp_cf (
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
    SALES_SERVICES                             DOUBLE,
    DEPOSIT_INTERBANK_ADD                      DOUBLE,
    LOAN_PBC_ADD                               DOUBLE,
    OFI_BF_ADD                                 DOUBLE,
    RECEIVE_ORIGIC_PREMIUM                     DOUBLE,
    RECEIVE_REINSURE_NET                       DOUBLE,
    INSURED_INVEST_ADD                         DOUBLE,
    DISPOSAL_TFA_ADD                           DOUBLE,
    RECEIVE_INTEREST_COMMISSION                DOUBLE,
    BORROW_FUND_ADD                            DOUBLE,
    LOAN_ADVANCE_REDUCE                        DOUBLE,
    REPO_BUSINESS_ADD                          DOUBLE,
    RECEIVE_TAX_REFUND                         DOUBLE,
    RECEIVE_OTHER_OPERATE                      DOUBLE,
    OPERATE_INFLOW_OTHER                       DOUBLE,
    OPERATE_INFLOW_BALANCE                     DOUBLE,
    TOTAL_OPERATE_INFLOW                       DOUBLE,
    BUY_SERVICES                               DOUBLE,
    LOAN_ADVANCE_ADD                           DOUBLE,
    PBC_INTERBANK_ADD                          DOUBLE,
    PAY_ORIGIC_COMPENSATE                      DOUBLE,
    PAY_INTEREST_COMMISSION                    DOUBLE,
    PAY_POLICY_BONUS                           DOUBLE,
    PAY_STAFF_CASH                             DOUBLE,
    PAY_ALL_TAX                                DOUBLE,
    PAY_OTHER_OPERATE                          DOUBLE,
    OPERATE_OUTFLOW_OTHER                      DOUBLE,
    OPERATE_OUTFLOW_BALANCE                    DOUBLE,
    TOTAL_OPERATE_OUTFLOW                      DOUBLE,
    OPERATE_NETCASH_OTHER                      DOUBLE,
    OPERATE_NETCASH_BALANCE                    DOUBLE,
    NETCASH_OPERATE                            DOUBLE,
    WITHDRAW_INVEST                            DOUBLE,
    RECEIVE_INVEST_INCOME                      DOUBLE,
    DISPOSAL_LONG_ASSET                        DOUBLE,
    DISPOSAL_SUBSIDIARY_OTHER                  DOUBLE,
    REDUCE_PLEDGE_TIMEDEPOSITS                 DOUBLE,
    RECEIVE_OTHER_INVEST                       DOUBLE,
    INVEST_INFLOW_OTHER                        DOUBLE,
    INVEST_INFLOW_BALANCE                      DOUBLE,
    TOTAL_INVEST_INFLOW                        DOUBLE,
    CONSTRUCT_LONG_ASSET                       DOUBLE,
    INVEST_PAY_CASH                            DOUBLE,
    PLEDGE_LOAN_ADD                            DOUBLE,
    OBTAIN_SUBSIDIARY_OTHER                    DOUBLE,
    ADD_PLEDGE_TIMEDEPOSITS                    DOUBLE,
    PAY_OTHER_INVEST                           DOUBLE,
    INVEST_OUTFLOW_OTHER                       DOUBLE,
    INVEST_OUTFLOW_BALANCE                     DOUBLE,
    TOTAL_INVEST_OUTFLOW                       DOUBLE,
    INVEST_NETCASH_OTHER                       DOUBLE,
    INVEST_NETCASH_BALANCE                     DOUBLE,
    NETCASH_INVEST                             DOUBLE,
    ACCEPT_INVEST_CASH                         DOUBLE,
    SUBSIDIARY_ACCEPT_INVEST                   DOUBLE,
    RECEIVE_LOAN_CASH                          DOUBLE,
    ISSUE_BOND                                 DOUBLE,
    RECEIVE_OTHER_FINANCE                      DOUBLE,
    FINANCE_INFLOW_OTHER                       DOUBLE,
    FINANCE_INFLOW_BALANCE                     DOUBLE,
    TOTAL_FINANCE_INFLOW                       DOUBLE,
    PAY_DEBT_CASH                              DOUBLE,
    ASSIGN_DIVIDEND_PORFIT                     DOUBLE,
    SUBSIDIARY_PAY_DIVIDEND                    DOUBLE,
    BUY_SUBSIDIARY_EQUITY                      DOUBLE,
    PAY_OTHER_FINANCE                          DOUBLE,
    SUBSIDIARY_REDUCE_CASH                     DOUBLE,
    FINANCE_OUTFLOW_OTHER                      DOUBLE,
    FINANCE_OUTFLOW_BALANCE                    DOUBLE,
    TOTAL_FINANCE_OUTFLOW                      DOUBLE,
    FINANCE_NETCASH_OTHER                      DOUBLE,
    FINANCE_NETCASH_BALANCE                    DOUBLE,
    NETCASH_FINANCE                            DOUBLE,
    RATE_CHANGE_EFFECT                         DOUBLE,
    CCE_ADD_OTHER                              DOUBLE,
    CCE_ADD_BALANCE                            DOUBLE,
    CCE_ADD                                    DOUBLE,
    BEGIN_CCE                                  DOUBLE,
    END_CCE_OTHER                              DOUBLE,
    END_CCE_BALANCE                            DOUBLE,
    END_CCE                                    DOUBLE,
    NETPROFIT                                  DOUBLE,
    ASSET_IMPAIRMENT                           DOUBLE,
    FA_IR_DEPR                                 DOUBLE,
    OILGAS_BIOLOGY_DEPR                        DOUBLE,
    IR_DEPR                                    DOUBLE,
    IA_AMORTIZE                                DOUBLE,
    LPE_AMORTIZE                               DOUBLE,
    DEFER_INCOME_AMORTIZE                      DOUBLE,
    PREPAID_EXPENSE_REDUCE                     DOUBLE,
    ACCRUED_EXPENSE_ADD                        DOUBLE,
    DISPOSAL_LONGASSET_LOSS                    DOUBLE,
    FA_SCRAP_LOSS                              DOUBLE,
    FAIRVALUE_CHANGE_LOSS                      DOUBLE,
    FINANCE_EXPENSE                            DOUBLE,
    INVEST_LOSS                                DOUBLE,
    DEFER_TAX                                  DOUBLE,
    DT_ASSET_REDUCE                            DOUBLE,
    DT_LIAB_ADD                                DOUBLE,
    PREDICT_LIAB_ADD                           DOUBLE,
    INVENTORY_REDUCE                           DOUBLE,
    OPERATE_RECE_REDUCE                        DOUBLE,
    OPERATE_PAYABLE_ADD                        DOUBLE,
    OTHER                                      DOUBLE,
    OPERATE_NETCASH_OTHERNOTE                  DOUBLE,
    OPERATE_NETCASH_BALANCENOTE                DOUBLE,
    NETCASH_OPERATENOTE                        DOUBLE,
    DEBT_TRANSFER_CAPITAL                      DOUBLE,
    CONVERT_BOND_1YEAR                         DOUBLE,
    FINLEASE_OBTAIN_FA                         DOUBLE,
    UNINVOLVE_INVESTFIN_OTHER                  DOUBLE,
    END_CASH                                   DOUBLE,
    BEGIN_CASH                                 DOUBLE,
    END_CASH_EQUIVALENTS                       DOUBLE,
    BEGIN_CASH_EQUIVALENTS                     DOUBLE,
    CCE_ADD_OTHERNOTE                          DOUBLE,
    CCE_ADD_BALANCENOTE                        DOUBLE,
    CCE_ADDNOTE                                DOUBLE,
    SALES_SERVICES_YOY                         DOUBLE,
    DEPOSIT_INTERBANK_ADD_YOY                  DOUBLE,
    LOAN_PBC_ADD_YOY                           DOUBLE,
    OFI_BF_ADD_YOY                             DOUBLE,
    RECEIVE_ORIGIC_PREMIUM_YOY                 DOUBLE,
    RECEIVE_REINSURE_NET_YOY                   DOUBLE,
    INSURED_INVEST_ADD_YOY                     DOUBLE,
    DISPOSAL_TFA_ADD_YOY                       DOUBLE,
    RECEIVE_INTEREST_COMMISSION_YOY            DOUBLE,
    BORROW_FUND_ADD_YOY                        DOUBLE,
    LOAN_ADVANCE_REDUCE_YOY                    DOUBLE,
    REPO_BUSINESS_ADD_YOY                      DOUBLE,
    RECEIVE_TAX_REFUND_YOY                     DOUBLE,
    RECEIVE_OTHER_OPERATE_YOY                  DOUBLE,
    OPERATE_INFLOW_OTHER_YOY                   DOUBLE,
    OPERATE_INFLOW_BALANCE_YOY                 DOUBLE,
    TOTAL_OPERATE_INFLOW_YOY                   DOUBLE,
    BUY_SERVICES_YOY                           DOUBLE,
    LOAN_ADVANCE_ADD_YOY                       DOUBLE,
    PBC_INTERBANK_ADD_YOY                      DOUBLE,
    PAY_ORIGIC_COMPENSATE_YOY                  DOUBLE,
    PAY_INTEREST_COMMISSION_YOY                DOUBLE,
    PAY_POLICY_BONUS_YOY                       DOUBLE,
    PAY_STAFF_CASH_YOY                         DOUBLE,
    PAY_ALL_TAX_YOY                            DOUBLE,
    PAY_OTHER_OPERATE_YOY                      DOUBLE,
    OPERATE_OUTFLOW_OTHER_YOY                  DOUBLE,
    OPERATE_OUTFLOW_BALANCE_YOY                DOUBLE,
    TOTAL_OPERATE_OUTFLOW_YOY                  DOUBLE,
    OPERATE_NETCASH_OTHER_YOY                  DOUBLE,
    OPERATE_NETCASH_BALANCE_YOY                DOUBLE,
    NETCASH_OPERATE_YOY                        DOUBLE,
    WITHDRAW_INVEST_YOY                        DOUBLE,
    RECEIVE_INVEST_INCOME_YOY                  DOUBLE,
    DISPOSAL_LONG_ASSET_YOY                    DOUBLE,
    DISPOSAL_SUBSIDIARY_OTHER_YOY              DOUBLE,
    REDUCE_PLEDGE_TIMEDEPOSITS_YOY             DOUBLE,
    RECEIVE_OTHER_INVEST_YOY                   DOUBLE,
    INVEST_INFLOW_OTHER_YOY                    DOUBLE,
    INVEST_INFLOW_BALANCE_YOY                  DOUBLE,
    TOTAL_INVEST_INFLOW_YOY                    DOUBLE,
    CONSTRUCT_LONG_ASSET_YOY                   DOUBLE,
    INVEST_PAY_CASH_YOY                        DOUBLE,
    PLEDGE_LOAN_ADD_YOY                        DOUBLE,
    OBTAIN_SUBSIDIARY_OTHER_YOY                DOUBLE,
    ADD_PLEDGE_TIMEDEPOSITS_YOY                DOUBLE,
    PAY_OTHER_INVEST_YOY                       DOUBLE,
    INVEST_OUTFLOW_OTHER_YOY                   DOUBLE,
    INVEST_OUTFLOW_BALANCE_YOY                 DOUBLE,
    TOTAL_INVEST_OUTFLOW_YOY                   DOUBLE,
    INVEST_NETCASH_OTHER_YOY                   DOUBLE,
    INVEST_NETCASH_BALANCE_YOY                 DOUBLE,
    NETCASH_INVEST_YOY                         DOUBLE,
    ACCEPT_INVEST_CASH_YOY                     DOUBLE,
    SUBSIDIARY_ACCEPT_INVEST_YOY               DOUBLE,
    RECEIVE_LOAN_CASH_YOY                      DOUBLE,
    ISSUE_BOND_YOY                             DOUBLE,
    RECEIVE_OTHER_FINANCE_YOY                  DOUBLE,
    FINANCE_INFLOW_OTHER_YOY                   DOUBLE,
    FINANCE_INFLOW_BALANCE_YOY                 DOUBLE,
    TOTAL_FINANCE_INFLOW_YOY                   DOUBLE,
    PAY_DEBT_CASH_YOY                          DOUBLE,
    ASSIGN_DIVIDEND_PORFIT_YOY                 DOUBLE,
    SUBSIDIARY_PAY_DIVIDEND_YOY                DOUBLE,
    BUY_SUBSIDIARY_EQUITY_YOY                  DOUBLE,
    PAY_OTHER_FINANCE_YOY                      DOUBLE,
    SUBSIDIARY_REDUCE_CASH_YOY                 DOUBLE,
    FINANCE_OUTFLOW_OTHER_YOY                  DOUBLE,
    FINANCE_OUTFLOW_BALANCE_YOY                DOUBLE,
    TOTAL_FINANCE_OUTFLOW_YOY                  DOUBLE,
    FINANCE_NETCASH_OTHER_YOY                  DOUBLE,
    FINANCE_NETCASH_BALANCE_YOY                DOUBLE,
    NETCASH_FINANCE_YOY                        DOUBLE,
    RATE_CHANGE_EFFECT_YOY                     DOUBLE,
    CCE_ADD_OTHER_YOY                          DOUBLE,
    CCE_ADD_BALANCE_YOY                        DOUBLE,
    CCE_ADD_YOY                                DOUBLE,
    BEGIN_CCE_YOY                              DOUBLE,
    END_CCE_OTHER_YOY                          DOUBLE,
    END_CCE_BALANCE_YOY                        DOUBLE,
    END_CCE_YOY                                DOUBLE,
    NETPROFIT_YOY                              DOUBLE,
    ASSET_IMPAIRMENT_YOY                       DOUBLE,
    FA_IR_DEPR_YOY                             DOUBLE,
    OILGAS_BIOLOGY_DEPR_YOY                    DOUBLE,
    IR_DEPR_YOY                                DOUBLE,
    IA_AMORTIZE_YOY                            DOUBLE,
    LPE_AMORTIZE_YOY                           DOUBLE,
    DEFER_INCOME_AMORTIZE_YOY                  DOUBLE,
    PREPAID_EXPENSE_REDUCE_YOY                 DOUBLE,
    ACCRUED_EXPENSE_ADD_YOY                    DOUBLE,
    DISPOSAL_LONGASSET_LOSS_YOY                DOUBLE,
    FA_SCRAP_LOSS_YOY                          DOUBLE,
    FAIRVALUE_CHANGE_LOSS_YOY                  DOUBLE,
    FINANCE_EXPENSE_YOY                        DOUBLE,
    INVEST_LOSS_YOY                            DOUBLE,
    DEFER_TAX_YOY                              DOUBLE,
    DT_ASSET_REDUCE_YOY                        DOUBLE,
    DT_LIAB_ADD_YOY                            DOUBLE,
    PREDICT_LIAB_ADD_YOY                       DOUBLE,
    INVENTORY_REDUCE_YOY                       DOUBLE,
    OPERATE_RECE_REDUCE_YOY                    DOUBLE,
    OPERATE_PAYABLE_ADD_YOY                    DOUBLE,
    OTHER_YOY                                  DOUBLE,
    OPERATE_NETCASH_OTHERNOTE_YOY              DOUBLE,
    OPERATE_NETCASH_BALANCENOTE_YOY            DOUBLE,
    NETCASH_OPERATENOTE_YOY                    DOUBLE,
    DEBT_TRANSFER_CAPITAL_YOY                  DOUBLE,
    CONVERT_BOND_1YEAR_YOY                     DOUBLE,
    FINLEASE_OBTAIN_FA_YOY                     DOUBLE,
    UNINVOLVE_INVESTFIN_OTHER_YOY              DOUBLE,
    END_CASH_YOY                               DOUBLE,
    BEGIN_CASH_YOY                             DOUBLE,
    END_CASH_EQUIVALENTS_YOY                   DOUBLE,
    BEGIN_CASH_EQUIVALENTS_YOY                 DOUBLE,
    CCE_ADD_OTHERNOTE_YOY                      DOUBLE,
    CCE_ADD_BALANCENOTE_YOY                    DOUBLE,
    CCE_ADDNOTE_YOY                            DOUBLE,
    OPINION_TYPE                               VARCHAR(100),
    OSOPINION_TYPE                             DOUBLE,
    MINORITY_INTEREST                          DOUBLE,
    MINORITY_INTEREST_YOY                      DOUBLE,
    USERIGHT_ASSET_AMORTIZE                    DOUBLE,
    USERIGHT_ASSET_AMORTIZE_YOY                DOUBLE
)"""


def import_to_dolt(csv_path: Path | None = None) -> int:
    """Import the fetched CSV into Dolt fin_cash_flow (full rebuild semantics).

    The table is atomically REPLACED with the CSV contents: old rows outside
    the CSV disappear, restated values in the CSV win (ref #202). Rows are
    deduped by the PK (symbol, report_date).
    """
    csv_path = csv_path or Path(f"{REPORT_NAME}.csv")
    print("[import cash_flow]", file=sys.stderr)

    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_cf",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} (symbol, report_date, {COLS})
            SELECT
                CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
                CAST(REPORT_DATE AS DATE), {COLS}
            FROM _tmp_cf
            WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
                  IN (SELECT symbol FROM stock_basic)
        """,
        create_sql=_TMP_CF_DDL,
        dolt_table=DOLT_TABLE,
        source_label=f"EastMoney datacenter {REPORT_NAME}",
        last_report_expr="MAX(report_date)",
        merge=False,
    )


if __name__ == "__main__":
    import argparse

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share cash flow statement")
        p.add_argument("--years", default="")
        p.add_argument("--periods", default="Q1,Q2,Q3,FY")
        args = p.parse_args()
        await run(
            years=[int(y) for y in args.years.split(",") if y] or None,
            periods=args.periods,
        )

    asyncio.run(_main())
