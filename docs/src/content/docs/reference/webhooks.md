---
title: "Webhook deliveries"
---

This page describes Kairos 0.17.0.

Kairos sends a signed HTTP request to a webhook target when an event occurs
(KAIROS-T-0384). A target is a URL on one board. A board manager makes the
target. Kairos makes the secret of the target and shows it one time.

For the settings of the deliveries, see
[Configuration](../configuration/#webhook-deliveries).

## The request

Kairos sends a `POST` to the URL of the target, with these headers:

| Header | Value |
|---|---|
| `Content-Type` | `application/json` |
| `X-Kairos-Signature` | `sha256=` and the hex of the HMAC-SHA256 of the body. The key is the secret of the target, as text. |
| `X-Kairos-Delivery` | The id of the delivery (UUID). Each try of a delivery has the same id. |
| `X-Kairos-Event` | The event. |
| `User-Agent` | `Kairos-Webhook/<version>` |

The body is a JSON object:

| Field | Description |
|---|---|
| `id` | The id of the delivery. It is the value of `X-Kairos-Delivery`. |
| `event` | The event. It is the value of `X-Kairos-Event`. |
| `created_at` | The time of the event (RFC 3339, UTC). |
| `organization` | The slug of the organization. |
| `data` | The data of the event. |

Kairos does not follow a redirect. A response with a `2xx` code is a good try.
Each other code, a time-out and a connection error make a failed try.

## Check the signature

Calculate the HMAC-SHA256 of the raw body bytes, with the secret as the key.
Compare the result with the value of `X-Kairos-Signature` in constant time.
Refuse a request with a different value.

```python
import hashlib
import hmac

def is_from_kairos(secret: str, body: bytes, header: str) -> bool:
    expected = "sha256=" + hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, header)
```

In a shell, with the raw body in `body.json` and the secret in `SECRET`:

```sh
expected="sha256=$(openssl dgst -sha256 -hmac "$SECRET" < body.json | sed 's/^.* //')"
[ "$expected" = "$SIGNATURE" ] && echo "from Kairos"
```

`SIGNATURE` is the value of `X-Kairos-Signature`. Use the shell check to
find a problem. Do not use it in a runner: its comparison is not in
constant time.

Use the raw body. When you parse the body and write it again, the bytes can
change.

## Tries and the log

Kairos tries a delivery again after a failed try. Each wait is 2 times the
wait before it. A delivery can come more than one time, for example when a
server stops after the request. Use `X-Kairos-Delivery` to find a second
copy.

Kairos keeps a log of each delivery: the status, the response code, the time
and the number of each try. The status of a delivery is one of these values:

| Status | Meaning |
|---|---|
| `pending` | Kairos sends the delivery, or waits for the next try. |
| `delivered` | The target answered with a `2xx` code. |
| `failed` | The delivery used its tries, or Kairos turned off the target. |
| `skipped` | The target was off when the event occurred. Kairos sent nothing. Kairos sends it when the target is on again. |

Kairos keeps a finished delivery for 30 days. This includes a `skipped`
delivery.

## A target that fails

Each failed try adds one to the failures in a row of the target. A good try
sets the count to 0. At the limit (`KAIROS_WEBHOOK_DISABLE_AFTER`), Kairos
turns off the target. Then the person who made the target and the managers
of its board get a notification of the kind `delivery_failing`. A board
manager turns on the target again.

## Skipped deliveries go again

When a board manager turns on a target, Kairos queues its `skipped`
deliveries again (KAIROS-T-0577). Kairos queues them in the order of their
events, oldest first. A delivery keeps its id, so `X-Kairos-Delivery` does
not change. Kairos does not send a delivery that is older than 30 days. A
`failed` delivery does not go again.

The answer of the request that turns on the target has the field
`redelivered`. It is the number of the deliveries that Kairos queued again.

To send the `skipped` deliveries on request, use this request:

```http
POST /api/boards/{id}/webhook-targets/{target_id}/redeliver?since=2026-10-17T09:00:00Z
```

| Parameter | Description |
|---|---|
| `since` | Optional. An RFC 3339 time. Kairos sends only the deliveries of the events at or after this time. |

The request needs `configure_boards` on the board. It is the same capability
as the request that turns on the target. When the target is off, Kairos
refuses the request with the code `CONFLICT`. Then turn on the target.

The CLI has the same action:

```sh
kairos boards dispatch redeliver <BOARD> <TARGET> [--since <TIME>]
```

In the GUI, open **Admin**, then **Boards**, and select the board. In the
section **Agent dispatch**, select **Send skipped again** on the target.

A worker can send 2 or more deliveries at the same time. Thus a receiver can
get the deliveries in a different order. Use `created_at` of the body to put
them in order.

## The addresses that Kairos refuses

The URL must start with `http://` or `https://`. It must not have a user name
or a password. Before each try, Kairos resolves the host and refuses these
addresses:

- loopback (`127.0.0.0/8`, `::1`) and `localhost`
- private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`)
- link-local (`169.254.0.0/16`, `fe80::/10`)
- shared (`100.64.0.0/10`)
- multicast, broadcast and unspecified

The request goes to the address that Kairos checked. To send deliveries to
your own network, set `KAIROS_WEBHOOK_ALLOW_PRIVATE=true`.
