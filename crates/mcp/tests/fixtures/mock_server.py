#!/usr/bin/env python3
"""Mock MCP stdio server for hn-mcp integration tests.

Tools:
  echo {text}          -> text content
  image                -> image content + text
  fail                 -> isError result
  structured           -> structuredContent only
  sleep {ms}           -> replies after ms (runs in a thread; cancellable)
  cancelled            -> JSON list of requestIds received via notifications/cancelled
  ping_client          -> sends a `ping` and `roots/list` request to the client, returns their replies
  add_tool             -> adds tool `extra` and emits notifications/tools/list_changed
  env {name}           -> value of an environment variable
  cwd                  -> current working directory
  spawn_child          -> spawns a long-lived grandchild, returns its pid
tools/list is paginated: 2 tools per page.
"""
import json
import os
import subprocess
import sys
import threading
import time

lock = threading.Lock()
cancelled = []
cancel_events = {}
pending_client = {}  # our request id -> (event, slot)
next_server_id = [1000]
extra = [False]


def send(msg):
    with lock:
        sys.stdout.write(json.dumps(msg) + "\n")
        sys.stdout.flush()


def tool_defs():
    tools = [
        {"name": "echo", "description": "Echo text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
        {"name": "image", "title": "Image tool"},
        {"name": "fail", "description": "Always fails", "inputSchema": {"type": "object"}},
        {"name": "structured", "description": "structured"},
        {"name": "sleep", "description": "sleep", "inputSchema": {"type": "object", "properties": {"ms": {"type": "integer"}}}},
        {"name": "cancelled", "description": "list cancelled"},
        {"name": "ping_client", "description": "ping the client"},
        {"name": "add_tool", "description": "add a tool"},
        {"name": "env", "description": "env var"},
        {"name": "cwd", "description": "cwd"},
        {"name": "spawn_child", "description": "spawn grandchild"},
        {"name": "dotted.name", "description": "name sanitization"},
    ]
    if extra[0]:
        tools.append({"name": "extra", "description": "added later"})
    return tools


def text(t):
    return {"content": [{"type": "text", "text": t}]}


def client_request(method):
    with lock:
        rid = next_server_id[0]
        next_server_id[0] += 1
    ev = threading.Event()
    slot = {}
    pending_client[rid] = (ev, slot)
    send({"jsonrpc": "2.0", "id": rid, "method": method, "params": {}})
    ev.wait(5)
    return slot.get("msg")


def call_tool(rid, name, args):
    if name == "echo":
        return text(args.get("text", ""))
    if name == "image":
        return {"content": [{"type": "text", "text": "an image"}, {"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]}
    if name == "fail":
        return {"content": [{"type": "text", "text": "boom"}], "isError": True}
    if name == "structured":
        return {"content": [], "structuredContent": {"answer": 42}}
    if name == "sleep":
        ev = threading.Event()
        cancel_events[rid] = ev
        if ev.wait(args.get("ms", 1000) / 1000.0):
            return None  # cancelled: no response
        return text("slept")
    if name == "cancelled":
        return text(json.dumps(cancelled))
    if name == "ping_client":
        a = client_request("ping")
        b = client_request("roots/list")
        c = client_request("sampling/createMessage")
        return text(json.dumps([a, b, c]))
    if name == "add_tool":
        extra[0] = True
        send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
        return text("added")
    if name == "env":
        return text(os.environ.get(args.get("name", ""), "<unset>"))
    if name == "cwd":
        return text(os.getcwd())
    if name == "spawn_child":
        p = subprocess.Popen(["sleep", "300"], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return text(str(p.pid))
    if name == "extra":
        return text("extra!")
    return None


def handle_call(rid, params):
    name = params.get("name")
    names = [t["name"] for t in tool_defs()]
    if name not in names:
        send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32602, "message": "unknown tool " + str(name)}})
        return
    res = call_tool(rid, name, params.get("arguments") or {})
    if res is not None:
        send({"jsonrpc": "2.0", "id": rid, "result": res})


def main():
    sys.stderr.write("mock server starting\n")
    sys.stderr.flush()
    # garbage on stdout must be tolerated
    sys.stdout.write("this is not json\n")
    sys.stdout.flush()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        method = msg.get("method")
        rid = msg.get("id")
        if method is None:
            # response to one of our requests
            entry = pending_client.pop(rid, None)
            if entry:
                entry[1]["msg"] = msg
                entry[0].set()
            continue
        if method == "initialize":
            send({"jsonrpc": "2.0", "id": rid, "result": {
                "protocolVersion": msg["params"]["protocolVersion"],
                "capabilities": {"tools": {"listChanged": True}},
                "serverInfo": {"name": "mock", "version": "1.0"},
                "instructions": "Use the mock tools wisely.",
                "_clientInfo": msg["params"]["clientInfo"],
            }})
        elif method == "notifications/initialized":
            send({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info", "data": "hello"}})
        elif method == "tools/list":
            cursor = int((msg.get("params") or {}).get("cursor") or 0)
            tools = tool_defs()
            page = tools[cursor:cursor + 2]
            res = {"tools": page}
            if cursor + 2 < len(tools):
                res["nextCursor"] = str(cursor + 2)
            send({"jsonrpc": "2.0", "id": rid, "result": res})
        elif method == "tools/call":
            threading.Thread(target=handle_call, args=(rid, msg.get("params") or {}), daemon=True).start()
        elif method == "notifications/cancelled":
            req = msg["params"]["requestId"]
            cancelled.append(req)
            ev = cancel_events.get(req)
            if ev:
                ev.set()
        elif rid is not None:
            send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "Method not found"}})


if __name__ == "__main__":
    main()
