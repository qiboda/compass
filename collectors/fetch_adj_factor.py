#!/usr/bin/env python3
"""Fetch adj_factor from Baostock for a given stock code and date range.

Usage: python3 scripts/fetch_adj_factor.py <ts_code> <start_date> <end_date>
Example: python3 scripts/fetch_adj_factor.py 000001.SZ 20200101 20250722

Output: JSON array to stdout: [{"trade_date":"20200102","adj_factor":1.0},...]
Exit: 0 on success, 1 on error
"""

import json
import sys

import baostock as bs

code = sys.argv[1]
start = sys.argv[2]
end = sys.argv[3]

# Convert ts_code to Baostock format: 000001.SZ → sz.000001
if code.endswith('.SZ'):
    bs_code = f'sz.{code[:-3]}'
elif code.endswith('.SH'):
    bs_code = f'sh.{code[:-3]}'
elif code.endswith('.BJ'):
    bs_code = f'bj.{code[:-3]}'
else:
    print(json.dumps({"error": f"Unknown exchange: {code}"}), file=sys.stderr)
    sys.exit(1)

lg = bs.login()
if lg.error_code != '0':
    print(json.dumps({"error": f"Login failed: {lg.error_msg}"}), file=sys.stderr)
    sys.exit(1)

rs = bs.query_adjust_factor(code=bs_code, start_date=start, end_date=end)
if rs.error_code != '0':
    print(json.dumps({"error": f"Query failed: {rs.error_msg}"}), file=sys.stderr)
    sys.exit(1)

items = []
while rs.next():
    row = rs.get_row_data()
    items.append({
        "trade_date": row[0],       # date
        "adj_factor": float(row[3])  # adj_factor (column index 3)
    })

bs.logout()
print(json.dumps(items))
