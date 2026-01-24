# Multi-File Creation

## Goal
Create three Python utility modules in parallel.

## Requirements

Create the following files:

1. **math_utils.py**: Contains `square(n)` that returns n*n
2. **string_utils.py**: Contains `reverse(s)` that returns the reversed string
3. **list_utils.py**: Contains `first(lst)` that returns the first element or None if empty

Each file should have a simple implementation - one function each.

## Worker Assignment

This task is suitable for parallel execution:
- Worker 1: math_utils.py
- Worker 2: string_utils.py
- Worker 3: list_utils.py

## Acceptance Criteria

- All three files exist
- Each function works correctly
- No dependencies between files
