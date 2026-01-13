# Todo API Evaluation

Verify the API works correctly. You may use any method to test (Python requests/httpx, curl, etc.).

## Setup

Start the server before running tests:
```bash
uv run uvicorn main:app --host 0.0.0.0 --port 8000 &
sleep 2  # Wait for server to start
```

## Required Endpoints & Behavior

### Health Check
- `GET /health` returns `{"status": "ok"}`

### Create Todo
- `POST /todos` with `{"title": "...", "description": "..."}` (description optional)
- Returns created todo with `id`, `title`, `completed=false`, `created_at`
- Returns 400/422 if title missing

### List Todos
- `GET /todos` returns `{"todos": [...]}`
- `GET /todos?completed=true` filters to completed only
- `GET /todos?completed=false` filters to incomplete only

### Get Single Todo
- `GET /todos/{id}` returns the todo
- Returns 404 if not found

### Update Todo
- `PUT /todos/{id}` with partial update data
- Can update `title`, `description`, `completed`
- Sets `updated_at` timestamp
- Returns 404 if not found

### Delete Todo
- `DELETE /todos/{id}` deletes single todo, returns 204
- Returns 404 if not found

### Bulk Delete Completed
- `DELETE /todos` deletes all completed todos
- Returns `{"deleted_count": N}`

## Success Criteria

All endpoints must:
- Return proper HTTP status codes (200, 201, 204, 400, 404, 422)
- Return JSON in the expected format
- Handle errors gracefully
- Maintain data consistency

Test each endpoint with valid and invalid inputs.
