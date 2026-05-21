# Feishu Endpoint Research Notes — 2026-05-18

Researcher: subagent
Method: WebFetch on open.feishu.cn was blocked. Used WebSearch + direct gh API / raw.githubusercontent.com pulls of larksuite/node-sdk (official) and larksuite/oapi-sdk-go (official) source code, plus WebSearch summaries of open.feishu.cn doc pages.

Sources cited:
- https://github.com/larksuite/node-sdk (official Node SDK)
- https://raw.githubusercontent.com/larksuite/node-sdk/main/scene/registration/index.ts (registerApp impl)
- https://raw.githubusercontent.com/larksuite/node-sdk/main/scene/registration/types.ts (types)
- https://raw.githubusercontent.com/larksuite/node-sdk/main/ws-client/ws-config.ts (WS handshake URL)
- https://raw.githubusercontent.com/larksuite/node-sdk/main/ws-client/index.ts (WS pull-connect-config impl)
- https://github.com/larksuite/oapi-sdk-go (official Go SDK)
- https://raw.githubusercontent.com/larksuite/oapi-sdk-go/v3_main/sample/apiall/cardkitv1/content_cardElement.go (CardKit streaming PUT shape)
- https://raw.githubusercontent.com/larksuite/oapi-sdk-go/v3_main/sample/apiall/imv1/get_messageResource.go (message resource GET shape)
- https://open.feishu.cn/document/server-docs/authentication-management/access-token/tenant_access_token_internal (via WebSearch summary)
- https://open.feishu.cn/document/server-docs/im-v1/message/create (via WebSearch summary)
- https://open.feishu.cn/document/server-docs/im-v1/message/events/receive (via WebSearch summary)
- https://open.feishu.cn/document/cardkit-v1/card/create (via WebSearch summary)
- https://open.feishu.cn/document/cardkit-v1/streaming-updates-openapi-overview (via WebSearch summary)
- https://open.feishu.cn/document/faq/trouble-shooting/how-to-fix-99991663-error (via WebSearch summary)

---

## Q1: Device authorization grant

**Verdict: EXISTS, but the plan's URLs are WRONG.**

The plan speculated `https://passport.feishu.cn/suite/passport/oauth/authorize/device` and `.../oauth/token` — these come from a misread of an OpenClaw plugin and **do not match** the real implementation. The actual flow uses a single endpoint on a different host and follows RFC 8628.

**Real endpoints (from official `larksuite/node-sdk` source `scene/registration/index.ts`):**

- **Host (Feishu):** `https://accounts.feishu.cn`
- **Host (Lark/intl):** `https://accounts.larksuite.com` (auto-switch if poll response has `user_info.tenant_brand === 'lark'`)
- **Single endpoint, both begin + poll:** `POST /oauth/v1/app/registration`
- **Content-Type:** `application/x-www-form-urlencoded` (NOT JSON)

**Begin request body (form-encoded):**
```
action=begin
archetype=PersonalAgent
auth_method=client_secret
request_user_info=open_id
```

**Begin response (JSON):**
```json
{
  "device_code": "...",
  "verification_uri_complete": "https://...",
  "verification_uri": "https://...",
  "user_code": "...",
  "interval": 5,         // poll every 5s
  "expires_in": 600      // device_code TTL in seconds
}
```

**Poll request body:**
```
action=poll
device_code=<from begin>
```

**Poll response (JSON, RFC 8628):**
- Success: `{ "client_id": "cli_xxx", "client_secret": "...", "user_info": { "open_id": "...", "tenant_brand": "feishu" | "lark" } }`
- Pending: `{ "error": "authorization_pending" }`
- Slow down (increase poll interval by 5s): `{ "error": "slow_down" }`
- User denied: `{ "error": "access_denied", "error_description": "..." }`
- Device code expired: `{ "error": "expired_token", "error_description": "..." }`
- Errors return as HTTP 400 with body, NOT as `{ "code": N }` shape

**Field names — the plan has them wrong:** Real fields are `client_id` / `client_secret` (RFC 8628 standard), NOT `app_id` / `app_secret`. They are equivalent (`client_id == app_id` for the same custom app, both look like `cli_xxx`), but **the device-flow response keys are `client_id`/`client_secret`**.

**Required scopes:** None at this step — the user picks scopes interactively on the verification page in the Feishu/Lark client. `archetype=PersonalAgent` is the only parameter that defines the app type.

**Verification URI:** The user is expected to scan/open `verification_uri_complete` in the Feishu/Lark client (not a generic web browser). The Node SDK appends `from=sdk`, `source=node-sdk`, `tp=sdk` query params before showing.

Confidence: **HIGH** — source code is the source of truth.

---

## Q2: tenant_access_token

**Verdict: CONFIRMED VERBATIM.**

- **URL:** `POST https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal`
- **Content-Type:** `application/json` (NOT form-encoded)
- **Request body:**
  ```json
  { "app_id": "cli_xxx", "app_secret": "..." }
  ```
- **Response fields:**
  ```json
  {
    "code": 0,
    "msg": "ok",
    "tenant_access_token": "t-...",
    "expire": 7200
  }
  ```
- **Token TTL:** 7200 seconds (2 hours). Plan's spec is correct.
- **Auth required:** None (this IS the auth endpoint).

Note: The device-flow returns the credentials as `client_id`/`client_secret`, but here they are sent as `app_id`/`app_secret` — same values, different field names. PR2 needs to remember to rename when persisting.

Confidence: **HIGH**.

---

## Q3: Inbound WebSocket vs Webhook

**Verdict: BOTH ARE OFFICIALLY SUPPORTED. The plan's Stream model is correct in principle. But the handshake URL is different from what the plan implies.**

WebSocket "long-connection / persistent connection" mode is documented at https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/event-subscription-guide/callback-subscription/configure-callback-request-address and is used by the official node-sdk `ws-client`. Webhook (HTTP POST callback) is the alternative.

**Handshake URL (from `ws-client/ws-config.ts` and `ws-client/index.ts`):**

- **Step 1 — Pull connect config:** `POST ${domain}/callback/ws/endpoint`
  - `domain = https://open.feishu.cn` for Feishu, `https://open.larksuite.com` for Lark
  - Full URL: `POST https://open.feishu.cn/callback/ws/endpoint`
  - Request body (JSON):
    ```json
    { "AppID": "cli_xxx", "AppSecret": "..." }
    ```
    Note: capitalized `AppID` / `AppSecret`, not snake_case.
  - Required HTTP headers: `locale: zh`, `User-Agent: <sdk-ua>`
  - Response (JSON):
    ```json
    {
      "code": 0,
      "msg": "ok",
      "data": {
        "URL": "wss://gateway-xxx.feishu.cn/...?device_id=...&service_id=...",
        "ClientConfig": {
          "PingInterval": 120,        // seconds
          "ReconnectCount": -1,       // -1 = infinite
          "ReconnectInterval": 120,   // seconds
          "ReconnectNonce": 30        // seconds jitter
        }
      }
    }
    ```
  - `device_id` and `service_id` are extracted from the URL's query string.

- **Step 2 — Open WS:** `new WebSocket(URL)` (no separate ticket header; the URL itself contains the auth).

- **Frame format:** Protobuf-encoded frames. The SDK uses `pbbp2` proto schema in `ws-client/proto-buf/`. NOT plain JSON-over-WS like dingtalk. Frame types include connect-ack, ping, control, business (the event payload, where `payload_type` indicates `card` or `event` and the body itself is gzip-compressed JSON).

- **Ping interval:** 120 seconds (from ClientConfig).
- **Reconnect:** Server-driven via ClientConfig fields. Plan's `ReconnectBackoff` 5/15/30/60s ladder is fine to keep as transport-layer backoff, but ping should be aligned with `PingInterval`.

**Webhook mode (alternative):**
- Register a public callback URL in the developer console.
- Feishu POSTs encrypted event payloads to that URL.
- For desktop client distribution this is infeasible (no public IP); WS is the correct choice.

Confidence: **HIGH** for the URL and request shape. **MEDIUM** for the proto-buf frame structure — implementing this in Rust requires writing a custom protobuf decoder. Either:
- Pull `pbbp2.proto` from `larksuite/node-sdk` and run `prost-build` in Rust, OR
- Re-port a pre-built Rust SDK (e.g. `lark-rs`) that already has the proto schema.

This is a non-trivial cost the plan undercounted. PR3 will be **larger than initially scoped** because frame parsing is not JSON.

---

## Q4: CardKit

**Verdict: ENDPOINTS DIFFER FROM PLAN — streaming update is PUT not PATCH.**

From `larksuite/oapi-sdk-go` sample sources:

| Operation | Method | Path | Verified source |
|---|---|---|---|
| Create card | POST | `/open-apis/cardkit/v1/cards` | `sample/apiall/cardkitv1/create_card.go` |
| Settings (lifecycle/streaming flag) | PATCH | `/open-apis/cardkit/v1/cards/:card_id/settings` | `settings_card.go` |
| ID convert | POST | `/open-apis/cardkit/v1/cards/id_convert` | `idConvert_card.go` |
| Update whole card | PUT | `/open-apis/cardkit/v1/cards/:card_id` | `update_card.go` |
| Batch update card | POST | `/open-apis/cardkit/v1/cards/:card_id/batch_update` | `batchUpdate_card.go` |
| Insert element | POST | `/open-apis/cardkit/v1/cards/:card_id/elements` | `create_cardElement.go` |
| Patch element (props) | PATCH | `/open-apis/cardkit/v1/cards/:card_id/elements/:element_id` | `patch_cardElement.go` |
| Update element (whole) | PUT | `/open-apis/cardkit/v1/cards/:card_id/elements/:element_id` | `update_cardElement.go` |
| **Stream content update** | **PUT** | `/open-apis/cardkit/v1/cards/:card_id/elements/:element_id/content` | `content_cardElement.go` |
| Delete element | DELETE | `/open-apis/cardkit/v1/cards/:card_id/elements/:element_id` | `delete_cardElement.go` |

**The plan's `PATCH /cards/{card_id}/elements/{element_id}/content` is WRONG — it is PUT.** Body shape is the same:
```json
{
  "uuid": "<idempotency-key>",
  "content": "<delta-or-full-text>",
  "sequence": <monotonic-increasing-int>
}
```

`sequence` is a monotonic int per `card_id+element_id`, used to enforce ordering at the server. Errcode 230002 (sequence error) per plan likely IS valid (haven't confirmed exact code on errcode reference page, but the streaming-update doc page mentions sequence/ordering errors).

**Delivery to chat:** Two-step.
1. `POST /open-apis/cardkit/v1/cards` returns `card_id`.
2. `POST /open-apis/im/v1/messages?receive_id_type=...` with `msg_type: "interactive"`, `content: JSON.stringify({ "type": "card", "data": { "card_id": "<card_id>" } })` (the wrapper schema; see Q5).

The create-card endpoint does NOT take a target chat — it creates a card "entity"; you have to deliver it via im.message.create. This matches the issue thread from hermes-agent #16084.

Confidence: **HIGH** for endpoint paths/methods. **MEDIUM** for the exact `interactive` wrapper content shape (verified loosely from search results, not exact JSON; doc page open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards needs manual verification at impl time).

---

## Q5: Send text/markdown reply

**Verdict: URL CONFIRMED. content field is JSON-STRINGIFIED.**

- **URL:** `POST https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={open_id|chat_id|user_id|union_id|email}`
- **Auth:** `Authorization: Bearer <tenant_access_token>`
- **Content-Type:** `application/json`
- **Request body:**
  ```json
  {
    "receive_id": "ou_xxx" | "oc_xxx" | ...,
    "msg_type": "text",
    "content": "{\"text\":\"hello world\"}"
  }
  ```
  The `content` field is a **JSON-stringified string**, NOT a raw object. For text: `JSON.stringify({ text: "..." })`.

- Other msg_type values: `post`, `image`, `file`, `audio`, `media`, `sticker`, `interactive`, `share_chat`, `share_user`. Each has its own content schema (e.g., `image` content is `{"image_key": "img_xxx"}` stringified).

- For markdown: feishu doesn't have a `markdown` msg_type. The closest is `post` (rich text JSON) or wrap as an `interactive` card with a markdown element. **Plan's "Text/Markdown reply" should be re-scoped:** for true markdown, use `interactive` card with a `markdown` element. For plain text fallback, use `text` msg_type and strip markdown.

Confidence: **HIGH**.

---

## Q6: Attachment download

**Verdict: PLAN GUESS IS CORRECT.**

From `oapi-sdk-go` sample:
- **URL:** `GET /open-apis/im/v1/messages/:message_id/resources/:file_key?type={image|file}`
- **Auth:** `Authorization: Bearer <tenant_access_token>`
- **`type` query param is REQUIRED:** values are `image` or `file` (audio/video also map to `file`).
- **Response:** raw binary bytes (`Content-Type: application/octet-stream` or similar).

Example: `GET /open-apis/im/v1/messages/om_dc13264520392913993dd051dba21dcf/resources/file_456a92d6-c6ea-4de4-ac3f-7afcf44ac78g?type=image`

Confidence: **HIGH**.

---

## Q7: Inbound message event JSON schema

**Verdict: PLAN'S FIELD PATHS ARE STRUCTURALLY CORRECT.**

From WebSearch summaries of open.feishu.cn/document/server-docs/im-v1/message/events/receive:

```json
{
  "schema": "2.0",
  "header": {
    "event_id": "...",
    "event_type": "im.message.receive_v1",
    "create_time": "1735574400000",
    "token": "...",
    "app_id": "cli_xxx",
    "tenant_key": "..."
  },
  "event": {
    "sender": {
      "sender_id": {
        "union_id": "on_...",
        "user_id": "u_...",
        "open_id": "ou_..."
      },
      "sender_type": "user",
      "tenant_key": "..."
    },
    "message": {
      "message_id": "om_...",
      "root_id": "om_...",            // optional, threading
      "parent_id": "om_...",          // optional, threading
      "create_time": "1735574400000",
      "chat_id": "oc_...",
      "thread_id": "...",             // optional
      "chat_type": "p2p" | "group",
      "message_type": "text" | "post" | "image" | "file" | "audio" | "media" | "sticker" | "interactive",
      "content": "<JSON-stringified per type>",
      "mentions": [
        {
          "key": "@_user_1",
          "id": { "union_id": "...", "user_id": "...", "open_id": "..." },
          "name": "...",
          "tenant_key": "..."
        }
      ]
    }
  }
}
```

**Path confirmations:**
- `chat_type` is at `event.message.chat_type` — **plan's path is correct.**
- `chat_id` is at `event.message.chat_id` — correct.
- `sender_id.open_id` at `event.sender.sender_id.open_id` — correct.
- `message_type` at `event.message.message_type` — correct (plan says `message_type` good).

**Per-type `content` shape** (each is JSON-stringified, must be parsed twice):
- text: `{"text": "hello @_user_1"}` — note `@_user_1` placeholder, real name via `mentions` array.
- image: `{"image_key": "img_v2_xxx"}`
- file: `{"file_key": "file_v2_xxx", "file_name": "...", "file_size": 12345, "file_type": "..."}`
- audio: `{"file_key": "...", "duration": 1500}`
- media: `{"file_key": "...", "image_key": "img_v2_xxx" /*thumbnail*/, "file_name": "...", "duration": ...}`
- sticker: `{"file_key": "..."}`
- post: rich JSON (nested `title`+`content` arrays of paragraphs)
- interactive: `{"template_id": "...", ...}` or card JSON

For WS transport, the event JSON is wrapped in a protobuf frame's `payload` field (which itself may be gzip-compressed). After unwrap, you get the JSON above.

Confidence: **HIGH** for paths and outer schema. **MEDIUM** for exact `file` content fields — should verify against a real captured event when PR3 runs against a sandbox app.

---

## Q8: Errcodes

**Verdict: MIXED — auth errcodes confirmed, device-flow errcodes are WRONG, CardKit errcodes unconfirmed.**

| Code | Status | Meaning | Source |
|---|---|---|---|
| `0` | ✓ confirmed | success | universal |
| `99991661` | partial | invalid app_id / app_secret | plan |
| **`99991663`** | ✓ confirmed | invalid/expired tenant_access_token — refresh and retry | open.feishu.cn/document/faq/trouble-shooting/how-to-fix-99991663-error |
| **`99991668`** | likely correct | invalid user_access_token | search-confirmed but not seen in primary docs |
| `230002` | ⚠ unconfirmed | CardKit sequence error | Doc page exists at cardkit-v1/streaming-updates-openapi-overview but full errcode list not fetched |
| `230005` | ⚠ unconfirmed | CardKit card not found | same |
| **`1264003`** | ✗ WRONG | plan said "device-code Waiting" | NO such code. The real "pending" signal is HTTP 400 + `{"error": "authorization_pending"}` per RFC 8628 |
| **`1264004`** | ✗ WRONG | plan said "device-code Expired" | NO such code. Real signal: `{"error": "expired_token"}` |
| **`1264005`** | ✗ WRONG | plan said "device-code Fail" | NO such code. Real signals: `{"error": "access_denied"}` or `{"error": "slow_down"}` |

**Device-flow errors are STRINGS not numeric codes.** The plan's mapping must be rewritten to switch on `error` string:
- `authorization_pending` → keep polling at current interval (Transient)
- `slow_down` → increase polling interval by 5s and keep polling (Transient)
- `access_denied` → user explicitly denied; stop (Fatal)
- `expired_token` → device_code expired; restart begin flow (Fatal in current attempt, retryable as new flow)

CardKit and IM specific errcodes need a separate WebFetch pass (couldn't get the errcode reference page via WebSearch alone). Marked **UNCONFIRMED — manual verification needed** for the 230xxx codes; implementer can find them on https://open.feishu.cn/document/server-docs/getting-started/server-error-codes at impl time.

Confidence: **HIGH** for device-flow rewrite (source-code-confirmed). **MEDIUM** for 99991663/99991668. **LOW** for 230xxx (best guess; verify before merging PR5).

---

## Summary impact on plan

- **PR2 (device-code + tenant_access_token):** **NEEDS REWRITE.** The URLs, host, method (form-encoded not JSON), field names (`client_id`/`client_secret` not `app_id`/`app_secret` on the device-flow response), and error mapping (RFC 8628 strings not 1264xxx numeric codes) all differ from the spec. The token endpoint itself (`/open-apis/auth/v3/tenant_access_token/internal`) is correct.
- **PR3 (WebSocket):** **NEEDS SIGNIFICANT REWORK.** Handshake URL is `POST ${open-domain}/callback/ws/endpoint` (not derived from spec), request body uses CapCase keys `{AppID, AppSecret}`, response wraps URL inside `data.URL` with ClientConfig sibling. Frame format is **protobuf** (pbbp2), not JSON — this is the biggest scope addition. Plan's `BoxStream<ChannelMessage>` and `ReconnectBackoff` can stay, but inner frame decoder must be a new protobuf module. Estimate: +1–2 days of work over the original PR3 scope.
- **PR4 (send text):** **MOSTLY UNCHANGED.** URL, query param, body shape match. Plan must remember `content` is JSON-stringified. Markdown story needs separate decision (use `interactive` card with markdown element, or downgrade to plain text).
- **PR5 (CardKit):** **METHOD CHANGE — PATCH → PUT** for the streaming content update endpoint. Body shape (`uuid`/`content`/`sequence`) is correct. Errcode 230002/230005 still unconfirmed — verify at impl time.
- **PR6 (download):** **UNCHANGED.** URL guess is correct; remember `type` query param is REQUIRED.
- **PR7 (event JSON):** **UNCHANGED structurally**; just verify per-type `content` fixtures against a real captured event before locking test schemas.

## Confidence (overall)

| Q | Confidence |
|---|---|
| Q1 Device-code | HIGH (source-code-verified) |
| Q2 tenant_access_token | HIGH |
| Q3 WS endpoint | HIGH for URL; MEDIUM for proto-buf details |
| Q4 CardKit | HIGH for endpoints; MEDIUM for delivery wrapper |
| Q5 Send text | HIGH |
| Q6 Download | HIGH |
| Q7 Event schema | HIGH structurally; MEDIUM per-type content fixtures |
| Q8 Errcodes | HIGH for auth, HIGH that device-flow uses RFC strings, LOW for CardKit-specific 230xxx |

## Recommendation

**PR2 cannot proceed against the plan as written.** The device-flow URLs and field names in `2026-05-18-im-feishu-phase1.md` §1.2 must be replaced before any code is touched. PR3 also needs the WS handshake URL replaced and a protobuf decoder scoped in. Suggested next step: edit the plan file's Task 2 + Task 3 sections to match the verified shapes above, then resume implementation.
