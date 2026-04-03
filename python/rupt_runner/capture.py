"""Capture stdout/stderr during test execution."""

import io
import sys


class CaptureFixture:
    def __init__(self):
        self._old_stdout = None
        self._old_stderr = None
        self._stdout_buf = None
        self._stderr_buf = None

    def start(self):
        self._stdout_buf = io.StringIO()
        self._stderr_buf = io.StringIO()
        self._old_stdout = sys.stdout
        self._old_stderr = sys.stderr
        sys.stdout = self._stdout_buf
        sys.stderr = self._stderr_buf

    def stop(self):
        out = ""
        err = ""
        if self._stdout_buf is not None:
            out = self._stdout_buf.getvalue()
        if self._stderr_buf is not None:
            err = self._stderr_buf.getvalue()
        if self._old_stdout is not None:
            sys.stdout = self._old_stdout
        if self._old_stderr is not None:
            sys.stderr = self._old_stderr
        self._old_stdout = None
        self._old_stderr = None
        return out, err

    def readouterr(self):
        """Read captured output so far without stopping capture."""
        out = self._stdout_buf.getvalue() if self._stdout_buf else ""
        err = self._stderr_buf.getvalue() if self._stderr_buf else ""
        # Reset buffers
        if self._stdout_buf:
            self._stdout_buf.truncate(0)
            self._stdout_buf.seek(0)
        if self._stderr_buf:
            self._stderr_buf.truncate(0)
            self._stderr_buf.seek(0)
        return out, err
