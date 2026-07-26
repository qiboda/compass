"""Integration tests for Dolt import logic — uses temp Dolt databases."""

import csv
import subprocess
import tempfile
from pathlib import Path

import pytest


class TestDoltIntegration:
    """Test Dolt table creation and CSV import using temporary databases."""

    @pytest.fixture
    def dolt_dir(self):
        """Create a temporary Dolt database."""
        with tempfile.TemporaryDirectory() as tmp:
            dolt = Path(tmp)
            result = subprocess.run(
                ["dolt", "--data-dir", str(dolt), "init"],
                capture_output=True,
                text=True,
            )
            assert result.returncode == 0, f"dolt init failed: {result.stderr}"
            yield dolt

    def test_create_and_drop_table(self, dolt_dir):
        """Verify basic table operations work."""
        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "sql",
                "-q",
                "CREATE TABLE test_table (id INT PRIMARY KEY, name VARCHAR(50))",
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0

        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "sql",
                "-q",
                "INSERT INTO test_table VALUES (1, 'hello')",
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0

        result = subprocess.run(
            ["dolt", "--data-dir", str(dolt_dir), "sql", "-r", "csv", "-q", "SELECT * FROM test_table"],
            capture_output=True,
            text=True,
        )
        assert "hello" in result.stdout

    def test_csv_import(self, dolt_dir):
        """Verify dolt table import from CSV works."""
        csv_path = dolt_dir / "test.csv"
        with open(csv_path, "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["id", "name"])
            writer.writerow(["1", "test1"])
            writer.writerow(["2", "test2"])

        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "table",
                "import",
                "-c",
                "test_import",
                str(csv_path),
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, f"import failed: {result.stderr}"

        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "sql",
                "-r",
                "csv",
                "-q",
                "SELECT COUNT(*) FROM test_import",
            ],
            capture_output=True,
            text=True,
        )
        lines = result.stdout.strip().split("\n")
        assert lines[-1] == "2"

    def test_stock_basic_table_schema(self, dolt_dir):
        """Verify stock_basic table can be created with correct schema."""
        subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "sql",
                "-q",
                """CREATE TABLE stock_basic (
                    symbol VARCHAR(20) NOT NULL,
                    ts_code VARCHAR(20),
                    code VARCHAR(10),
                    market INT,
                    name VARCHAR(100),
                    list_date VARCHAR(20),
                    industry VARCHAR(50),
                    PRIMARY KEY (symbol)
                )""",
            ],
            capture_output=True,
            text=True,
        )
        subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "sql",
                "-q",
                "INSERT INTO stock_basic VALUES ('SH600519', '600519.SH', '600519', 1, '贵州茅台', '2001-08-27', '白酒Ⅱ')",
            ],
            capture_output=True,
            text=True,
        )

        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(dolt_dir),
                "sql",
                "-r",
                "csv",
                "-q",
                "SELECT name FROM stock_basic WHERE symbol='SH600519'",
            ],
            capture_output=True,
            text=True,
        )
        assert "贵州茅台" in result.stdout
