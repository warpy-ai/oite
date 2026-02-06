---
sidebar_position: 10
title: Testing
description: How to write and run tests in Oite projects using unroll test.
keywords: [testing, tests, test runner, coverage, filtering]
---

# Testing

Unroll discovers and runs test files automatically.

## Running Tests

```bash
unroll test
```

## Test Discovery

Unroll finds test files in two locations:

### Integration Tests

All `.ot` files in the `./tests/` directory:

```
tests/
├── http_client_test.ot
├── json_parser_test.ot
└── auth_test.ot
```

### Unit Tests (Co-located)

Files matching `*_test.ot` or `*.test.ot` in `./src/`:

```
src/
├── server.ot
├── server_test.ot          # Discovered
├── utils/
│   ├── parser.ot
│   └── parser.test.ot      # Discovered
└── main.ot
```

## Filtering Tests

Run a subset of tests using glob patterns:

```bash
# Run tests matching "http*"
unroll test --filter "http*"

# Run tests matching "*_key"
unroll test --filter "*_key"

# Run exact test name
unroll test --filter auth
```

### Filter Patterns

| Pattern | Matches |
|---------|---------|
| `http*` | `http_client`, `http_server`, `http_utils` |
| `*_key` | `api_key`, `session_key` |
| `auth` | `auth` (exact match only) |
| `*parser*` | `json_parser`, `xml_parser_v2` |

The filter matches against the test file name (without extension).

## Options

```bash
unroll test [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--filter <PATTERN>` | Glob pattern to filter tests | All tests |
| `--verbose`, `-v` | Show individual test results | Summary only |
| `--nocapture` | Show stdout/stderr from tests | Captured |
| `--coverage` | Generate coverage report | No coverage |
| `--jobs <N>`, `-j <N>` | Parallel test execution | Sequential |

## Verbose Output

```bash
unroll test --verbose
```

Shows each test result:

```
Running 3 tests...

  PASS  http_client
  PASS  json_parser
  FAIL  auth (exit code 1)

Results: 2 passed, 1 failed, 3 total
```

## Seeing Test Output

By default, stdout and stderr from tests are captured. To see output:

```bash
unroll test --nocapture
```

## Parallel Execution

Run tests in parallel:

```bash
unroll test --jobs 4
```

Runs up to 4 tests concurrently.

## Project Structure for Tests

```
my-app/
├── unroll.toml
├── src/
│   ├── main.ot
│   ├── server.ot
│   └── server_test.ot        # Unit test
├── tests/
│   ├── integration_test.ot   # Integration test
│   └── e2e_test.ot           # End-to-end test
└── examples/
    └── basic.ot
```

## Test Dependencies

Add test-only dependencies as dev dependencies:

```bash
unroll add --dev @rolls/test
```

```toml
[dev-dependencies]
"@rolls/test" = "0.1"
```

Dev dependencies are available in test files but not included in published packages.

## CI Integration

```yaml
# GitHub Actions
- name: Run tests
  run: unroll test

# With verbose output
- name: Run tests (verbose)
  run: unroll test --verbose

# Check formatting too
- name: Check format
  run: unroll fmt --check
```
