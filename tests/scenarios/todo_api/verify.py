#!/usr/bin/env python3
"""Verification script for todo_api scenario.

Starts the server, runs curl-based tests, and reports results.
"""

import json
import subprocess
import sys
import time
from pathlib import Path

BASE_URL = "http://localhost:8000"


def run_curl(
    method: str, path: str, data: dict | None = None, expected_status: int | None = None
) -> tuple[int, dict | str]:
    """Run a curl command and return (status_code, response_body)."""
    url = f"{BASE_URL}{path}"
    cmd = ["curl", "-s", "-w", "\n%{http_code}", "-X", method, url]

    if data is not None:
        cmd.extend(["-H", "Content-Type: application/json", "-d", json.dumps(data)])

    result = subprocess.run(cmd, capture_output=True, text=True)
    lines = result.stdout.strip().split("\n")
    status_code = int(lines[-1])
    body = "\n".join(lines[:-1])

    try:
        body = json.loads(body) if body else {}
    except json.JSONDecodeError:
        pass

    return status_code, body


def test_health():
    """Test health endpoint."""
    status, body = run_curl("GET", "/health")
    assert status == 200, f"Expected 200, got {status}"
    assert body.get("status") == "ok", f"Expected status=ok, got {body}"
    return True


def test_create_todo():
    """Test creating a todo."""
    status, body = run_curl(
        "POST", "/todos", {"title": "Test todo", "description": "Test desc"}
    )
    assert status == 201, f"Expected 201, got {status}"
    assert body.get("id"), "Expected id in response"
    assert body.get("title") == "Test todo", f"Expected title='Test todo', got {body}"
    assert body.get("completed") is False, f"Expected completed=false, got {body}"
    return True


def test_create_todo_no_description():
    """Test creating todo without description."""
    status, body = run_curl("POST", "/todos", {"title": "No description"})
    assert status == 201, f"Expected 201, got {status}"
    assert body.get("id"), "Expected id in response"
    return True


def test_create_todo_missing_title():
    """Test creating todo without title fails."""
    status, body = run_curl("POST", "/todos", {"description": "Missing title"})
    assert status in (400, 422), f"Expected 400 or 422, got {status}"
    return True


def test_list_todos():
    """Test listing todos."""
    status, body = run_curl("GET", "/todos")
    assert status == 200, f"Expected 200, got {status}"
    assert "todos" in body, f"Expected 'todos' key, got {body}"
    assert isinstance(body["todos"], list), f"Expected list, got {type(body['todos'])}"
    return True


def test_get_todo():
    """Test getting a single todo."""
    # Create one
    _, created = run_curl("POST", "/todos", {"title": "Fetch me"})
    todo_id = created["id"]

    # Fetch it
    status, body = run_curl("GET", f"/todos/{todo_id}")
    assert status == 200, f"Expected 200, got {status}"
    assert body.get("title") == "Fetch me", f"Expected title='Fetch me', got {body}"
    return True


def test_get_nonexistent_todo():
    """Test getting nonexistent todo returns 404."""
    status, _ = run_curl("GET", "/todos/99999")
    assert status == 404, f"Expected 404, got {status}"
    return True


def test_update_todo():
    """Test updating a todo."""
    # Create
    _, created = run_curl("POST", "/todos", {"title": "Update me"})
    todo_id = created["id"]

    # Update
    status, body = run_curl(
        "PUT", f"/todos/{todo_id}", {"title": "Updated", "completed": True}
    )
    assert status == 200, f"Expected 200, got {status}"
    assert body.get("title") == "Updated", f"Expected title='Updated', got {body}"
    assert body.get("completed") is True, f"Expected completed=true, got {body}"
    return True


def test_update_sets_timestamp():
    """Test that update sets updated_at."""
    # Create
    _, created = run_curl("POST", "/todos", {"title": "Timestamp test"})
    todo_id = created["id"]
    assert created.get("updated_at") is None, "Expected updated_at=null on create"

    # Update
    status, body = run_curl("PUT", f"/todos/{todo_id}", {"title": "Timestamped"})
    assert status == 200, f"Expected 200, got {status}"
    assert (
        body.get("updated_at") is not None
    ), f"Expected updated_at to be set, got {body}"
    return True


def test_delete_todo():
    """Test deleting a todo."""
    # Create
    _, created = run_curl("POST", "/todos", {"title": "Delete me"})
    todo_id = created["id"]

    # Delete
    status, _ = run_curl("DELETE", f"/todos/{todo_id}")
    assert status == 204, f"Expected 204, got {status}"

    # Verify gone
    status, _ = run_curl("GET", f"/todos/{todo_id}")
    assert status == 404, f"Expected 404 after delete, got {status}"
    return True


def test_delete_nonexistent():
    """Test deleting nonexistent todo returns 404."""
    status, _ = run_curl("DELETE", "/todos/99999")
    assert status == 404, f"Expected 404, got {status}"
    return True


def test_filter_completed():
    """Test filtering by completed status."""
    # Create incomplete
    run_curl("POST", "/todos", {"title": "Incomplete"})

    # Create and complete one
    _, created = run_curl("POST", "/todos", {"title": "Will complete"})
    run_curl("PUT", f"/todos/{created['id']}", {"completed": True})

    # Filter completed=true
    status, body = run_curl("GET", "/todos?completed=true")
    assert status == 200, f"Expected 200, got {status}"
    for todo in body.get("todos", []):
        assert todo.get("completed") is True, f"Expected all completed=true, got {todo}"

    # Filter completed=false
    status, body = run_curl("GET", "/todos?completed=false")
    assert status == 200, f"Expected 200, got {status}"
    for todo in body.get("todos", []):
        assert (
            todo.get("completed") is False
        ), f"Expected all completed=false, got {todo}"

    return True


def test_bulk_delete_completed():
    """Test bulk deleting completed todos."""
    # Ensure we have some completed
    _, created = run_curl("POST", "/todos", {"title": "Bulk delete test"})
    run_curl("PUT", f"/todos/{created['id']}", {"completed": True})

    # Bulk delete
    status, body = run_curl("DELETE", "/todos")
    assert status == 200, f"Expected 200, got {status}"
    assert "deleted_count" in body, f"Expected deleted_count, got {body}"

    # Verify none completed remain
    status, body = run_curl("GET", "/todos?completed=true")
    assert status == 200, f"Expected 200, got {status}"
    assert len(body.get("todos", [])) == 0, f"Expected no completed todos, got {body}"
    return True


def start_server(work_dir: Path) -> subprocess.Popen:
    """Start the API server."""
    # Remove old database
    db_path = work_dir / "todos.db"
    if db_path.exists():
        db_path.unlink()

    cmd = ["uv", "run", "uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]
    proc = subprocess.Popen(
        cmd,
        cwd=work_dir,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    # Wait for server to start
    for _ in range(30):
        try:
            status, _ = run_curl("GET", "/health")
            if status == 200:
                return proc
        except Exception:
            pass
        time.sleep(0.5)

    proc.kill()
    raise RuntimeError("Server failed to start")


def main():
    if len(sys.argv) < 2:
        print("Usage: verify.py <work_dir>")
        sys.exit(1)

    work_dir = Path(sys.argv[1])

    # Check main.py exists
    if not (work_dir / "main.py").exists():
        print("ERROR: main.py not found")
        sys.exit(1)

    print("Starting server...")
    server = start_server(work_dir)

    tests = [
        ("Health check", test_health),
        ("Create todo", test_create_todo),
        ("Create todo (no description)", test_create_todo_no_description),
        ("Create todo (missing title)", test_create_todo_missing_title),
        ("List todos", test_list_todos),
        ("Get single todo", test_get_todo),
        ("Get nonexistent todo", test_get_nonexistent_todo),
        ("Update todo", test_update_todo),
        ("Update sets timestamp", test_update_sets_timestamp),
        ("Delete todo", test_delete_todo),
        ("Delete nonexistent todo", test_delete_nonexistent),
        ("Filter by completed", test_filter_completed),
        ("Bulk delete completed", test_bulk_delete_completed),
    ]

    passed = 0
    failed = 0

    try:
        for name, test_fn in tests:
            try:
                test_fn()
                print(f"  [PASS] {name}")
                passed += 1
            except AssertionError as e:
                print(f"  [FAIL] {name}: {e}")
                failed += 1
            except Exception as e:
                print(f"  [ERROR] {name}: {e}")
                failed += 1
    finally:
        server.terminate()
        server.wait()

    print(f"\nResults: {passed}/{passed + failed} passed")
    sys.exit(0 if failed == 0 else 1)


if __name__ == "__main__":
    main()
