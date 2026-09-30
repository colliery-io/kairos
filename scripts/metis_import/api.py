"""The HTTP and MCP client of the import. Standard library only.

- One request at a time.
- A 5xx answer or a network error: 3 more tries with backoff (1 s, 2 s,
  4 s), then Stop. A write that is not safe to send two times (a create)
  asks for retry=False and does its own retry, see run_remap.create.
- A count of the requests, per method, for the report.
- Credentials go in the Authorization header only. They are never printed.
"""

import json
import time
import urllib.error
import urllib.parse
import urllib.request

LOCAL_HOSTS = ("127.0.0.1", "localhost", "::1")
BACKOFF = (1, 2, 4)


class Stop(Exception):
    pass


def is_local(url):
    return (urllib.parse.urlparse(url).hostname or "") in LOCAL_HOSTS


class Api:
    def __init__(self, url, key, admin=None, tenant=None, sleep=time.sleep):
        self.url = url.rstrip("/")
        self.key = key
        self.admin = admin
        self.tenant = tenant
        self.sleep = sleep
        self.sessions = {}
        self.rpc_id = 0
        self.count = {}
        self.seconds = 0.0

    @property
    def requests(self):
        return sum(self.count.values())

    def _once(self, method, path, body, admin, extra):
        token = self.admin if admin else self.key
        if not token:
            raise Stop("no %s for %s %s" % (
                "KAIROS_ADMIN_TOKEN" if admin else "KAIROS_KEY", method, path))
        headers = {"authorization": "Bearer " + token, "accept": "application/json"}
        if self.tenant:
            headers["x-tenant"] = self.tenant
        data = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["content-type"] = "application/json"
        headers.update(extra or {})
        req = urllib.request.Request(self.url + path, data=data, method=method, headers=headers)
        self.count[method] = self.count.get(method, 0) + 1
        started = time.monotonic()
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return resp.status, resp.read().decode("utf-8"), resp.headers
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode("utf-8", "replace"), e.headers
        finally:
            self.seconds += time.monotonic() - started

    def call(self, method, path, body=None, admin=False, extra=None, retry=True):
        """Send one request. Returns (status, text, headers). With retry, a
        5xx answer or a network error is sent again, up to 3 times."""
        tries = (0,) + BACKOFF if retry else (0,)
        last = None
        for wait in tries:
            if wait:
                self.sleep(wait)
            try:
                status, raw, headers = self._once(method, path, body, admin, extra)
            except (urllib.error.URLError, OSError) as e:
                last = "network error: %s" % e
                continue
            if status >= 500:
                last = "HTTP %s: %s" % (status, raw[:300])
                continue
            return status, raw, headers
        raise Stop("%s %s failed %d times (%s). The state file is correct. "
                   "Run the command again to continue." % (method, path, len(tries), last))

    def json(self, method, path, body=None, admin=False, ok=(200, 201)):
        status, raw, _ = self.call(method, path, body, admin)
        if status not in ok:
            raise Stop("%s %s -> HTTP %s: %s" % (method, path, status, raw[:500]))
        return json.loads(raw) if raw.strip() else {}

    # --- MCP --------------------------------------------------------------
    def _rpc(self, payload, admin, notify=False):
        extra = {"accept": "application/json, text/event-stream"}
        session = self.sessions.get(admin)
        if session:
            extra["mcp-session-id"] = session
        status, raw, headers = self.call("POST", "/mcp", payload, admin=admin, extra=extra)
        if status not in (200, 202):
            raise Stop("mcp %s -> HTTP %s: %s" % (payload.get("method"), status, raw[:300]))
        if headers.get("mcp-session-id"):
            self.sessions[admin] = headers.get("mcp-session-id")
        if notify:
            return None
        for line in raw.splitlines():
            line = line.strip()
            if line.startswith("data:"):
                line = line[5:].strip()
            if line.startswith("{"):
                msg = json.loads(line)
                if "result" in msg or "error" in msg:
                    return msg
        raise Stop("mcp %s: no JSON-RPC answer" % payload.get("method"))

    def tool(self, name, arguments, admin=False):
        """Call one MCP tool. Returns (ok, text)."""
        if admin not in self.sessions:
            self._rpc({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "migrate-metis-to-kairos", "version": "2"}}}, admin)
            self._rpc({"jsonrpc": "2.0", "method": "notifications/initialized"}, admin, notify=True)
        self.rpc_id += 1
        msg = self._rpc({"jsonrpc": "2.0", "id": self.rpc_id, "method": "tools/call",
                         "params": {"name": name, "arguments": arguments}}, admin)
        if "error" in msg:
            return False, json.dumps(msg["error"])
        result = msg["result"]
        text = " ".join(c.get("text", "") for c in result.get("content", []))
        return not result.get("isError", False), text
