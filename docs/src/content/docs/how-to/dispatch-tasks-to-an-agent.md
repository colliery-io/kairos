---
title: "Dispatch tasks to an agent runner"
---

This page describes Kairos 0.19.0.

Send a task to an agent when the task enters a column of a board. For
example, a bug that enters Todo can start a Claude Code session that works on
the bug. Kairos does not start the session itself. Kairos sends a signed HTTP
request to a runner, and the runner starts the agent.

A task enters a column by a transition, a move to another board, a cancel or a
create. A new task starts in the entry column of its board. A task from forge
intake starts there too. So a rule on the entry column fires for each new task.

**Before you start:**

- You have `configure_boards` on the board, or you are an organization admin.
- The deployment has `KAIROS_SECRETS_KEY`. Kairos keeps the secret of each
  target sealed with this key.
- You have a runner: an HTTP service that Kairos can reach. A runner on your
  own network needs `KAIROS_WEBHOOK_ALLOW_PRIVATE=true` on the deployment.

## 1. Write the runner

The runner gets a `POST` with a JSON body. It does 3 things:

1. It checks the signature in `X-Kairos-Signature` with the secret of the
   target.
2. It answers `204` fast. Kairos waits 10 seconds for the answer, and then
   tries again.
3. It starts the agent with the code and the link of the task.

This example in Python starts Claude Code in print mode. It uses only the
standard library.

```python
import hashlib
import hmac
import json
import os
import subprocess
from http.server import BaseHTTPRequestHandler, HTTPServer

SECRET = os.environ["KAIROS_WEBHOOK_SECRET"]
SEEN = set()

class Runner(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        expected = "sha256=" + hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()
        if not hmac.compare_digest(expected, self.headers.get("X-Kairos-Signature", "")):
            self.send_response(401)
            self.end_headers()
            return
        self.send_response(204)
        self.end_headers()
        delivery = self.headers["X-Kairos-Delivery"]
        if delivery in SEEN or self.headers["X-Kairos-Event"] != "task.entered_column":
            return
        SEEN.add(delivery)
        task = json.loads(body)["data"]
        prompt = f"Implement {task['code']}: {task['title']} ({task['link']})."
        subprocess.Popen(["claude", "-p", prompt])

HTTPServer(("0.0.0.0", 8787), Runner).serve_forever()
```

Other runners can do the same work. Examples are a CI job with a webhook
trigger, a queue, or a Claude Code routine behind a small proxy. Each runner
must check the signature before it starts an agent.

## 2. Make the target

```sh
kairos boards dispatch add-target platform-delivery \
  --name "Agent runner" --hook-url https://runner.example.com/kairos
# → Made the webhook target Agent runner (6f1a…) for https://runner.example.com/kairos.
# → secret: kairos_whsec_3b9e…
```

Keep the secret in the runner, for example as `KAIROS_WEBHOOK_SECRET`. Kairos
shows the secret one time. If you lose it, remove the target and make a new
one.

Send a test delivery, and look at the log:

```sh
kairos boards dispatch test-target platform-delivery 6f1a…
kairos boards dispatch log platform-delivery
```

The event of a test delivery is `ping`.

## 3. Make the rule

```sh
kairos boards dispatch add-rule platform-delivery \
  --column Todo --target 6f1a… --task-type bug
# → Made the dispatch rule 9c2d…: type bug that enters Todo goes to Agent runner.
```

The filter is optional. Each part that you set must match:

- `--task-type`: `task`, `bug`, `tech_debt` or `support`.
- `--metadata`: a metadata value, for example `priority=high`.
- `--repository`: the repository of the task.

A rule on an initiative board fires when an initiative enters the column
(KAIROS-T-0719). The event is `initiative.entered_column`; its `data` has
no task type, lane or repository. A strategy board does the same with
`strategy.entered_column`. Such a rule takes the metadata filter only.

A rule can also fire when an item loses its last open blocker. The blocker
goes into a done column, or a person archives or purges it. A person can also
remove the `blocks` link. A cancel does not fire the rule: a cancelled blocker
still blocks. The item can be a task, an initiative or a
strategy.

The event is `task.unblocked`. Its `blocker` is the code of the blocker. Use
this trigger to start a wave when an upstream task is done:

```sh
kairos boards dispatch add-rule platform-delivery \
  --trigger unblocked --column Todo --target 6f1a…
# → Made the dispatch rule 4e7b…: each task in Todo that is unblocked goes to Agent runner.
```

Without `--column`, a task in each column that is not done matches.

The rule names no done column: a task in a done column waits for nothing.

A rule can also fire when the review state of a task changes
(`kairos tasks review`, the MCP tool `set_review`). The event is
`task.review_changed`; its `review` is the new value, `not_needed`, `needed`
or `done`. Use this trigger to send a task to a reviewer:

```sh
kairos boards dispatch add-rule platform-delivery \
  --trigger review_changed --target 6f1a…
# → Made the dispatch rule 9c2d…: each task whose review state changes goes to Agent runner.
```

An agent can make the same rule with the MCP tool `create_dispatch_rule`. With
`url` in place of `target_id`, the tool makes the target too, and its answer
has the secret.

## 4. Look at the deliveries

Move a matching task into the column. Then look at the log:

```sh
kairos boards dispatch log platform-delivery
```

Each delivery has a status: `pending`, `delivered`, `failed` or `skipped`.
Kairos tries a failed delivery again, with longer waits, for about 17 hours.
After 24 failed tries in a row, Kairos turns off the target, and you get a
notification. Repair the runner, then turn on the target again:

```sh
kairos boards dispatch enable-target platform-delivery 6f1a…
```

Then Kairos sends the deliveries that it skipped while the target was off.
It sends the deliveries of the last 30 days, oldest first. The command
`redeliver` sends them again on request.

The GUI has the same targets, rules and log. Open **Admin**, then
**Boards**, and select the board. Use the section **Agent dispatch**:

1. Put a name and the URL of the runner, and select **Add target**. Copy
   the secret that the page shows.
2. Select the trigger in **A task that**: **enters the column**, or the
   trigger for the last blocker. Then select the column, the target and a
   filter, and select **Add rule**. With the trigger for the last blocker,
   the column can be **any column**.
3. Read the **Delivery log**. Select **Refresh** to read it again. A page
   shows 50 deliveries. Select **Older** for the next page, and **Newer** to
   go back.
4. Select **Turn on** on a target that is off. Select **Send skipped
   again** to send its skipped deliveries on request.

With the CLI, `--before <id>` shows the deliveries older than the delivery
`<id>`. Send the id of the last delivery of a page to get the next page.

## Related reading

- [Webhook deliveries](../../reference/webhooks/): the request, the
  signature, the tries and the addresses that Kairos refuses.
- [MCP tools](../../reference/mcp-tools/#agent-dispatch): the dispatch tools.
- [CLI](../../reference/cli/#kairos-boards-dispatch): the dispatch commands.
- [Read your notifications](../read-your-notifications/): the notification
  of a target that Kairos turned off.
