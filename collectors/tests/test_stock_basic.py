"""Unit tests for stock_basic utilities — symbol/ts_code/exchange logic."""

from fetch_stock_basic import infer_exchange, to_symbol, to_ts_code, ts_to_date


class TestInferExchange:
    def test_shanghai_prefix_6(self):
        assert infer_exchange("600519") == "SH"
        assert infer_exchange("601318") == "SH"
        assert infer_exchange("688001") == "SH"

    def test_beijing_prefix_8(self):
        assert infer_exchange("830799") == "BJ"
        assert infer_exchange("836149") == "BJ"
        assert infer_exchange("873169") == "BJ"

    def test_shenzhen_default(self):
        assert infer_exchange("000001") == "SZ"
        assert infer_exchange("300750") == "SZ"
        assert infer_exchange("002415") == "SZ"


class TestToSymbol:
    def test_shanghai(self):
        assert to_symbol("600519") == "SH600519"

    def test_shenzhen(self):
        assert to_symbol("000001") == "SZ000001"

    def test_beijing(self):
        assert to_symbol("830799") == "BJ830799"


class TestToTsCode:
    def test_shanghai(self):
        assert to_ts_code("600519") == "600519.SH"

    def test_shenzhen(self):
        assert to_ts_code("000001") == "000001.SZ"

    def test_beijing(self):
        assert to_ts_code("830799") == "830799.BJ"


class TestTsToDate:
    def test_valid_timestamp(self):
        result = ts_to_date(997920000)
        assert result == "2001-08-15" or result == "2001-08-16"  # timezone-dependent

    def test_zero_timestamp(self):
        assert ts_to_date(0) == ""

    def test_none_timestamp(self):
        assert ts_to_date(None) == ""

    def test_negative_timestamp(self):
        assert ts_to_date(-1) == ""
