"""Tests for fetch_cash_flow.py — import_to_dolt, run()."""

import asyncio
import csv
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# 254-field header for RPT_F10_FINANCE_GCASHFLOW — generated from
# .dsh/evidence/financial-f10/f10_columns.json GCASHFLOW.fields (JSON order).
_HEADER = [
    "SECUCODE",
    "SECURITY_CODE",
    "SECURITY_NAME_ABBR",
    "ORG_CODE",
    "ORG_TYPE",
    "REPORT_DATE",
    "REPORT_TYPE",
    "REPORT_DATE_NAME",
    "SECURITY_TYPE_CODE",
    "NOTICE_DATE",
    "UPDATE_DATE",
    "CURRENCY",
    "SALES_SERVICES",
    "DEPOSIT_INTERBANK_ADD",
    "LOAN_PBC_ADD",
    "OFI_BF_ADD",
    "RECEIVE_ORIGIC_PREMIUM",
    "RECEIVE_REINSURE_NET",
    "INSURED_INVEST_ADD",
    "DISPOSAL_TFA_ADD",
    "RECEIVE_INTEREST_COMMISSION",
    "BORROW_FUND_ADD",
    "LOAN_ADVANCE_REDUCE",
    "REPO_BUSINESS_ADD",
    "RECEIVE_TAX_REFUND",
    "RECEIVE_OTHER_OPERATE",
    "OPERATE_INFLOW_OTHER",
    "OPERATE_INFLOW_BALANCE",
    "TOTAL_OPERATE_INFLOW",
    "BUY_SERVICES",
    "LOAN_ADVANCE_ADD",
    "PBC_INTERBANK_ADD",
    "PAY_ORIGIC_COMPENSATE",
    "PAY_INTEREST_COMMISSION",
    "PAY_POLICY_BONUS",
    "PAY_STAFF_CASH",
    "PAY_ALL_TAX",
    "PAY_OTHER_OPERATE",
    "OPERATE_OUTFLOW_OTHER",
    "OPERATE_OUTFLOW_BALANCE",
    "TOTAL_OPERATE_OUTFLOW",
    "OPERATE_NETCASH_OTHER",
    "OPERATE_NETCASH_BALANCE",
    "NETCASH_OPERATE",
    "WITHDRAW_INVEST",
    "RECEIVE_INVEST_INCOME",
    "DISPOSAL_LONG_ASSET",
    "DISPOSAL_SUBSIDIARY_OTHER",
    "REDUCE_PLEDGE_TIMEDEPOSITS",
    "RECEIVE_OTHER_INVEST",
    "INVEST_INFLOW_OTHER",
    "INVEST_INFLOW_BALANCE",
    "TOTAL_INVEST_INFLOW",
    "CONSTRUCT_LONG_ASSET",
    "INVEST_PAY_CASH",
    "PLEDGE_LOAN_ADD",
    "OBTAIN_SUBSIDIARY_OTHER",
    "ADD_PLEDGE_TIMEDEPOSITS",
    "PAY_OTHER_INVEST",
    "INVEST_OUTFLOW_OTHER",
    "INVEST_OUTFLOW_BALANCE",
    "TOTAL_INVEST_OUTFLOW",
    "INVEST_NETCASH_OTHER",
    "INVEST_NETCASH_BALANCE",
    "NETCASH_INVEST",
    "ACCEPT_INVEST_CASH",
    "SUBSIDIARY_ACCEPT_INVEST",
    "RECEIVE_LOAN_CASH",
    "ISSUE_BOND",
    "RECEIVE_OTHER_FINANCE",
    "FINANCE_INFLOW_OTHER",
    "FINANCE_INFLOW_BALANCE",
    "TOTAL_FINANCE_INFLOW",
    "PAY_DEBT_CASH",
    "ASSIGN_DIVIDEND_PORFIT",
    "SUBSIDIARY_PAY_DIVIDEND",
    "BUY_SUBSIDIARY_EQUITY",
    "PAY_OTHER_FINANCE",
    "SUBSIDIARY_REDUCE_CASH",
    "FINANCE_OUTFLOW_OTHER",
    "FINANCE_OUTFLOW_BALANCE",
    "TOTAL_FINANCE_OUTFLOW",
    "FINANCE_NETCASH_OTHER",
    "FINANCE_NETCASH_BALANCE",
    "NETCASH_FINANCE",
    "RATE_CHANGE_EFFECT",
    "CCE_ADD_OTHER",
    "CCE_ADD_BALANCE",
    "CCE_ADD",
    "BEGIN_CCE",
    "END_CCE_OTHER",
    "END_CCE_BALANCE",
    "END_CCE",
    "NETPROFIT",
    "ASSET_IMPAIRMENT",
    "FA_IR_DEPR",
    "OILGAS_BIOLOGY_DEPR",
    "IR_DEPR",
    "IA_AMORTIZE",
    "LPE_AMORTIZE",
    "DEFER_INCOME_AMORTIZE",
    "PREPAID_EXPENSE_REDUCE",
    "ACCRUED_EXPENSE_ADD",
    "DISPOSAL_LONGASSET_LOSS",
    "FA_SCRAP_LOSS",
    "FAIRVALUE_CHANGE_LOSS",
    "FINANCE_EXPENSE",
    "INVEST_LOSS",
    "DEFER_TAX",
    "DT_ASSET_REDUCE",
    "DT_LIAB_ADD",
    "PREDICT_LIAB_ADD",
    "INVENTORY_REDUCE",
    "OPERATE_RECE_REDUCE",
    "OPERATE_PAYABLE_ADD",
    "OTHER",
    "OPERATE_NETCASH_OTHERNOTE",
    "OPERATE_NETCASH_BALANCENOTE",
    "NETCASH_OPERATENOTE",
    "DEBT_TRANSFER_CAPITAL",
    "CONVERT_BOND_1YEAR",
    "FINLEASE_OBTAIN_FA",
    "UNINVOLVE_INVESTFIN_OTHER",
    "END_CASH",
    "BEGIN_CASH",
    "END_CASH_EQUIVALENTS",
    "BEGIN_CASH_EQUIVALENTS",
    "CCE_ADD_OTHERNOTE",
    "CCE_ADD_BALANCENOTE",
    "CCE_ADDNOTE",
    "SALES_SERVICES_YOY",
    "DEPOSIT_INTERBANK_ADD_YOY",
    "LOAN_PBC_ADD_YOY",
    "OFI_BF_ADD_YOY",
    "RECEIVE_ORIGIC_PREMIUM_YOY",
    "RECEIVE_REINSURE_NET_YOY",
    "INSURED_INVEST_ADD_YOY",
    "DISPOSAL_TFA_ADD_YOY",
    "RECEIVE_INTEREST_COMMISSION_YOY",
    "BORROW_FUND_ADD_YOY",
    "LOAN_ADVANCE_REDUCE_YOY",
    "REPO_BUSINESS_ADD_YOY",
    "RECEIVE_TAX_REFUND_YOY",
    "RECEIVE_OTHER_OPERATE_YOY",
    "OPERATE_INFLOW_OTHER_YOY",
    "OPERATE_INFLOW_BALANCE_YOY",
    "TOTAL_OPERATE_INFLOW_YOY",
    "BUY_SERVICES_YOY",
    "LOAN_ADVANCE_ADD_YOY",
    "PBC_INTERBANK_ADD_YOY",
    "PAY_ORIGIC_COMPENSATE_YOY",
    "PAY_INTEREST_COMMISSION_YOY",
    "PAY_POLICY_BONUS_YOY",
    "PAY_STAFF_CASH_YOY",
    "PAY_ALL_TAX_YOY",
    "PAY_OTHER_OPERATE_YOY",
    "OPERATE_OUTFLOW_OTHER_YOY",
    "OPERATE_OUTFLOW_BALANCE_YOY",
    "TOTAL_OPERATE_OUTFLOW_YOY",
    "OPERATE_NETCASH_OTHER_YOY",
    "OPERATE_NETCASH_BALANCE_YOY",
    "NETCASH_OPERATE_YOY",
    "WITHDRAW_INVEST_YOY",
    "RECEIVE_INVEST_INCOME_YOY",
    "DISPOSAL_LONG_ASSET_YOY",
    "DISPOSAL_SUBSIDIARY_OTHER_YOY",
    "REDUCE_PLEDGE_TIMEDEPOSITS_YOY",
    "RECEIVE_OTHER_INVEST_YOY",
    "INVEST_INFLOW_OTHER_YOY",
    "INVEST_INFLOW_BALANCE_YOY",
    "TOTAL_INVEST_INFLOW_YOY",
    "CONSTRUCT_LONG_ASSET_YOY",
    "INVEST_PAY_CASH_YOY",
    "PLEDGE_LOAN_ADD_YOY",
    "OBTAIN_SUBSIDIARY_OTHER_YOY",
    "ADD_PLEDGE_TIMEDEPOSITS_YOY",
    "PAY_OTHER_INVEST_YOY",
    "INVEST_OUTFLOW_OTHER_YOY",
    "INVEST_OUTFLOW_BALANCE_YOY",
    "TOTAL_INVEST_OUTFLOW_YOY",
    "INVEST_NETCASH_OTHER_YOY",
    "INVEST_NETCASH_BALANCE_YOY",
    "NETCASH_INVEST_YOY",
    "ACCEPT_INVEST_CASH_YOY",
    "SUBSIDIARY_ACCEPT_INVEST_YOY",
    "RECEIVE_LOAN_CASH_YOY",
    "ISSUE_BOND_YOY",
    "RECEIVE_OTHER_FINANCE_YOY",
    "FINANCE_INFLOW_OTHER_YOY",
    "FINANCE_INFLOW_BALANCE_YOY",
    "TOTAL_FINANCE_INFLOW_YOY",
    "PAY_DEBT_CASH_YOY",
    "ASSIGN_DIVIDEND_PORFIT_YOY",
    "SUBSIDIARY_PAY_DIVIDEND_YOY",
    "BUY_SUBSIDIARY_EQUITY_YOY",
    "PAY_OTHER_FINANCE_YOY",
    "SUBSIDIARY_REDUCE_CASH_YOY",
    "FINANCE_OUTFLOW_OTHER_YOY",
    "FINANCE_OUTFLOW_BALANCE_YOY",
    "TOTAL_FINANCE_OUTFLOW_YOY",
    "FINANCE_NETCASH_OTHER_YOY",
    "FINANCE_NETCASH_BALANCE_YOY",
    "NETCASH_FINANCE_YOY",
    "RATE_CHANGE_EFFECT_YOY",
    "CCE_ADD_OTHER_YOY",
    "CCE_ADD_BALANCE_YOY",
    "CCE_ADD_YOY",
    "BEGIN_CCE_YOY",
    "END_CCE_OTHER_YOY",
    "END_CCE_BALANCE_YOY",
    "END_CCE_YOY",
    "NETPROFIT_YOY",
    "ASSET_IMPAIRMENT_YOY",
    "FA_IR_DEPR_YOY",
    "OILGAS_BIOLOGY_DEPR_YOY",
    "IR_DEPR_YOY",
    "IA_AMORTIZE_YOY",
    "LPE_AMORTIZE_YOY",
    "DEFER_INCOME_AMORTIZE_YOY",
    "PREPAID_EXPENSE_REDUCE_YOY",
    "ACCRUED_EXPENSE_ADD_YOY",
    "DISPOSAL_LONGASSET_LOSS_YOY",
    "FA_SCRAP_LOSS_YOY",
    "FAIRVALUE_CHANGE_LOSS_YOY",
    "FINANCE_EXPENSE_YOY",
    "INVEST_LOSS_YOY",
    "DEFER_TAX_YOY",
    "DT_ASSET_REDUCE_YOY",
    "DT_LIAB_ADD_YOY",
    "PREDICT_LIAB_ADD_YOY",
    "INVENTORY_REDUCE_YOY",
    "OPERATE_RECE_REDUCE_YOY",
    "OPERATE_PAYABLE_ADD_YOY",
    "OTHER_YOY",
    "OPERATE_NETCASH_OTHERNOTE_YOY",
    "OPERATE_NETCASH_BALANCENOTE_YOY",
    "NETCASH_OPERATENOTE_YOY",
    "DEBT_TRANSFER_CAPITAL_YOY",
    "CONVERT_BOND_1YEAR_YOY",
    "FINLEASE_OBTAIN_FA_YOY",
    "UNINVOLVE_INVESTFIN_OTHER_YOY",
    "END_CASH_YOY",
    "BEGIN_CASH_YOY",
    "END_CASH_EQUIVALENTS_YOY",
    "BEGIN_CASH_EQUIVALENTS_YOY",
    "CCE_ADD_OTHERNOTE_YOY",
    "CCE_ADD_BALANCENOTE_YOY",
    "CCE_ADDNOTE_YOY",
    "OPINION_TYPE",
    "OSOPINION_TYPE",
    "MINORITY_INTEREST",
    "MINORITY_INTEREST_YOY",
    "USERIGHT_ASSET_AMORTIZE",
    "USERIGHT_ASSET_AMORTIZE_YOY",
]

# Field type annotations (VARCHAR / DOUBLE) for the same F10 field set.
_FIELD_TYPES = {
    "SECUCODE": "VARCHAR",
    "SECURITY_CODE": "VARCHAR",
    "SECURITY_NAME_ABBR": "VARCHAR",
    "ORG_CODE": "VARCHAR",
    "ORG_TYPE": "VARCHAR",
    "REPORT_DATE": "VARCHAR",
    "REPORT_TYPE": "VARCHAR",
    "REPORT_DATE_NAME": "VARCHAR",
    "SECURITY_TYPE_CODE": "VARCHAR",
    "NOTICE_DATE": "VARCHAR",
    "UPDATE_DATE": "VARCHAR",
    "CURRENCY": "VARCHAR",
    "SALES_SERVICES": "DOUBLE",
    "DEPOSIT_INTERBANK_ADD": "DOUBLE",
    "LOAN_PBC_ADD": "DOUBLE",
    "OFI_BF_ADD": "DOUBLE",
    "RECEIVE_ORIGIC_PREMIUM": "DOUBLE",
    "RECEIVE_REINSURE_NET": "DOUBLE",
    "INSURED_INVEST_ADD": "DOUBLE",
    "DISPOSAL_TFA_ADD": "DOUBLE",
    "RECEIVE_INTEREST_COMMISSION": "DOUBLE",
    "BORROW_FUND_ADD": "DOUBLE",
    "LOAN_ADVANCE_REDUCE": "DOUBLE",
    "REPO_BUSINESS_ADD": "DOUBLE",
    "RECEIVE_TAX_REFUND": "DOUBLE",
    "RECEIVE_OTHER_OPERATE": "DOUBLE",
    "OPERATE_INFLOW_OTHER": "DOUBLE",
    "OPERATE_INFLOW_BALANCE": "DOUBLE",
    "TOTAL_OPERATE_INFLOW": "DOUBLE",
    "BUY_SERVICES": "DOUBLE",
    "LOAN_ADVANCE_ADD": "DOUBLE",
    "PBC_INTERBANK_ADD": "DOUBLE",
    "PAY_ORIGIC_COMPENSATE": "DOUBLE",
    "PAY_INTEREST_COMMISSION": "DOUBLE",
    "PAY_POLICY_BONUS": "DOUBLE",
    "PAY_STAFF_CASH": "DOUBLE",
    "PAY_ALL_TAX": "DOUBLE",
    "PAY_OTHER_OPERATE": "DOUBLE",
    "OPERATE_OUTFLOW_OTHER": "DOUBLE",
    "OPERATE_OUTFLOW_BALANCE": "DOUBLE",
    "TOTAL_OPERATE_OUTFLOW": "DOUBLE",
    "OPERATE_NETCASH_OTHER": "DOUBLE",
    "OPERATE_NETCASH_BALANCE": "DOUBLE",
    "NETCASH_OPERATE": "DOUBLE",
    "WITHDRAW_INVEST": "DOUBLE",
    "RECEIVE_INVEST_INCOME": "DOUBLE",
    "DISPOSAL_LONG_ASSET": "DOUBLE",
    "DISPOSAL_SUBSIDIARY_OTHER": "DOUBLE",
    "REDUCE_PLEDGE_TIMEDEPOSITS": "DOUBLE",
    "RECEIVE_OTHER_INVEST": "DOUBLE",
    "INVEST_INFLOW_OTHER": "DOUBLE",
    "INVEST_INFLOW_BALANCE": "DOUBLE",
    "TOTAL_INVEST_INFLOW": "DOUBLE",
    "CONSTRUCT_LONG_ASSET": "DOUBLE",
    "INVEST_PAY_CASH": "DOUBLE",
    "PLEDGE_LOAN_ADD": "DOUBLE",
    "OBTAIN_SUBSIDIARY_OTHER": "DOUBLE",
    "ADD_PLEDGE_TIMEDEPOSITS": "DOUBLE",
    "PAY_OTHER_INVEST": "DOUBLE",
    "INVEST_OUTFLOW_OTHER": "DOUBLE",
    "INVEST_OUTFLOW_BALANCE": "DOUBLE",
    "TOTAL_INVEST_OUTFLOW": "DOUBLE",
    "INVEST_NETCASH_OTHER": "DOUBLE",
    "INVEST_NETCASH_BALANCE": "DOUBLE",
    "NETCASH_INVEST": "DOUBLE",
    "ACCEPT_INVEST_CASH": "DOUBLE",
    "SUBSIDIARY_ACCEPT_INVEST": "DOUBLE",
    "RECEIVE_LOAN_CASH": "DOUBLE",
    "ISSUE_BOND": "DOUBLE",
    "RECEIVE_OTHER_FINANCE": "DOUBLE",
    "FINANCE_INFLOW_OTHER": "DOUBLE",
    "FINANCE_INFLOW_BALANCE": "DOUBLE",
    "TOTAL_FINANCE_INFLOW": "DOUBLE",
    "PAY_DEBT_CASH": "DOUBLE",
    "ASSIGN_DIVIDEND_PORFIT": "DOUBLE",
    "SUBSIDIARY_PAY_DIVIDEND": "DOUBLE",
    "BUY_SUBSIDIARY_EQUITY": "DOUBLE",
    "PAY_OTHER_FINANCE": "DOUBLE",
    "SUBSIDIARY_REDUCE_CASH": "DOUBLE",
    "FINANCE_OUTFLOW_OTHER": "DOUBLE",
    "FINANCE_OUTFLOW_BALANCE": "DOUBLE",
    "TOTAL_FINANCE_OUTFLOW": "DOUBLE",
    "FINANCE_NETCASH_OTHER": "DOUBLE",
    "FINANCE_NETCASH_BALANCE": "DOUBLE",
    "NETCASH_FINANCE": "DOUBLE",
    "RATE_CHANGE_EFFECT": "DOUBLE",
    "CCE_ADD_OTHER": "DOUBLE",
    "CCE_ADD_BALANCE": "DOUBLE",
    "CCE_ADD": "DOUBLE",
    "BEGIN_CCE": "DOUBLE",
    "END_CCE_OTHER": "DOUBLE",
    "END_CCE_BALANCE": "DOUBLE",
    "END_CCE": "DOUBLE",
    "NETPROFIT": "DOUBLE",
    "ASSET_IMPAIRMENT": "DOUBLE",
    "FA_IR_DEPR": "DOUBLE",
    "OILGAS_BIOLOGY_DEPR": "DOUBLE",
    "IR_DEPR": "DOUBLE",
    "IA_AMORTIZE": "DOUBLE",
    "LPE_AMORTIZE": "DOUBLE",
    "DEFER_INCOME_AMORTIZE": "DOUBLE",
    "PREPAID_EXPENSE_REDUCE": "DOUBLE",
    "ACCRUED_EXPENSE_ADD": "DOUBLE",
    "DISPOSAL_LONGASSET_LOSS": "DOUBLE",
    "FA_SCRAP_LOSS": "DOUBLE",
    "FAIRVALUE_CHANGE_LOSS": "DOUBLE",
    "FINANCE_EXPENSE": "DOUBLE",
    "INVEST_LOSS": "DOUBLE",
    "DEFER_TAX": "DOUBLE",
    "DT_ASSET_REDUCE": "DOUBLE",
    "DT_LIAB_ADD": "DOUBLE",
    "PREDICT_LIAB_ADD": "DOUBLE",
    "INVENTORY_REDUCE": "DOUBLE",
    "OPERATE_RECE_REDUCE": "DOUBLE",
    "OPERATE_PAYABLE_ADD": "DOUBLE",
    "OTHER": "DOUBLE",
    "OPERATE_NETCASH_OTHERNOTE": "DOUBLE",
    "OPERATE_NETCASH_BALANCENOTE": "DOUBLE",
    "NETCASH_OPERATENOTE": "DOUBLE",
    "DEBT_TRANSFER_CAPITAL": "DOUBLE",
    "CONVERT_BOND_1YEAR": "DOUBLE",
    "FINLEASE_OBTAIN_FA": "DOUBLE",
    "UNINVOLVE_INVESTFIN_OTHER": "DOUBLE",
    "END_CASH": "DOUBLE",
    "BEGIN_CASH": "DOUBLE",
    "END_CASH_EQUIVALENTS": "DOUBLE",
    "BEGIN_CASH_EQUIVALENTS": "DOUBLE",
    "CCE_ADD_OTHERNOTE": "DOUBLE",
    "CCE_ADD_BALANCENOTE": "DOUBLE",
    "CCE_ADDNOTE": "DOUBLE",
    "SALES_SERVICES_YOY": "DOUBLE",
    "DEPOSIT_INTERBANK_ADD_YOY": "DOUBLE",
    "LOAN_PBC_ADD_YOY": "DOUBLE",
    "OFI_BF_ADD_YOY": "DOUBLE",
    "RECEIVE_ORIGIC_PREMIUM_YOY": "DOUBLE",
    "RECEIVE_REINSURE_NET_YOY": "DOUBLE",
    "INSURED_INVEST_ADD_YOY": "DOUBLE",
    "DISPOSAL_TFA_ADD_YOY": "DOUBLE",
    "RECEIVE_INTEREST_COMMISSION_YOY": "DOUBLE",
    "BORROW_FUND_ADD_YOY": "DOUBLE",
    "LOAN_ADVANCE_REDUCE_YOY": "DOUBLE",
    "REPO_BUSINESS_ADD_YOY": "DOUBLE",
    "RECEIVE_TAX_REFUND_YOY": "DOUBLE",
    "RECEIVE_OTHER_OPERATE_YOY": "DOUBLE",
    "OPERATE_INFLOW_OTHER_YOY": "DOUBLE",
    "OPERATE_INFLOW_BALANCE_YOY": "DOUBLE",
    "TOTAL_OPERATE_INFLOW_YOY": "DOUBLE",
    "BUY_SERVICES_YOY": "DOUBLE",
    "LOAN_ADVANCE_ADD_YOY": "DOUBLE",
    "PBC_INTERBANK_ADD_YOY": "DOUBLE",
    "PAY_ORIGIC_COMPENSATE_YOY": "DOUBLE",
    "PAY_INTEREST_COMMISSION_YOY": "DOUBLE",
    "PAY_POLICY_BONUS_YOY": "DOUBLE",
    "PAY_STAFF_CASH_YOY": "DOUBLE",
    "PAY_ALL_TAX_YOY": "DOUBLE",
    "PAY_OTHER_OPERATE_YOY": "DOUBLE",
    "OPERATE_OUTFLOW_OTHER_YOY": "DOUBLE",
    "OPERATE_OUTFLOW_BALANCE_YOY": "DOUBLE",
    "TOTAL_OPERATE_OUTFLOW_YOY": "DOUBLE",
    "OPERATE_NETCASH_OTHER_YOY": "DOUBLE",
    "OPERATE_NETCASH_BALANCE_YOY": "DOUBLE",
    "NETCASH_OPERATE_YOY": "DOUBLE",
    "WITHDRAW_INVEST_YOY": "DOUBLE",
    "RECEIVE_INVEST_INCOME_YOY": "DOUBLE",
    "DISPOSAL_LONG_ASSET_YOY": "DOUBLE",
    "DISPOSAL_SUBSIDIARY_OTHER_YOY": "DOUBLE",
    "REDUCE_PLEDGE_TIMEDEPOSITS_YOY": "DOUBLE",
    "RECEIVE_OTHER_INVEST_YOY": "DOUBLE",
    "INVEST_INFLOW_OTHER_YOY": "DOUBLE",
    "INVEST_INFLOW_BALANCE_YOY": "DOUBLE",
    "TOTAL_INVEST_INFLOW_YOY": "DOUBLE",
    "CONSTRUCT_LONG_ASSET_YOY": "DOUBLE",
    "INVEST_PAY_CASH_YOY": "DOUBLE",
    "PLEDGE_LOAN_ADD_YOY": "DOUBLE",
    "OBTAIN_SUBSIDIARY_OTHER_YOY": "DOUBLE",
    "ADD_PLEDGE_TIMEDEPOSITS_YOY": "DOUBLE",
    "PAY_OTHER_INVEST_YOY": "DOUBLE",
    "INVEST_OUTFLOW_OTHER_YOY": "DOUBLE",
    "INVEST_OUTFLOW_BALANCE_YOY": "DOUBLE",
    "TOTAL_INVEST_OUTFLOW_YOY": "DOUBLE",
    "INVEST_NETCASH_OTHER_YOY": "DOUBLE",
    "INVEST_NETCASH_BALANCE_YOY": "DOUBLE",
    "NETCASH_INVEST_YOY": "DOUBLE",
    "ACCEPT_INVEST_CASH_YOY": "DOUBLE",
    "SUBSIDIARY_ACCEPT_INVEST_YOY": "DOUBLE",
    "RECEIVE_LOAN_CASH_YOY": "DOUBLE",
    "ISSUE_BOND_YOY": "DOUBLE",
    "RECEIVE_OTHER_FINANCE_YOY": "DOUBLE",
    "FINANCE_INFLOW_OTHER_YOY": "DOUBLE",
    "FINANCE_INFLOW_BALANCE_YOY": "DOUBLE",
    "TOTAL_FINANCE_INFLOW_YOY": "DOUBLE",
    "PAY_DEBT_CASH_YOY": "DOUBLE",
    "ASSIGN_DIVIDEND_PORFIT_YOY": "DOUBLE",
    "SUBSIDIARY_PAY_DIVIDEND_YOY": "DOUBLE",
    "BUY_SUBSIDIARY_EQUITY_YOY": "DOUBLE",
    "PAY_OTHER_FINANCE_YOY": "DOUBLE",
    "SUBSIDIARY_REDUCE_CASH_YOY": "DOUBLE",
    "FINANCE_OUTFLOW_OTHER_YOY": "DOUBLE",
    "FINANCE_OUTFLOW_BALANCE_YOY": "DOUBLE",
    "TOTAL_FINANCE_OUTFLOW_YOY": "DOUBLE",
    "FINANCE_NETCASH_OTHER_YOY": "DOUBLE",
    "FINANCE_NETCASH_BALANCE_YOY": "DOUBLE",
    "NETCASH_FINANCE_YOY": "DOUBLE",
    "RATE_CHANGE_EFFECT_YOY": "DOUBLE",
    "CCE_ADD_OTHER_YOY": "DOUBLE",
    "CCE_ADD_BALANCE_YOY": "DOUBLE",
    "CCE_ADD_YOY": "DOUBLE",
    "BEGIN_CCE_YOY": "DOUBLE",
    "END_CCE_OTHER_YOY": "DOUBLE",
    "END_CCE_BALANCE_YOY": "DOUBLE",
    "END_CCE_YOY": "DOUBLE",
    "NETPROFIT_YOY": "DOUBLE",
    "ASSET_IMPAIRMENT_YOY": "DOUBLE",
    "FA_IR_DEPR_YOY": "DOUBLE",
    "OILGAS_BIOLOGY_DEPR_YOY": "DOUBLE",
    "IR_DEPR_YOY": "DOUBLE",
    "IA_AMORTIZE_YOY": "DOUBLE",
    "LPE_AMORTIZE_YOY": "DOUBLE",
    "DEFER_INCOME_AMORTIZE_YOY": "DOUBLE",
    "PREPAID_EXPENSE_REDUCE_YOY": "DOUBLE",
    "ACCRUED_EXPENSE_ADD_YOY": "DOUBLE",
    "DISPOSAL_LONGASSET_LOSS_YOY": "DOUBLE",
    "FA_SCRAP_LOSS_YOY": "DOUBLE",
    "FAIRVALUE_CHANGE_LOSS_YOY": "DOUBLE",
    "FINANCE_EXPENSE_YOY": "DOUBLE",
    "INVEST_LOSS_YOY": "DOUBLE",
    "DEFER_TAX_YOY": "DOUBLE",
    "DT_ASSET_REDUCE_YOY": "DOUBLE",
    "DT_LIAB_ADD_YOY": "DOUBLE",
    "PREDICT_LIAB_ADD_YOY": "DOUBLE",
    "INVENTORY_REDUCE_YOY": "DOUBLE",
    "OPERATE_RECE_REDUCE_YOY": "DOUBLE",
    "OPERATE_PAYABLE_ADD_YOY": "DOUBLE",
    "OTHER_YOY": "DOUBLE",
    "OPERATE_NETCASH_OTHERNOTE_YOY": "DOUBLE",
    "OPERATE_NETCASH_BALANCENOTE_YOY": "DOUBLE",
    "NETCASH_OPERATENOTE_YOY": "DOUBLE",
    "DEBT_TRANSFER_CAPITAL_YOY": "DOUBLE",
    "CONVERT_BOND_1YEAR_YOY": "DOUBLE",
    "FINLEASE_OBTAIN_FA_YOY": "DOUBLE",
    "UNINVOLVE_INVESTFIN_OTHER_YOY": "DOUBLE",
    "END_CASH_YOY": "DOUBLE",
    "BEGIN_CASH_YOY": "DOUBLE",
    "END_CASH_EQUIVALENTS_YOY": "DOUBLE",
    "BEGIN_CASH_EQUIVALENTS_YOY": "DOUBLE",
    "CCE_ADD_OTHERNOTE_YOY": "DOUBLE",
    "CCE_ADD_BALANCENOTE_YOY": "DOUBLE",
    "CCE_ADDNOTE_YOY": "DOUBLE",
    "OPINION_TYPE": "VARCHAR",
    "OSOPINION_TYPE": "DOUBLE",
    "MINORITY_INTEREST": "DOUBLE",
    "MINORITY_INTEREST_YOY": "DOUBLE",
    "USERIGHT_ASSET_AMORTIZE": "DOUBLE",
    "USERIGHT_ASSET_AMORTIZE_YOY": "DOUBLE",
}


def _make_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31 00:00:00",
    netcash_operate: str = "500",
) -> list[str]:
    """Build a full 254-field F10 row.

    Every DOUBLE column gets a numeric value, every VARCHAR column a
    placeholder — the import exercises the complete DDL instead of leaving
    columns empty for dolt type inference to guess.
    """
    row: list[str] = []
    for name in _HEADER:
        if name == "SECUCODE":
            row.append(secucode)
        elif name == "SECURITY_CODE":
            row.append(secucode.split(".")[0])
        elif name == "REPORT_DATE":
            row.append(report_date)
        elif name == "NETCASH_OPERATE":
            row.append(netcash_operate)
        elif _FIELD_TYPES[name] == "DOUBLE":
            row.append("1.5")
        else:
            row.append("x")
    return row


# ── import_to_dolt tests ──


class TestImportToDolt:
    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
            capture_output=True,
            text=True,
        )
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.name", "CI"],
            capture_output=True,
            text=True,
        )
        init = subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "init"],
            capture_output=True,
            text=True,
        )
        assert init.returncode == 0, init.stderr

        def dolt_sql_csv(sql: str) -> str:
            return subprocess.run(
                ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
                capture_output=True,
                text=True,
            ).stdout

        dolt_sql_csv(
            "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY); "
            "INSERT INTO stock_basic VALUES ('SZ000001'), ('SZ000002')"
        )
        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        return tmp_path, dolt_sql_csv

    @staticmethod
    def _last(stdout: str) -> str:
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerows(rows)

    def test_first_run_creates_table_and_imports(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "1"

        # DDL: symbol + report_date + 253 F10 data fields (REPORT_DATE is
        # carried by the report_date PK column) = 255 columns total.
        col_count = self._last(
            dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.columns WHERE table_name='fin_cash_flow'"
            )
        )
        assert col_count == str(len(_HEADER) + 1)
        # F10-specific column lands with its value (YOY field not in the old
        # 48-field report).
        yoy = self._last(
            dolt_sql_csv(
                "SELECT NETCASH_OPERATE_YOY FROM fin_cash_flow "
                "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
            )
        )
        assert yoy == "1.5"

        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates WHERE table_name='fin_cash_flow'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

    @staticmethod
    def _table_exists(dolt_sql_csv: Callable[[str], str], table: str) -> bool:
        return (
            dolt_sql_csv(
                f"SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{table}'"
            )
            .strip()
            .split("\n")[-1]
            == "1"
        )

    def test_merge_keeps_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Merge semantics: CSV B appends/upserts, history outside the CSV remains."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row(), _make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 2

        self._write_csv(csv_path, [_make_row(), _make_row(secucode="000002.SZ")])
        rows = import_to_dolt(csv_path)
        assert rows == 3
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "3"
        # 2023-12-31 row was outside the new CSV but is retained by merge.
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT COUNT(*) FROM fin_cash_flow "
                    "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
                )
            )
            == "1"
        )
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT row_count, last_report_date FROM data_updates "
                    "WHERE table_name='fin_cash_flow'"
                )
            )
            == "3,2024-12-31"
        )

    def test_rebuild_applies_restated_value(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Full rebuild applies the restated value (200), unlike merge which
        kept the original 500 (PIN: replace contract).
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(
            csv_path,
            [_make_row(netcash_operate="200"), _make_row(secucode="000002.SZ")],
        )
        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "2"
        val = self._last(
            dolt_sql_csv(
                "SELECT NETCASH_OPERATE FROM fin_cash_flow "
                "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
            )
        )
        assert val == "200"

    def test_same_report_refetch_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Refetching the same report period twice stays at one row (PIN)."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "1"

    def test_merge_watermark_full_total_and_max_date(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Watermark counts the merged table and its max report date."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(csv_path, [_make_row()])
        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT row_count, last_report_date FROM data_updates "
                    "WHERE table_name='fin_cash_flow'"
                )
            )
            == "2,2024-12-31"
        )

    def test_first_run_insert_failure_leaves_empty_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Merge first-run INSERT failure leaves the empty table, no tmp residue."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._table_exists(dolt_sql_csv, "fin_cash_flow")
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "0"
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf")
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf_old")
        assert (
            self._last(
                dolt_sql_csv("SELECT COUNT(*) FROM data_updates WHERE table_name='fin_cash_flow'")
            )
            == "0"
        )

    def test_rerun_insert_failure_preserves_prior_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT preserves prior rows and watermark.

        Replace relies on RENAME rollback, merge on never touching the table —
        both keep the prior row, so this passes for the rebuild contract (PIN).
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "1"
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT NETCASH_OPERATE FROM fin_cash_flow "
                    "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
                )
            )
            == "500"
        )
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf")
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf_old")
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT row_count, last_report_date FROM data_updates "
                    "WHERE table_name='fin_cash_flow'"
                )
            )
            == "1,2024-12-31"
        )

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORT_DATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="FY")

        assert result.name == "RPT_F10_FINANCE_GCASHFLOW.csv"
        csv_path = tmp_path / "RPT_F10_FINANCE_GCASHFLOW.csv"
        assert csv_path.exists()

    async def test_run_default_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Call run() without years — triggers the `if years is None` default path."""
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [], "pages": 1},
            }
        )

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(periods="FY")

        assert result.name == "RPT_F10_FINANCE_GCASHFLOW.csv"

    async def test_run_incremental_since_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date returns a future date, run() returns early."""
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_cash_flow.last_report_date", lambda _tbl: "2099-12-31")

        result = await run(years=[2024], periods="FY")
        assert result.name == "RPT_F10_FINANCE_GCASHFLOW.csv"

    async def test_run_fetch_exception_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() catches and continues."""
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            if call_count[0] <= 4:
                raise RuntimeError("simulated fetch error")
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [], "pages": 1},
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_F10_FINANCE_GCASHFLOW.csv"

    async def test_run_incremental_overwrites_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() overwrites a stale CSV — history lives in Dolt, not CSV (PIN)."""
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_cash_flow.last_report_date", lambda _tbl: "2026-06-30")

        csv_path = tmp_path / "RPT_F10_FINANCE_GCASHFLOW.csv"
        with open(csv_path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(["code", "REPORT_DATE"])
            writer.writerow(["000001", "2024-12-31"])

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORT_DATE": "2026-06-30"}],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q2", page_size=100)

        assert result.name == "RPT_F10_FINANCE_GCASHFLOW.csv"
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]

    async def test_run_incremental_window_starts_at_watermark(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() fetches only dates >= watermark — window starts there (PIN)."""
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_cash_flow.last_report_date", lambda _tbl: "2026-06-30")

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {
                        "data": [{"code": "000001", "REPORT_DATE": "2026-06-30"}],
                        "pages": 1,
                    },
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_F10_FINANCE_GCASHFLOW.csv"
        assert call_count[0] == 1
        csv_path = tmp_path / "RPT_F10_FINANCE_GCASHFLOW.csv"
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]
