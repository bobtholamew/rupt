"""
Coverage tracing using sys.monitoring (Python 3.12+).
Falls back to sys.settrace on older versions.
"""

import json
import os
import sys

_data = {}  # filename -> set of line numbers
_active = False
_tool_id = None


def start(source_dirs=None):
    """Start collecting coverage data."""
    global _data, _active, _tool_id
    _data = {}
    _active = True

    if hasattr(sys, "monitoring"):
        _start_monitoring(source_dirs)
    else:
        _start_settrace(source_dirs)


def stop():
    """Stop collecting and return the data."""
    global _active
    _active = False

    if hasattr(sys, "monitoring") and _tool_id is not None:
        _stop_monitoring()
    else:
        sys.settrace(None)

    return _data


def _start_monitoring(source_dirs):
    global _tool_id
    _tool_id = sys.monitoring.COVERAGE_ID
    sys.monitoring.use_tool_id(_tool_id, "rupt_coverage")
    sys.monitoring.set_events(_tool_id, sys.monitoring.events.LINE)

    def _line_handler(code, line_number):
        filename = code.co_filename
        if _should_track(filename, source_dirs):
            if filename not in _data:
                _data[filename] = set()
            _data[filename].add(line_number)

    sys.monitoring.register_callback(
        _tool_id, sys.monitoring.events.LINE, _line_handler
    )


def _stop_monitoring():
    global _tool_id
    if _tool_id is not None:
        sys.monitoring.set_events(_tool_id, 0)
        sys.monitoring.free_tool_id(_tool_id)
        _tool_id = None


def _start_settrace(source_dirs):
    def _trace(frame, event, arg):
        if event == "line":
            filename = frame.f_code.co_filename
            if _should_track(filename, source_dirs):
                if filename not in _data:
                    _data[filename] = set()
                _data[filename].add(frame.f_lineno)
        return _trace

    sys.settrace(_trace)


def _should_track(filename, source_dirs):
    if not filename or filename.startswith("<"):
        return False
    # Skip stdlib and site-packages
    if "site-packages" in filename or "lib/python" in filename.lower():
        return False
    if source_dirs:
        return any(filename.startswith(d) for d in source_dirs)
    return True


def serialize(data, rootdir=None):
    """Convert coverage data to a JSON-serializable dict."""
    result = {}
    for filename, lines in data.items():
        # Convert to relative path if rootdir given
        if rootdir and filename.startswith(rootdir):
            key = os.path.relpath(filename, rootdir).replace("\\", "/")
        else:
            key = filename.replace("\\", "/")
        result[key] = sorted(lines)
    return result


def to_lcov(data, rootdir=None):
    """Generate LCOV format coverage report."""
    lines = []
    for filename, covered_lines in sorted(data.items()):
        if rootdir and filename.startswith(rootdir):
            rel = os.path.relpath(filename, rootdir).replace("\\", "/")
        else:
            rel = filename.replace("\\", "/")

        lines.append(f"SF:{rel}")
        for lineno in sorted(covered_lines):
            lines.append(f"DA:{lineno},1")
        lines.append(f"LF:{len(covered_lines)}")
        lines.append(f"LH:{len(covered_lines)}")
        lines.append("end_of_record")
    return "\n".join(lines) + "\n"
