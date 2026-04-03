"""
Fixture resolution and execution engine.

Tier 1: function-scoped fixtures from conftest.py and test modules,
yield fixtures, dependency chains, tmp_path, capsys/capfd, request.
"""

import inspect
import os
import sys
import tempfile
import shutil
from pathlib import Path


class FixtureRequest:
    """Minimal pytest-compatible request object."""

    def __init__(self, node_id, param=None, config=None):
        self.node_id = node_id
        self.param = param
        self.config = config or {}
        self.fixturenames = []
        self._finalizers = []

    def addfinalizer(self, func):
        self._finalizers.append(func)

    def run_finalizers(self):
        while self._finalizers:
            func = self._finalizers.pop()
            func()

    @property
    def function(self):
        return None

    @property
    def cls(self):
        return None


class FixtureEngine:
    def __init__(self, rootdir):
        self.rootdir = rootdir
        self._conftest_cache = {}  # dir_path -> module
        self._fixture_registry = {}  # name -> (func, scope, module)
        self._active_generators = []  # for yield fixture teardown
        self._tmp_dirs = []
        self._request = None
        self._discover_conftests(rootdir)

    def _discover_conftests(self, rootdir):
        """Walk rootdir and import all conftest.py files."""
        for dirpath, dirnames, filenames in os.walk(rootdir):
            # Skip hidden/venv dirs
            dirnames[:] = [
                d for d in dirnames
                if not d.startswith(".")
                and d not in ("__pycache__", "node_modules", "venv", ".venv")
            ]
            if "conftest.py" in filenames:
                self._load_conftest(dirpath)

    def _load_conftest(self, dirpath):
        conftest_path = os.path.join(dirpath, "conftest.py")
        if conftest_path in self._conftest_cache:
            return self._conftest_cache[conftest_path]

        import importlib.util

        mod_name = f"conftest_{dirpath.replace(os.sep, '_').replace(':', '_')}"
        spec = importlib.util.spec_from_file_location(mod_name, conftest_path)
        if spec is None or spec.loader is None:
            return None

        module = importlib.util.module_from_spec(spec)
        sys.modules[mod_name] = module
        try:
            spec.loader.exec_module(module)
        except Exception:
            return None

        self._conftest_cache[conftest_path] = module
        self._register_fixtures_from(module, dirpath)
        return module

    def _register_fixtures_from(self, module, scope_dir=None):
        """Scan a module for @pytest.fixture decorated functions."""
        for name in dir(module):
            try:
                obj = getattr(module, name, None)
            except Exception:
                continue
            if obj is None or not callable(obj):
                continue

            try:
                marker = (
                    getattr(obj, "_pytestfixturefunction", None)
                    or getattr(obj, "_fixture_function_marker", None)
                )
            except Exception:
                continue
            if marker is not None:
                scope = getattr(marker, "scope", "function")
                params = getattr(marker, "params", None)
                autouse = getattr(marker, "autouse", False)
                func = getattr(obj, "_fixture_function", None) or obj
                self._fixture_registry[name] = (func, scope, scope_dir, params, autouse)

    def resolve(self, func, module, file_path, rootdir):
        """Resolve fixture values for a test function. Returns dict of name->value."""
        # Register fixtures from the test module itself
        self._register_fixtures_from(module, os.path.dirname(os.path.join(rootdir, file_path)))

        sig = inspect.signature(func)
        params = list(sig.parameters.keys())

        # Remove 'self' for methods
        if params and params[0] == "self":
            params = params[1:]

        self._request = FixtureRequest(node_id=file_path)
        kwargs = {}
        self._active_generators = []

        for param_name in params:
            kwargs[param_name] = self._resolve_one(param_name, rootdir, kwargs)

        return kwargs

    def _resolve_one(self, name, rootdir, existing):
        """Resolve a single fixture by name."""
        # Built-in fixtures
        if name == "tmp_path":
            return self._make_tmp_path()
        if name == "tmp_path_factory":
            return TmpPathFactory(self)
        if name == "capsys":
            from rupt_runner.capture import CaptureFixture
            cap = CaptureFixture()
            cap.start()
            self._active_generators.append(("capsys", cap))
            return cap
        if name == "request":
            return self._request
        if name == "monkeypatch":
            mp = MonkeyPatch()
            self._active_generators.append(("monkeypatch", mp))
            return mp

        # Look up in registry
        if name in self._fixture_registry:
            entry = self._fixture_registry[name]
            fixture_func = entry[0]
            params = entry[3] if len(entry) > 3 else None
            if params is not None and self._request is not None:
                # Parametrized fixture — use request.param
                self._request.param = params[0]  # default to first
            return self._call_fixture(fixture_func, rootdir, existing)

        return None

    def _call_fixture(self, fixture_func, rootdir, existing):
        """Call a fixture function, resolving its own dependencies."""
        sig = inspect.signature(fixture_func)
        params = list(sig.parameters.keys())

        kwargs = {}
        for p in params:
            if p in existing:
                kwargs[p] = existing[p]
            else:
                kwargs[p] = self._resolve_one(p, rootdir, {**existing, **kwargs})

        result = fixture_func(**kwargs)

        # Handle generator (yield) fixtures
        if inspect.isgenerator(result):
            value = next(result)
            self._active_generators.append(("yield", result))
            return value

        return result

    def _make_tmp_path(self):
        d = tempfile.mkdtemp(prefix="rupt_")
        self._tmp_dirs.append(d)
        return Path(d)

    def teardown(self):
        """Run teardown for yield fixtures, then cleanup tmp dirs."""
        errors = []

        # Teardown in reverse order
        for kind, obj in reversed(self._active_generators):
            try:
                if kind == "yield":
                    try:
                        next(obj)
                    except StopIteration:
                        pass
                elif kind == "capsys":
                    obj.stop()
                elif kind == "monkeypatch":
                    obj.undo()
            except Exception as e:
                errors.append(e)

        self._active_generators.clear()

        if self._request:
            self._request.run_finalizers()
            self._request = None

        for d in self._tmp_dirs:
            shutil.rmtree(d, ignore_errors=True)
        self._tmp_dirs.clear()

        if errors:
            raise errors[0]


class TmpPathFactory:
    def __init__(self, engine):
        self._engine = engine

    def mktemp(self, basename, numbered=True):
        d = tempfile.mkdtemp(prefix=f"rupt_{basename}_")
        self._engine._tmp_dirs.append(d)
        return Path(d)


_NOTSET = object()


class MonkeyPatch:
    """Minimal monkeypatch implementation."""

    def __init__(self):
        self._patches = []
        self._env_changes = []

    def setattr(self, target, name_or_value, value=_NOTSET):
        if value is _NOTSET:
            # Two-arg form: target is "mod.Class.attr", name_or_value is the value
            parts = target.rsplit(".", 1)
            if len(parts) == 2:
                mod_path, attr = parts
                import importlib
                mod = importlib.import_module(mod_path)
                old = getattr(mod, attr, _NOTSET)
                setattr(mod, attr, name_or_value)
                self._patches.append((mod, attr, old))
        else:
            old = getattr(target, name_or_value, _NOTSET)
            setattr(target, name_or_value, value)
            self._patches.append((target, name_or_value, old))

    def delattr(self, target, name):
        old = getattr(target, name)
        delattr(target, name)
        self._patches.append((target, name, old))

    def setenv(self, name, value, prepend=None):
        old = os.environ.get(name)
        self._env_changes.append((name, old))
        if prepend and old is not None:
            value = value + prepend + old
        os.environ[name] = value

    def delenv(self, name, raising=True):
        old = os.environ.get(name)
        self._env_changes.append((name, old))
        if name in os.environ:
            del os.environ[name]
        elif raising:
            raise KeyError(name)

    def chdir(self, path):
        old = os.getcwd()
        os.chdir(path)
        self._patches.append(("__chdir__", old, None))

    def undo(self):
        for item in reversed(self._patches):
            if item[0] == "__chdir__":
                os.chdir(item[1])
            else:
                target, name, old = item
                if old is _NOTSET:
                    try:
                        delattr(target, name)
                    except AttributeError:
                        pass
                else:
                    setattr(target, name, old)
        self._patches.clear()

        for name, old in reversed(self._env_changes):
            if old is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = old
        self._env_changes.clear()
