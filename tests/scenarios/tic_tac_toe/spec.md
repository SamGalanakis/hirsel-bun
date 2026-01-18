# Tic-Tac-Toe Game

## Goal
Build a complete tic-tac-toe game with a computer opponent.

## Requirements

1. **Game Board**
   - 3x3 grid
   - Display current board state
   - Track X and O positions

2. **Game Logic**
   - `make_move(board, position, player)`: Place X or O at position (0-8)
   - `check_winner(board)`: Return winner ('X', 'O'), 'draw', or None if game continues
   - `get_valid_moves(board)`: Return list of available positions

3. **Computer AI**
   - `computer_move(board, player)`: Return best move for computer
   - Should block opponent wins
   - Should take winning moves when available

4. **Main Game**
   - Human plays as X, computer as O
   - Alternate turns until win or draw
   - Display board after each move

## Acceptance Criteria

- All win conditions detected (rows, columns, diagonals)
- Computer never loses (plays optimally or near-optimally)
- Invalid moves are rejected
- Game ends correctly on win or draw

## Files to Create

- `tic_tac_toe.py` - game logic and AI
- `test_tic_tac_toe.py` - unit tests
