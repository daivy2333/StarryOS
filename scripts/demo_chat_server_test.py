#!/usr/bin/env python3
"""Host unit tests for the demo chat server decision core.

Loads `demo_chat_server.py` as a module (no network or terminal I/O) and
covers the pure decision logic: line classification (EMPTY/QUIT/TEXT), the
local-input action decision (SKIP/SEND/QUIT), the remote-line display
decision (empty lines dropped) and the remote-EOF result (peer closed).
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "scripts" / "demo_chat_server.py"

spec = importlib.util.spec_from_file_location("demo_chat_server", SERVER)
module = importlib.util.module_from_spec(spec)
assert spec is not None and spec.loader is not None
spec.loader.exec_module(module)
sys.modules["demo_chat_server"] = module


def test_line_classification() -> None:
    assert module.classify_line("") == module.LINE_EMPTY
    assert module.classify_line("   ") == module.LINE_EMPTY
    assert module.classify_line("/quit") == module.LINE_QUIT
    assert module.classify_line("   /quit   ") == module.LINE_QUIT
    assert module.classify_line("hello") == module.LINE_TEXT
    assert module.classify_line("hi there") == module.LINE_TEXT


def test_local_input_decisions() -> None:
    action, payload = module.handle_local_input("")
    assert action == module.ACTION_SKIP and payload is None
    action, payload = module.handle_local_input("/quit")
    assert action == module.ACTION_QUIT and payload is None
    action, payload = module.handle_local_input("ping")
    assert action == module.ACTION_SEND
    assert payload == "ping\n"
    action, payload = module.handle_local_input("head tail")
    assert action == module.ACTION_SEND
    assert payload == "head tail\n"


def test_remote_line_display() -> None:
    assert module.handle_remote_line("") is None
    assert module.handle_remote_line(" ") is None
    assert module.handle_remote_line("pong") == "pong"
    # A remote "/quit" arriving as a chat line is displayed like any text;
    # "/quit" is only a control command for the local side.
    assert module.handle_remote_line("/quit") == "/quit"


def test_remote_eof() -> None:
    assert module.handle_remote_eof() == module.RESULT_PEER_CLOSED


def main() -> int:
    tests = [
        test_line_classification,
        test_local_input_decisions,
        test_remote_line_display,
        test_remote_eof,
    ]
    failed = 0
    for test in tests:
        try:
            test()
        except AssertionError as error:
            failed += 1
            print(f"FAIL {test.__name__}: {error}")
        else:
            print(f"PASS {test.__name__}")
    if failed:
        print(f"demo_chat_server_test: {len(tests) - failed}/{len(tests)} PASS, "
              f"{failed} FAIL")
        return 1
    print("demo_chat_server_test: all PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())