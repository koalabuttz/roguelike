### Python Best Practices

#### Code Style
- Use type hints for function signatures
- Use `pathlib.Path` over `os.path` for new code
- Use context managers (`with`) for file/resource handling
- Prefer list comprehensions over `map()`/`filter()` for readability
- Run `flake8` or `ruff` if available

#### Error Handling
```python
# GOOD: Specific exception handling with context
try:
    data = json.loads(content)
except json.JSONDecodeError as e:
    logger.error(f"Invalid JSON in {filepath}: {e}")
    raise

# BAD: Bare except or catching too broadly
try:
    data = json.loads(content)
except:  # Don't do this
    pass
```

#### Security
- Never use `eval()` or `exec()` with user input
- Use `subprocess.run()` with list args, not shell=True with user input
- Use `logging` module, not print for debugging in production
- Validate file paths to prevent path traversal

#### Testing
- Write tests with `pytest` when available
- Use `tempfile` for filesystem tests
- Mock external dependencies

#### SQL Injection Prevention
Always use parameterized queries:
```python
# GOOD
cursor.execute("SELECT * FROM users WHERE name = ?", (name,))

# BAD - SQL injection vulnerability
cursor.execute(f"SELECT * FROM users WHERE name = '{name}'")
```
