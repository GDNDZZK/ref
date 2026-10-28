#!/usr/bin/env python3
"""Fake stdio MCP server used by tests. Behaviour knobs via env:
FAKE_LOG: file to append received methods/notifications to.
FAKE_INIT_FAIL: exit immediately with a message on stderr."""
import json
import os
import sys
import threading
import time

LOG = os.environ.get("FAKE_LOG")
lock = threading.Lock()
pending = {}
extra_tool = False


def log(line):
    if LOG:
        with open(LOG, "a") as f:
            f.write(line + "\n")


def send(msg):
    with lock:
        sys.stdout.write(json.dumps(msg) + "\n")
        sys.stdout.flush()


def reply(id, result):
    send({"jsonrpc": "2.0", "id": id, "result": result})


def text(s, is_error=False):
    return {"content": [{"type": "text", "text": s}], "isError": is_error}


def tools_page(cursor):
    page1 = [
        {"name": "echo", "description": "Echo text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}},
        {"name": "img", "title": "Image tool"},
        {"name": "fail"},
    ]
    page2 = [{"name": "crash"}, {"name": "slow"}, {"name": "ping_me"}, {"name": "change"}]
    if extra_tool:
        page2.append({"name": "added", "description": "Added later"})
    if cursor is None:
        return {"tools": page1, "nextCursor": "p2"}
    return {"tools": page2}


def slow(id, secs):
    time.sleep(secs)
    reply(id, text("slow done"))


def ping_me(id):
    pid = "srv-1"
    ev = threading.Event()
    pending[pid] = ev
    send({"jsonrpc": "2.0", "id": pid, "method": "ping"})
    ok = ev.wait(5)
    unk = "srv-2"
    ev2 = threading.Event()
    pending[unk] = ev2
    send({"jsonrpc": "2.0", "id": unk, "method": "sampling/createMessage", "params": {}})
    ev2.wait(5)
    reply(id, text("pong %s; sampling %s" % ("ok" if ok else "missing", pending.pop("srv-2-result", None))))


def call(id, params):
    global extra_tool
    name = params.get("name")
    args = params.get("arguments") or {}
    if name == "echo":
        reply(id, text("echo: " + str(args.get("text"))))
    elif name == "img":
        reply(id, {"content": [{"type": "text", "text": "here"}, {"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]})
    elif name == "fail":
        reply(id, text("bad thing", True))
    elif name == "crash":
        sys.stderr.write("fatal: crashing on purpose\n")
        sys.stderr.flush()
        os._exit(3)
    elif name == "slow":
        threading.Thread(target=slow, args=(id, float(args.get("secs", 10))), daemon=True).start()
    elif name == "ping_me":
        threading.Thread(target=ping_me, args=(id,), daemon=True).start()
    elif name == "change":
        extra_tool = True
        reply(id, text("changed"))
        send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
    else:
        send({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "unknown tool " + str(name)}})


def main():
    if os.environ.get("FAKE_INIT_FAIL"):
        sys.stderr.write("cannot start: missing API key\n")
        sys.stderr.flush()
        sys.exit(1)
    sys.stderr.write("fake server starting\n")
    sys.stderr.flush()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        method = msg.get("method")
        id = msg.get("id")
        if method is None:
            # response to our own request
            if id in pending:
                if "error" in msg:
                    pending[id + "-result"] = msg["error"]["code"]
                pending.pop(id).set()
            continue
        log(method + ("" if id is not None else " (notification)") + (" " + json.dumps(msg.get("params")) if method == "notifications/cancelled" else ""))
        if method == "initialize":
            reply(id, {"protocolVersion": msg["params"]["protocolVersion"], "capabilities": {"tools": {"listChanged": True}}, "serverInfo": {"name": "fake", "version": "1"}})
        elif method == "tools/list":
            reply(id, tools_page((msg.get("params") or {}).get("cursor")))
        elif method == "tools/call":
            call(id, msg["params"])
        elif id is not None:
            send({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "nope"}})


main()
