"""Reusable async helpers for multi-step ref-server E2E scenarios.

Usage:
    from e2elib import Server, Client
    srv = Server(); await srv.start()
    c = await Client.connect(srv.port, cwd)
    sid = await c.create(model="dgn/claude-sonnet-5.5")
    await c.prompt(sid, "hi"); await c.wait_idle(sid)
"""
import asyncio, json, os, subprocess, time
import websockets

ROOT = "/home/zzk/app/ref"
BIN = f"{ROOT}/target/debug/ref-server"
os.environ.setdefault("REF_STATE_DIR", "/tmp/dev3src/e2e-state")
os.environ.setdefault("REF_DATA_DIR", "/tmp/dev3src/e2e-data")


def mref(s):
    p, m = s.split("/", 1)
    v = None
    if "#" in m:
        m, v = m.split("#", 1)
    r = {"provider": p, "model": m}
    if v:
        r["variant"] = v
    return r


class Server:
    def __init__(self, port=None, args=(), env=None):
        self.port = port or (20000 + (os.getpid() * 7 + int(time.time())) % 20000)
        self.args = list(args)
        self.env = env
        self.proc = None

    async def start(self):
        env = dict(os.environ, **(self.env or {}))
        self.proc = subprocess.Popen([BIN, "--port", str(self.port), "--idle-timeout", "0", "--token", "t", *self.args],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
        for _ in range(100):
            try:
                ws = await websockets.connect(f"ws://127.0.0.1:{self.port}/?token=t")
                await ws.close()
                return self
            except Exception:
                await asyncio.sleep(0.1)
        raise RuntimeError("server did not start")

    def stop(self, sig="term"):
        if self.proc and self.proc.poll() is None:
            if sig == "kill":
                self.proc.kill()
            else:
                self.proc.terminate()
            self.proc.wait()


class Client:
    def __init__(self, ws, cwd, name="c", verbose=True):
        self.ws, self.cwd, self.name, self.verbose = ws, cwd, name, verbose
        self.rid = 0
        self.waiters = {}
        self.events = []  # (time, session_id, event)
        self.cond = asyncio.Condition()
        self.status = {}
        self.auto_approve = True
        self.perm_log = []
        self.t0 = time.time()
        self.task = asyncio.create_task(self._reader())

    @classmethod
    async def connect(cls, port, cwd, name="c", verbose=True):
        os.makedirs(cwd, exist_ok=True)
        ws = await websockets.connect(f"ws://127.0.0.1:{port}/?token=t", max_size=None)
        c = cls(ws, cwd, name, verbose)
        await c.req("hello", client="e2e-" + name, version=1, cwd=cwd, token="t")
        return c

    def log(self, *a):
        if self.verbose:
            print(f"[{self.name} {time.time()-self.t0:6.1f}]", *a, flush=True)

    async def _reader(self):
        try:
            async for raw in self.ws:
                f = json.loads(raw)
                k = f.get("kind")
                if k == "response":
                    fut = self.waiters.pop(f["id"], None)
                    if fut and not fut.done():
                        fut.set_result(f)
                elif k == "event":
                    sid, ev = f["session_id"], f["event"]
                    self._show(sid, ev)
                    if ev["type"] == "info":
                        self.status[sid] = ev["info"]
                    if ev["type"] == "permission" and self.auto_approve:
                        r = ev["request"]
                        self.perm_log.append(r)
                        asyncio.create_task(self.req("permission_reply", session_id=sid, call_id=r["call_id"], allow=True))
                    async with self.cond:
                        self.events.append((time.time(), sid, ev))
                        self.cond.notify_all()
                elif k == "toast":
                    self.log("TOAST", f.get("level"), f.get("text"))
        except Exception as e:
            self.log("reader ended", repr(e))

    def _show(self, sid, ev):
        t = ev["type"]
        s = sid[-6:]
        if t == "part":
            p = ev["part"]
            if p["type"] == "tool" and p["state"] in ("done", "error", "denied", "cancelled"):
                out = (p.get("output") or "")[:400].replace("\n", "\\n")
                self.log(f"{s} TOOL {p['name']} [{p['state']}] ({p.get('purpose')}) {json.dumps(p.get('input'), ensure_ascii=False)[:200]} -> {out}")
            elif p["type"] == "tool" and p["state"] in ("delayed", "awaiting_permission", "running"):
                self.log(f"{s} tool {p['name']} {p['state']} run_at={p.get('run_at')}")
        elif t == "message":
            m = ev["message"]
            if m["role"] == "assistant" and m.get("time_end"):
                txt = " | ".join(p.get("text", "") for p in m["parts"] if p["type"] == "text")[:800]
                rs = sum(len(p.get("text", "")) for p in m["parts"] if p["type"] == "reasoning")
                self.log(f"{s} ASSISTANT usage={m.get('usage')} err={m.get('error')} reasoning={rs} text={txt!r}")
            elif m["role"] == "user":
                txt = m["parts"][0].get("text", "")[:300] if m["parts"] else ""
                self.log(f"{s} USER({m.get('source', {}).get('kind')}): {txt!r}")
        elif t == "info":
            st = ev["info"]["status"]
            if st["state"] == "retrying":
                self.log(f"{s} RETRYING attempt={st['attempt']} in={(st['next_at']/1000-time.time()):.1f}s err={st['error'][:150]}")
            elif st["state"] in ("compacting",):
                self.log(f"{s} STATUS {st['state']}")
        elif t == "reset":
            self.log(f"{s} RESET -> {len(ev['messages'])} messages")
        elif t == "error":
            self.log(f"{s} ERROR EVENT {ev['text'][:300]}")
        elif t == "removed":
            self.log(f"{s} REMOVED {ev['message_id']}")
        elif t == "closed":
            self.log(f"{s} CLOSED")

    async def req(self, op, **kw):
        self.rid += 1
        rid = self.rid
        fut = asyncio.get_event_loop().create_future()
        self.waiters[rid] = fut
        await self.ws.send(json.dumps({"id": rid, "op": op, **kw}))
        r = await asyncio.wait_for(fut, 120)
        if not r.get("ok"):
            self.log(f"ERR {op}: {r.get('error')}")
        return r

    async def create(self, model=None, agent=None, name=None, attach=True, foreground=True, cwd=None):
        kw = {"cwd": cwd or self.cwd}
        if model:
            kw["model"] = mref(model)
        if agent:
            kw["agent"] = agent
        if name:
            kw["name"] = name
        r = await self.req("create_session", **kw)
        sid = r["result"]["session"]["id"]
        if attach:
            await self.req("attach", session_id=sid, foreground=foreground)
        return sid

    async def prompt(self, sid, text, images=None):
        mark = len(self.events)
        await self.req("prompt", session_id=sid, text=text, images=images or [])
        return mark

    async def wait_for(self, pred, timeout=300, start=0):
        """Wait until pred(sid, ev) is true for an event at index >= start. Returns index."""
        deadline = time.time() + timeout
        i = start
        async with self.cond:
            while True:
                while i < len(self.events):
                    _, sid, ev = self.events[i]
                    if pred(sid, ev):
                        return i
                    i += 1
                rem = deadline - time.time()
                if rem <= 0:
                    raise TimeoutError("wait_for timed out")
                try:
                    await asyncio.wait_for(self.cond.wait(), rem)
                except asyncio.TimeoutError:
                    raise TimeoutError("wait_for timed out")

    async def wait_idle(self, sid, start=0, timeout=300):
        """Wait for busy -> idle transition after event index `start`."""
        busy = [False]

        def pred(s, ev):
            if s != sid or ev["type"] != "info":
                return False
            st = ev["info"]["status"]["state"]
            if st != "idle":
                busy[0] = True
                return False
            return busy[0]
        return await self.wait_for(pred, timeout, start)

    def tool_parts(self, sid, start=0):
        out = []
        for _, s, ev in self.events[start:]:
            if s == sid and ev["type"] == "part" and ev["part"]["type"] == "tool" and ev["part"]["state"] in ("done", "error", "denied", "cancelled"):
                out.append(ev["part"])
        return out

    async def snapshot(self, sid):
        r = await self.req("attach", session_id=sid, foreground=False)
        return r["result"]["snapshot"]

    async def close(self):
        await self.ws.close()
