#!/usr/bin/env python3
"""E2E driver: start ref-server, create session, send prompt, print event stream summary."""
import asyncio, json, os, subprocess, sys, time, glob
import websockets

ROOT = "/home/zzk/app/ref"
BIN = f"{ROOT}/target/debug/ref-server"
STATE = os.environ.setdefault("REF_STATE_DIR", "/tmp/dev3src/e2e-state")
os.environ.setdefault("REF_DATA_DIR", "/tmp/dev3src/e2e-data")


async def main():
    model = sys.argv[1]
    prompt = sys.argv[2]
    cwd = sys.argv[3] if len(sys.argv) > 3 else "/tmp/dev3src/e2e-work"
    agent = os.environ.get("E2E_AGENT")
    os.makedirs(cwd, exist_ok=True)
    port = 23456 + os.getpid() % 1000
    proc = subprocess.Popen([BIN, "--port", str(port), "--idle-timeout", "0", "--token", "t"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            try:
                ws = await websockets.connect(f"ws://127.0.0.1:{port}/?token=t", max_size=None)
                break
            except Exception:
                await asyncio.sleep(0.1)
        rid = 0
        pending = {}

        async def req(op, **kw):
            nonlocal rid
            rid += 1
            await ws.send(json.dumps({"id": rid, "op": op, **kw}))
            return rid

        await req("hello", client="e2e", version=1, cwd=cwd, token="t")
        p, m = model.split("/", 1)
        variant = None
        if "#" in m:
            m, variant = m.split("#", 1)
        mr = {"provider": p, "model": m}
        if variant:
            mr["variant"] = variant
        cid = await req("create_session", cwd=cwd, model=mr, **({"agent": agent} if agent else {}))
        sid = None
        t0 = time.time()
        idle_seen = False
        busy_seen = False
        reasoning_chars = 0
        text = ""
        async for raw in ws:
            f = json.loads(raw)
            k = f.get("kind")
            if k == "response":
                if not f.get("ok"):
                    print("ERR response", f)
                if f["id"] == cid:
                    sid = f["result"]["session"]["id"]
                    await req("attach", session_id=sid, foreground=True)
                    await req("prompt", session_id=sid, text=prompt)
                continue
            if k == "event":
                ev = f["event"]
                t = ev["type"]
                if t == "delta":
                    d = ev["delta"]
                    if d["type"] == "reasoning":
                        reasoning_chars += len(d["text"])
                    elif d["type"] == "text":
                        text += d["text"]
                elif t == "part":
                    part = ev["part"]
                    if part["type"] == "tool" and part["state"] in ("done", "error", "denied", "cancelled"):
                        out = (part.get("output") or "")[:300].replace("\n", "\\n")
                        print(f"[{time.time()-t0:5.1f}] TOOL {part['name']} ({part.get('purpose')}) {part['state']}: {json.dumps(part.get('input'),ensure_ascii=False)[:200]} -> {out}")
                    elif part["type"] == "tool":
                        print(f"[{time.time()-t0:5.1f}] tool {part['name']} state={part['state']}")
                elif t == "message":
                    msg = ev["message"]
                    if msg["role"] == "assistant" and msg.get("time_end"):
                        print(f"[{time.time()-t0:5.1f}] ASSISTANT done usage={msg.get('usage')} err={msg.get('error')} reasoning_chars={reasoning_chars}")
                        if text:
                            print("   TEXT:", text[:1500])
                        text = ""
                        reasoning_chars = 0
                    elif msg["role"] == "user":
                        print(f"[{time.time()-t0:5.1f}] USER({msg.get('source',{}).get('kind')}): {msg['parts'][0].get('text','')[:200] if msg['parts'] else ''}")
                elif t == "info":
                    st = ev["info"]["status"]
                    if st["state"] != "idle":
                        busy_seen = True
                    if st["state"] == "retrying":
                        print("RETRY", st)
                    if st["state"] == "idle" and busy_seen:
                        idle_seen = True
                        print(f"[{time.time()-t0:5.1f}] IDLE; context={ev['info'].get('context')}")
                        break
                elif t == "permission":
                    r = ev["request"]
                    print("PERMISSION", r["tool"], r["input"])
                    await req("permission_reply", session_id=sid, call_id=r["call_id"], allow=True)
                elif t == "error":
                    print("ERROR EVENT", ev["text"])
            elif k == "toast":
                print("TOAST", f)
        await ws.close()
    finally:
        time.sleep(0.5)
        proc.terminate()
        proc.wait()


asyncio.run(main())
