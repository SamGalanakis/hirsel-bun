# Todo API

Build a REST API for managing todo items using FastAPI and SQLite.

## Tech Stack

- **Framework**: FastAPI
- **Database**: SQLite with raw SQL (no ORM)
- **Server**: uvicorn

## Database Schema

```sql
CREATE TABLE todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT,
    completed INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT
);
```

## API Endpoints

### GET /health
Health check endpoint.

**Response**: `200 OK`
```json
{"status": "ok"}
```

### GET /todos
List all todos, optionally filtered by completion status.

**Query Parameters**:
- `completed` (optional): `true` or `false` to filter

**Response**: `200 OK`
```json
{
  "todos": [
    {
      "id": 1,
      "title": "Buy groceries",
      "description": "Milk, eggs, bread",
      "completed": false,
      "created_at": "2024-01-15T10:30:00",
      "updated_at": null
    }
  ]
}
```

### POST /todos
Create a new todo.

**Request Body**:
```json
{
  "title": "Buy groceries",
  "description": "Milk, eggs, bread"
}
```
- `title` is required
- `description` is optional

**Response**: `201 Created`
```json
{
  "id": 1,
  "title": "Buy groceries",
  "description": "Milk, eggs, bread",
  "completed": false,
  "created_at": "2024-01-15T10:30:00",
  "updated_at": null
}
```

**Error Response**: `400 Bad Request` if title is missing
```json
{"detail": "Title is required"}
```

### GET /todos/{id}
Get a specific todo by ID.

**Response**: `200 OK`
```json
{
  "id": 1,
  "title": "Buy groceries",
  "description": "Milk, eggs, bread",
  "completed": false,
  "created_at": "2024-01-15T10:30:00",
  "updated_at": null
}
```

**Error Response**: `404 Not Found`
```json
{"detail": "Todo not found"}
```

### PUT /todos/{id}
Update an existing todo.

**Request Body** (all fields optional):
```json
{
  "title": "Buy groceries updated",
  "description": "Milk, eggs, bread, cheese",
  "completed": true
}
```

**Response**: `200 OK` - returns the updated todo

**Error Response**: `404 Not Found` if todo doesn't exist

### DELETE /todos/{id}
Delete a todo.

**Response**: `204 No Content`

**Error Response**: `404 Not Found` if todo doesn't exist

### DELETE /todos
Delete all completed todos (bulk cleanup).

**Response**: `200 OK`
```json
{"deleted_count": 3}
```

## Project Structure

```
project/
├── pyproject.toml      # uv project config
├── main.py             # FastAPI app with all endpoints
├── database.py         # SQLite connection and queries
└── models.py           # Pydantic models for request/response
```

## Requirements

1. Use `uv` for dependency management
2. All endpoints must match the spec exactly
3. Use ISO 8601 format for timestamps
4. Database file should be `todos.db` in the working directory
5. Server should run on port 8000
