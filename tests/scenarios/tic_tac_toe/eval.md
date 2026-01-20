# Tic-Tac-Toe Evaluation

## Test Criteria

1. **Win Detection**: All 8 win conditions work (3 rows, 3 cols, 2 diagonals)
2. **Draw Detection**: Full board with no winner returns 'draw'
3. **Valid Moves**: Only empty positions returned
4. **Computer Blocks**: AI blocks opponent's winning move
5. **Computer Wins**: AI takes winning move when available
6. **Tests Pass**: All unit tests pass

## How to Evaluate

Run: `python -m pytest test_tic_tac_toe.py -v`

Check that the computer AI:
- Blocks when opponent has 2 in a row
- Wins when it has 2 in a row
