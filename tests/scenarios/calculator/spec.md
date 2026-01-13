# Calculator Enhancement

## Goal
Add multiply and divide functions to the calculator module.

## Requirements

1. **multiply(a, b)**: Return the product of two integers
2. **divide(a, b)**: Return the quotient of a divided by b
   - Must handle division by zero (raise `ValueError` with message "Cannot divide by zero")
   - Return a float for accurate division

## Acceptance Criteria

- All existing tests pass
- New tests added for multiply and divide
- divide(10, 0) raises ValueError
- divide(10, 4) returns 2.5 (float division)

## Files to Modify

- `calculator.py` - add the new functions
- `test_calculator.py` - add tests for new functions
