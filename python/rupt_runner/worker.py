"""
Test worker process. Reads a JSON manifest from stdin, executes tests,
streams JSON-line results to stdout.

Protocol:
  stdin  <- {"tests": [...], "rootdir": "/abs/path", "coverage_sources": [...]}
  stdout -> one JSON line per test result
  stdout -> {"type": "coverage", "data": {...}} if coverage enabled
  stdout -> {"type": "finished"}
"""

import importlib
import importlib.util
import inspect
import itertools
import json
import os
import sys
import time
import traceback

from rupt_runner.capture import CaptureFixture
from rupt_runner.fixture_engine import FixtureEngine


def main():
    raw = sys.stdin.read()
    manifest = json.loads(raw)
    tests = manifest["tests"]
    rootdir = manifest.get("rootdir", os.getcwd())
    cov_sources = manifest.get("coverage_sources", None)

    default_timeout = manifest.get("timeout", None)

    rootdir = os.path.abspath(rootdir)

    if rootdir not in sys.path:
        sys.path.insert(0, rootdir)

    from rupt_runner.assertion import install_hook
    install_hook()

    if cov_sources is not None:
        from rupt_runner import coverage_tracer
        source_dirs = [os.path.normpath(os.path.join(rootdir, s)) for s in cov_sources]
        coverage_tracer.start(source_dirs)

    engine = FixtureEngine(rootdir)
    real_stdout = sys.stdout

    for node_id in tests:
        result = run_one(node_id, rootdir, engine, default_timeout)
        real_stdout.write(json.dumps(result, default=str) + "\n")
        real_stdout.flush()

    if cov_sources is not None:
        from rupt_runner import coverage_tracer
        cov_data = coverage_tracer.stop()
        cov_json = coverage_tracer.serialize(cov_data, rootdir)
        real_stdout.write(json.dumps({"type": "coverage", "data": cov_json}) + "\n")
        real_stdout.flush()

    real_stdout.write(json.dumps({"type": "finished"}) + "\n")
    real_stdout.flush()


def run_one(node_id, rootdir, engine, default_timeout=None):
    result = {
        "node_id": node_id,
        "outcome": "error",
        "duration": 0.0,
        "stdout": "",
        "stderr": "",
        "longrepr": "",
        "message": "",
    }

    try:
        file_path, class_name, func_name, param_key = parse_node_id(node_id)
    except ValueError as e:
        result["longrepr"] = str(e)
        return result

    try:
        module = import_module(file_path, rootdir)
    except Exception:
        result["longrepr"] = traceback.format_exc()
        result["message"] = "import error"
        return result

    try:
        func, instance = resolve_callable(module, class_name, func_name)
    except Exception:
        result["longrepr"] = traceback.format_exc()
        result["message"] = "lookup error"
        return result

    # Check for skip/xfail markers (including from pytest.param marks)
    skip_reason = check_skip_markers(func, param_key)
    if skip_reason is not None:
        result["outcome"] = "skipped"
        result["message"] = skip_reason
        return result

    xfail = check_xfail_marker(func, param_key)

    # Resolve fixtures
    try:
        fixture_values = engine.resolve(func, module, file_path, rootdir)
    except Exception:
        result["longrepr"] = traceback.format_exc()
        result["message"] = "fixture error"
        return result

    # Resolve parametrize values
    if param_key is not None:
        try:
            param_values = resolve_parametrize(func, param_key)
            fixture_values.update(param_values)
        except Exception:
            result["longrepr"] = traceback.format_exc()
            result["message"] = "parametrize error"
            return result

    # Filter kwargs to only valid function parameters
    sig = inspect.signature(func)
    valid_params = set(sig.parameters.keys()) - {"self"}
    fixture_values = {k: v for k, v in fixture_values.items() if k in valid_params}

    # Determine timeout: marker overrides default
    timeout = _get_timeout(func, default_timeout)

    capsys_fixture = fixture_values.get("capsys")
    if capsys_fixture is not None:
        capture = None
    else:
        capture = CaptureFixture()
        capture.start()

    t0 = time.perf_counter()
    try:
        if timeout is not None:
            ret = _run_with_timeout(func, instance, fixture_values, timeout)
        elif instance is not None:
            ret = func(instance, **fixture_values)
        else:
            ret = func(**fixture_values)
        if inspect.iscoroutine(ret):
            import asyncio
            asyncio.run(ret)
        result["outcome"] = "passed"
    except TimeoutError:
        result["outcome"] = "failed"
        result["longrepr"] = f"Timeout: test exceeded {timeout}s"
        result["message"] = f"Timeout ({timeout}s)"
    except AssertionError as e:
        result["outcome"] = "failed"
        result["longrepr"] = traceback.format_exc()
        result["message"] = str(e)
    except SkipTest as e:
        result["outcome"] = "skipped"
        result["message"] = str(e)
    except KeyboardInterrupt:
        raise
    except Exception:
        result["outcome"] = "failed"
        result["longrepr"] = traceback.format_exc()
    finally:
        result["duration"] = time.perf_counter() - t0
        if capture is not None:
            out, err = capture.stop()
            result["stdout"] = out
            result["stderr"] = err
        elif capsys_fixture is not None:
            out, err = capsys_fixture.stop()
            result["stdout"] = out
            result["stderr"] = err

    if xfail:
        if result["outcome"] == "failed":
            result["outcome"] = "xfailed"
            result["message"] = result["message"] or "expected failure"
        elif result["outcome"] == "passed":
            if xfail == "strict":
                result["outcome"] = "failed"
                result["message"] = "[XPASS(strict)]"
            else:
                result["outcome"] = "xpassed"

    try:
        engine.teardown()
    except Exception:
        if result["outcome"] == "passed":
            result["outcome"] = "error"
            result["longrepr"] = traceback.format_exc()
            result["message"] = "fixture teardown error"

    return result


class SkipTest(Exception):
    pass


def _get_timeout(func, default_timeout):
    """Get timeout from @pytest.mark.timeout or default."""
    markers = _collect_markers(func)
    for marker in markers:
        name = getattr(marker, "name", None) or ""
        if name == "timeout":
            args = getattr(marker, "args", ())
            if args:
                return float(args[0])
            kwargs = getattr(marker, "kwargs", {})
            if "timeout" in kwargs:
                return float(kwargs["timeout"])
    return default_timeout


def _run_with_timeout(func, instance, fixture_values, timeout):
    """Run a function with a timeout using a thread."""
    import threading

    result_holder = [None]
    exc_holder = [None]

    def target():
        try:
            if instance is not None:
                result_holder[0] = func(instance, **fixture_values)
            else:
                result_holder[0] = func(**fixture_values)
        except Exception as e:
            exc_holder[0] = e

    thread = threading.Thread(target=target, daemon=True)
    thread.start()
    thread.join(timeout=timeout)

    if thread.is_alive():
        raise TimeoutError(f"test exceeded {timeout}s timeout")

    if exc_holder[0] is not None:
        raise exc_holder[0]

    return result_holder[0]


def parse_node_id(node_id):
    param_key = None
    base = node_id
    if "[" in node_id:
        idx = node_id.index("[")
        base = node_id[:idx]
        param_key = node_id[idx + 1 : -1]

    parts = base.split("::")
    if len(parts) == 2:
        return parts[0], None, parts[1], param_key
    elif len(parts) == 3:
        return parts[0], parts[1], parts[2], param_key
    else:
        raise ValueError(f"cannot parse node_id: {node_id}")


def import_module(file_path, rootdir):
    file_path = file_path.replace("\\", "/")
    rel = file_path
    if rel.endswith(".py"):
        rel = rel[:-3]
    mod_name = rel.replace("/", ".")

    if mod_name in sys.modules:
        return sys.modules[mod_name]

    # Ensure parent directories are on sys.path for subdirectory modules
    abs_path = os.path.normpath(os.path.join(rootdir, file_path))
    parent = os.path.dirname(abs_path)
    if parent not in sys.path:
        sys.path.insert(0, parent)

    # Use importlib.import_module so sys.meta_path hooks (assertion rewriting) fire
    try:
        return importlib.import_module(mod_name)
    except ImportError:
        # Fallback: direct spec load for modules not on the normal import path
        spec = importlib.util.spec_from_file_location(mod_name, abs_path)
        if spec is None or spec.loader is None:
            raise ImportError(f"cannot find module for {file_path}")
        module = importlib.util.module_from_spec(spec)
        sys.modules[mod_name] = module
        spec.loader.exec_module(module)
        return module


def resolve_callable(module, class_name, func_name):
    if class_name:
        cls = getattr(module, class_name)
        instance = cls()
        func = getattr(cls, func_name)
        return func, instance
    else:
        func = getattr(module, func_name)
        return func, None


# ---------------------------------------------------------------------------
# Parametrize resolution
# ---------------------------------------------------------------------------

def resolve_parametrize(func, param_key):
    """Resolve parametrize values from the decorator + param_key string.

    Builds an index of all param combinations → ID string, then looks up
    the param_key to find the matching combination.
    """
    markers = _collect_markers(func)
    parametrize_markers = [m for m in markers if getattr(m, "name", "") == "parametrize"]

    if not parametrize_markers:
        return {}

    # Build the full cross-product of all parametrize decorators,
    # matching the same order as Rust collection: top decorator first,
    # bottom decorator varies fastest.
    layers = []
    for marker in parametrize_markers:
        args = getattr(marker, "args", ())
        if len(args) < 2:
            continue
        arg_names = _parse_argnames(args[0])
        argvalues = list(args[1])
        # Unwrap pytest.param objects
        cases = []
        for v in argvalues:
            val, marks, pid = _unwrap_pytest_param(v)
            if not isinstance(val, (tuple, list)):
                val = (val,)
            cases.append((arg_names, val, pid))
        layers.append(cases)

    if not layers:
        return {}

    # Generate the cross product, top-first (layers[0] varies slowest)
    for combo in itertools.product(*layers):
        # combo is a tuple of (arg_names, values, explicit_id) per layer
        id_parts = []
        result = {}
        for arg_names, values, pid in combo:
            if pid is not None:
                id_parts.append(pid)
            else:
                id_parts.append("-".join(_val_repr(v) for v in values))
            for name, val in zip(arg_names, values):
                result[name] = val

        combined_id = "-".join(id_parts)
        # Deduplicate: if needed, append 0, 1, ... but for matching we try
        # the raw ID first since deduplication only matters for collisions
        if combined_id == param_key:
            return result

    # Fallback: try matching with deduplication suffixes stripped
    # Also try matching by index if the param_key ends with a digit
    for idx, combo in enumerate(itertools.product(*layers)):
        id_parts = []
        result = {}
        for arg_names, values, pid in combo:
            if pid is not None:
                id_parts.append(pid)
            else:
                id_parts.append("-".join(_val_repr(v) for v in values))
            for name, val in zip(arg_names, values):
                result[name] = val

        combined_id = "-".join(id_parts)
        # Check with dedup suffix: param_key might be "foo0" matching "foo"
        if param_key.rstrip("0123456789") == combined_id:
            return result

    return {}


def _unwrap_pytest_param(value):
    """Unwrap a pytest.param() object into (values, marks, id)."""
    # Check if it's a pytest.param object
    typ = type(value).__name__
    if typ == "ParameterSet":
        values = value.values
        marks = value.marks
        pid = value.id
        if len(values) == 1:
            return values[0], marks, pid
        return values, marks, pid
    return value, [], None


def _parse_argnames(arg):
    if isinstance(arg, str):
        return [s.strip() for s in arg.split(",")]
    return list(arg)


def _val_repr(v):
    """Match rupt's static ID generation."""
    if v is None:
        return "None"
    if isinstance(v, bool):
        return "True" if v else "False"
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, str):
        return v
    if isinstance(v, bytes):
        try:
            return v.decode("utf-8")
        except UnicodeDecodeError:
            return repr(v)
    return repr(v)


# ---------------------------------------------------------------------------
# Marker handling
# ---------------------------------------------------------------------------

def check_skip_markers(func, param_key=None):
    markers = _collect_markers(func)

    # Also check for marks on the specific pytest.param
    if param_key is not None:
        param_marks = _get_param_marks(func, param_key)
        markers = markers + param_marks

    for marker in markers:
        name = getattr(marker, "name", None) or ""
        if name == "skip":
            args = getattr(marker, "args", ())
            kwargs = getattr(marker, "kwargs", {})
            reason = kwargs.get("reason", args[0] if args else "unconditional skip")
            return str(reason)
        if name == "skipif":
            args = getattr(marker, "args", ())
            kwargs = getattr(marker, "kwargs", {})
            if args and args[0]:
                reason = kwargs.get("reason", "condition is true")
                return str(reason)
    return None


def check_xfail_marker(func, param_key=None):
    markers = _collect_markers(func)

    if param_key is not None:
        param_marks = _get_param_marks(func, param_key)
        markers = markers + param_marks

    for marker in markers:
        name = getattr(marker, "name", None) or ""
        if name == "xfail":
            kwargs = getattr(marker, "kwargs", {})
            args = getattr(marker, "args", ())
            # xfail(condition) — check the condition
            if args and not args[0]:
                continue  # condition is False, xfail doesn't apply
            strict = kwargs.get("strict", False)
            return "strict" if strict else True
    return None


def _get_param_marks(func, param_key):
    """Get marks from the specific pytest.param that matches param_key."""
    markers = _collect_markers(func)
    parametrize_markers = [m for m in markers if getattr(m, "name", "") == "parametrize"]

    for marker in parametrize_markers:
        args = getattr(marker, "args", ())
        if len(args) < 2:
            continue
        arg_names = _parse_argnames(args[0])
        for v in args[1]:
            val, marks, pid = _unwrap_pytest_param(v)
            if not isinstance(val, (tuple, list)):
                val = (val,)
            case_id = pid if pid else "-".join(_val_repr(x) for x in val)
            if case_id == param_key or param_key.startswith(case_id):
                return list(marks)
    return []


def _collect_markers(func):
    markers = []
    pytestmark = getattr(func, "pytestmark", [])
    if isinstance(pytestmark, list):
        markers.extend(pytestmark)
    elif pytestmark:
        markers.append(pytestmark)
    return markers


if __name__ == "__main__":
    main()
