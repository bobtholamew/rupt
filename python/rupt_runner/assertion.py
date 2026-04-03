"""
Assertion rewriting for better failure messages.

Installs an import hook that rewrites `assert` statements to capture
intermediate values, similar to pytest's assertion rewriting.
"""

import ast
import importlib
import importlib.abc
import importlib.machinery
import importlib.util
import os
import sys
import textwrap
import types


_hook_installed = False


def install_hook():
    """Install the assertion rewriting import hook."""
    global _hook_installed
    if _hook_installed:
        return
    hook = AssertRewriteHook()
    sys.meta_path.insert(0, hook)
    _hook_installed = True


class AssertRewriteHook(importlib.abc.MetaPathFinder):
    """Import hook that rewrites assert statements in test modules."""

    def __init__(self):
        self._rewriting = set()

    def find_spec(self, fullname, path, target=None):
        if fullname in self._rewriting:
            return None

        parts = fullname.split(".")
        basename = parts[-1]
        if not (basename.startswith("test_") or basename.endswith("_test")):
            return None

        # Find the original spec
        self._rewriting.add(fullname)
        try:
            spec = importlib.util.find_spec(fullname)
        finally:
            self._rewriting.discard(fullname)

        if spec is None or spec.origin is None:
            return None

        if not spec.origin.endswith(".py"):
            return None

        return importlib.machinery.ModuleSpec(
            fullname,
            AssertRewriteLoader(spec.origin),
            origin=spec.origin,
        )


class AssertRewriteLoader(importlib.abc.Loader):
    def __init__(self, path):
        self.path = path

    def create_module(self, spec):
        return None

    def exec_module(self, module):
        try:
            source = open(self.path, "r", encoding="utf-8").read()
        except (OSError, UnicodeDecodeError):
            # Fall back to normal exec
            code = compile(open(self.path, "rb").read(), self.path, "exec")
            exec(code, module.__dict__)
            return

        try:
            tree = ast.parse(source, filename=self.path)
            rewriter = AssertRewriter()
            tree = rewriter.visit(tree)
            ast.fix_missing_locations(tree)
            code = compile(tree, self.path, "exec")
        except SyntaxError:
            code = compile(source, self.path, "exec")

        # Inject our helper into the module namespace
        module.__dict__["__rupt_assert_repr"] = _assert_repr
        module.__dict__["__rupt_format_explanation"] = _format_explanation
        exec(code, module.__dict__)


class AssertRewriter(ast.NodeTransformer):
    """Rewrites assert statements to capture intermediate values."""

    def visit_Assert(self, node):
        self.generic_visit(node)

        # Simple comparison: assert a == b → capture both sides
        if isinstance(node.test, ast.Compare) and len(node.test.ops) == 1:
            left = node.test.left
            right = node.test.comparators[0]
            op = node.test.ops[0]

            # Build: if not (left op right): raise AssertionError(msg)
            left_repr = ast.Call(
                func=ast.Name(id="__rupt_assert_repr", ctx=ast.Load()),
                args=[left],
                keywords=[],
            )
            right_repr = ast.Call(
                func=ast.Name(id="__rupt_assert_repr", ctx=ast.Load()),
                args=[right],
                keywords=[],
            )

            op_str = _op_to_str(op)
            msg = ast.Call(
                func=ast.Name(id="__rupt_format_explanation", ctx=ast.Load()),
                args=[left_repr, right_repr, ast.Constant(value=op_str), left, right],
                keywords=[],
            )

            # Build: if not <test>: raise AssertionError(<msg>)
            raise_stmt = ast.Raise(
                exc=ast.Call(
                    func=ast.Name(id="AssertionError", ctx=ast.Load()),
                    args=[msg],
                    keywords=[],
                ),
                cause=None,
            )

            if_stmt = ast.If(
                test=ast.UnaryOp(op=ast.Not(), operand=node.test),
                body=[raise_stmt],
                orelse=[],
            )

            return if_stmt

        # For assert with a message: keep the message
        if node.msg is not None:
            return node

        # For boolean expressions: assert x → capture x's value
        val_repr = ast.Call(
            func=ast.Name(id="__rupt_assert_repr", ctx=ast.Load()),
            args=[node.test],
            keywords=[],
        )

        raise_stmt = ast.Raise(
            exc=ast.Call(
                func=ast.Name(id="AssertionError", ctx=ast.Load()),
                args=[
                    ast.JoinedStr(
                        values=[
                            ast.Constant(value="assert "),
                            ast.FormattedValue(
                                value=val_repr,
                                conversion=-1,
                                format_spec=None,
                            ),
                        ]
                    )
                ],
                keywords=[],
            ),
            cause=None,
        )

        if_stmt = ast.If(
            test=ast.UnaryOp(op=ast.Not(), operand=node.test),
            body=[raise_stmt],
            orelse=[],
        )

        return if_stmt


def _op_to_str(op):
    ops = {
        ast.Eq: "==",
        ast.NotEq: "!=",
        ast.Lt: "<",
        ast.LtE: "<=",
        ast.Gt: ">",
        ast.GtE: ">=",
        ast.Is: "is",
        ast.IsNot: "is not",
        ast.In: "in",
        ast.NotIn: "not in",
    }
    return ops.get(type(op), "??")


def _assert_repr(value):
    """Produce a short repr of a value for assertion messages."""
    r = repr(value)
    if len(r) > 200:
        r = r[:197] + "..."
    return r


def _format_explanation(left_repr, right_repr, op_str, left_val, right_val):
    """Format an assertion failure explanation with both sides."""
    lines = [f"assert {left_repr} {op_str} {right_repr}"]

    if op_str == "==" and left_val != right_val:
        # Show diff for strings
        if isinstance(left_val, str) and isinstance(right_val, str):
            if len(left_val) > 40 or len(right_val) > 40:
                lines.append(f"  Left:  {left_repr}")
                lines.append(f"  Right: {right_repr}")
                # Find first difference
                for i, (a, b) in enumerate(zip(left_val, right_val)):
                    if a != b:
                        lines.append(f"  First diff at index {i}: '{a}' != '{b}'")
                        break
                else:
                    if len(left_val) != len(right_val):
                        lines.append(
                            f"  Left has {len(left_val)} chars, right has {len(right_val)} chars"
                        )
            else:
                lines.append(f"  where {left_repr} != {right_repr}")
        # Show diff for sequences
        elif isinstance(left_val, (list, tuple)) and isinstance(right_val, (list, tuple)):
            if len(left_val) != len(right_val):
                lines.append(
                    f"  Left has {len(left_val)} items, right has {len(right_val)} items"
                )
            else:
                for i, (a, b) in enumerate(zip(left_val, right_val)):
                    if a != b:
                        lines.append(f"  At index {i}: {repr(a)} != {repr(b)}")
                        break
        else:
            lines.append(f"  where {left_repr} != {right_repr}")

    return "\n".join(lines)
