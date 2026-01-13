# Calculator Eval Criteria

## Verification Steps

1. **Review test coverage**: Check that `test_calculator.py` contains tests for:
   - `multiply()` function with various inputs
   - `divide()` function with normal division
   - `divide()` with zero divisor (should raise `ValueError`)

2. **Run tests**: Execute `pytest -v` and verify all tests pass

## Pass Criteria

- Tests exist for multiply and divide functions
- Tests cover the division by zero edge case
- All tests pass when run with pytest
