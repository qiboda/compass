.PHONY: test lint fmt check ci

test:
	uv run pytest tests/ -v

test-cov:
	uv run pytest tests/ -v --cov=. --cov-report=term-missing

lint:
	uv run ruff check *.py tests/

fmt:
	uv run ruff format *.py tests/
	uv run ruff check *.py tests/ --fix --unsafe-fixes

check: lint test
	@echo "All checks passed."

ci: check
	@echo "CI passed."
