# rupt

An extremely fast Python test runner, written in Rust.

rupt collects tests by parsing Python source files directly. No imports, no interpreter, no dependencies needed. On CPython's own test suite (1,700+ files), collection takes 153ms vs pytest's 14 seconds. Then it runs them.

```bash
rupt
rupt tests/test_api.py -k "not slow" -x
rupt -n auto --cov=src
```

## Installation

```bash
# from source (requires Rust 1.75+)
cargo install --path crates/rupt_cli
```

Python 3.12+ required for execution. Collection works without Python at all.

## Why

pytest imports every test module through the interpreter to discover tests. For large projects that's hundreds of Python imports before anything runs. rupt parses the AST instead, using `ruff_python_parser` in parallel. Execution still happens in Python via worker subprocesses; Rust handles the orchestration.

## Benchmarks

Collection time. Windows 11, Rust 1.94.1, Python 3.14.3, pytest 9.0.2. Best of 5 runs (rupt) / best of 3 (pytest), wall-clock including process startup.

| Project | pytest | rupt | |
|---------|--------|------|-|
| requests | 640ms | 24ms | 26x |
| flask | 823ms | 24ms | 34x |
| fastapi | 2.7s | 85ms | 32x |
| django | 6.1s | 358ms | 17x |
| cpython | 14.2s | 153ms | 92x |

Worth noting: pytest can't even collect Django or FastAPI without installing all their dependencies first. rupt parses the source directly, so it works on a fresh clone.

Execution (18-test fixture project):

| | time |
|-|------|
| rupt | 197ms |
| rupt -n 4 | 207ms |
| rupt --cov=. | 263ms |
| pytest | 512ms |

Coverage uses `sys.monitoring` (Python 3.12+). Overhead is around 33%, vs coverage.py's typical 50-80%.

## Collection accuracy

Every statically-defined test function is found. Tested against real projects:

| Project | pytest | rupt | match |
|---------|--------|------|-------|
| requests | 339 | 339 | 100% |
| flask | 371 | 374 | 100% |
| fastapi | 771 | 2,163 | 99.5% |

fastapi: rupt actually finds *more* tests. pytest fails on 451 import errors from missing dependencies; rupt doesn't care because it doesn't import anything.

## Usage

```bash
rupt                              # run everything
rupt tests/test_auth.py           # one file
rupt tests/test_auth.py::test_login  # one test
rupt -k "auth and not oauth"      # keyword filter
rupt -m "not slow"                # marker filter
rupt -x                           # stop on first failure
rupt --maxfail=5                  # stop after 5
rupt -n auto                      # parallel (one worker per core)
rupt -n 4                         # 4 workers
rupt --cov=src                    # coverage
rupt --cov-report=lcov            # lcov output for CI
rupt --cov-fail-under=80          # fail below threshold
rupt --watch                      # re-run on file changes
rupt --collect-only               # just list tests, don't run
rupt --timeout=30                 # kill tests after 30s
rupt --durations=10               # show 10 slowest
rupt -v                           # verbose
rupt --tb=short                   # shorter tracebacks
rupt --junit-xml=results.xml      # CI output
```

## What works

rupt reads `pyproject.toml`, `pytest.ini`, and `setup.cfg` for pytest configuration. It supports:

- `test_*` / `Test*` discovery with configurable patterns
- `conftest.py` fixture hierarchy (scoping, autouse, params, yield)
- `@pytest.mark.parametrize` including tuple unpacking, ids, stacking, `pytest.param`
- `skip` / `skipif` / `xfail` markers
- Assertion rewriting with value diffs
- `-k` / `-m` expression filtering
- Parallel execution (built-in, not a plugin)
- Coverage via `sys.monitoring` (built-in, not a plugin)
- Watch mode (built-in, not a plugin)
- `tmp_path`, `capsys`, `monkeypatch`, `request` fixtures
- Async test functions
- JUnit XML output

## What doesn't work (yet)

- **pytest plugin API** - rupt doesn't load pytest plugins. The most popular plugin features (xdist, cov, timeout, randomly) are built in instead.
- **Doctest collection**
- **`unittest.TestCase` discovery**
- **Dynamic parametrize** - if your parametrize decorator calls a function at import time to generate the parameter list, rupt won't see it during static collection. The tests will still run, but `--collect-only` may not list them.
- **`conftest.py` hooks** - `pytest_configure`, `pytest_collection_modifyitems`, etc. are not called.

## How it works

Collection is pure Rust: walk the filesystem, parse each `.py` file into an AST with `ruff_python_parser`, extract test functions and classes by naming convention, read markers and parametrize decorators from the AST, expand parametrize IDs, apply filters.

Execution uses a pool of Python worker subprocesses. Each worker receives a batch of test node IDs on stdin as JSON, imports the modules, resolves fixtures, runs the tests, and streams results back on stdout as JSON lines. Rust aggregates everything and formats the output.

Parallelism partitions tests across workers by file (default), by class, or by individual test. Collection happens once in Rust; unlike pytest-xdist, workers don't re-collect.

## Project layout

```
crates/
  rupt_core/       collection, execution, coverage, reporting (14 modules)
  rupt_cli/        CLI entry point
python/
  rupt_runner/     worker, fixture engine, assertion rewriting, coverage tracer
```

Rust dependencies: ruff_python_parser, walkdir, rayon, clap, serde, globset, notify, dunce. Python side uses stdlib only.

## License

MIT