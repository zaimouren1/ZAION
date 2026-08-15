# Zaion vs Hermes Latest-Source Recalibration

Report date: 2026-05-28

Primary Hermes reference: `D:/zaion-reference/hermes-agent-latest` at
`9c0807070388c4f612a827230f1314ebbf24e857`
(`2026-05-24 15:57:26 -0700`,
`test(cli): update resume usage-hint assertion for numbered selection`).

Historical reference only:
`D:/zaion-reference/zaion-rust-cleanup-20260501/hermes-agent-2026.4.8.zip`.
That archive remains useful for older stage evidence, but latest-Hermes parity
must be judged against the mirror above.

Continuation reference: prior thread
`019e53d7-e739-7c51-91e0-3f7d29a343dd` confirmed the same ruling: latest
Hermes comparison is `PARTIAL`, not `SURPASSED`.

## Verdict

Overall latest-Hermes comparison: `PARTIAL`.

Latest Telegram/channel slice: Zaion now implements and verifies captioned
photo dispatch metadata plus the first safe photo download/cache path for
Telegram. Live Telegram runtime configures a Zaion-managed media root, calls
Bot API `getFile` for the largest incoming photo, rejects unsafe returned
`file_path` values, downloads `/file/bot<TOKEN>/<file_path>`, and caches the
bytes through `MediaCacheManager`. Caption-only photo updates can use
`message.caption` as wake text, caption entities participate in bot mention
gating, and signed `telegram.delivery` evidence preserves
`telegram_caption`, `telegram_media_group_id`, `telegram_media_types`,
`telegram_media_file_ids`, `telegram_media_file_unique_ids`,
`telegram_photo_count`, `telegram_media_cached_paths`, and
`telegram_media_cached_mime_types`. This is now photo-cache parity for a
narrow slice, not full channel parity: Hermes remains ahead on album
debounce/merge, image-document/voice/video/document processing,
model-visible media consumption depth, sticker analysis, and outbound native
media delivery.

`TELEGRAM_REACTIONS=true` makes live polling set an in-progress reaction
before model/tool processing, swap it to a success or failure reaction after
reply delivery, and record concise reaction labels on signed
`telegram.delivery` events. A `Cancelled` processing outcome clears the
in-progress reaction through an empty `setMessageReaction` payload, and the
live `/stop` command clears locally registered in-progress reaction markers
while recording `telegram_reactions: ["cleared"]` on its signed command
delivery audit. It also requests cancellation through any registered active
wake `StreamCallback` cancel handle and records `cancel_requested` in that
command audit when such a handle is present. The runner slice moves live
Telegram wake execution off the receive loop and proves `/stop` can be handled
while a test-held active turn remains in flight. Cancelled completions now
treat a cancelled `StreamCallback` as an explicit `status: "cancelled"`
delivery, clear the in-progress reaction, and suppress stale assistant
replies. The newest guard-release slice records enough runner owner metadata
for `/stop` to synthesize signed `cancelled` completions for unfinished active
tasks, release the busy guard, drain the latest queued follow-up exactly once,
and drop late stale completions. Reactions remain disabled by default, and
this still uses cooperative cancellation rather than Hermes' owned async task
`cancel()`/bounded unwind path, so full mid-flight wake/model/tool
cancellation plus media batching/cache remain open. This is a partial slice
rather than full Hermes channel parity.

Zaion has real product and architecture progress, including a clearer launch
contract, Ed25519 principal identity, signed append-only proof chains,
provenance-aware runtime traces, neural/topology observability, TUI visible
reply isolation, minimum busy-input queue drain, local queued prompt
edit/delete UX, local TUI steer/interrupt busy-control semantics, a local TUI
gateway event-frame reducer, and a first stdio JSON-RPC transport slice for
gateway session bootstrap plus prompt/steer/interrupt routing.
Since the transport slice, Zaion also gained gateway approval/clarify response
controls, Telegram UTF-16 chunking plus outbound topic/reply routing and live
busy-guard groundwork, ACP protocol-event JSON-RPC notification helpers, and
dynamic MCP `mcp-<server>` toolset reporting. The latest wake/runtime slice
adds aggregate tool-result turn budgeting before model re-entry and
todo-aware compression split for current-turn active todos. These are useful
parity slices, not full macro-module completion. Wake now also persists
session todo state through signed `zaion.session_todo.state.v1` ledger events
and hydrates from them on later wake turns, including compression child
sessions. The durable todo lookup now uses a queryable ledger payload string
match on `thread_id`, so newer todo-state events from other threads cannot
shadow the current thread's older state. Wake durable todo writes also redact
obvious secrets and cap long todo title/content/notes strings before writing
state into Zaion's append-only ledger. The newest tool-runtime slice adds a
target-aware tool-result storage boundary, so per-result and aggregate spill
can write through an active environment target while injecting a model-visible
pointer to the environment-visible file. Wake native tool execution helpers
now thread the same budget config and storage target into per-result spill.
Default local live wake also stores oversized spill files under the current
workspace's `.zaion/tool-results`, making local native-tool output visible from
the same workspace boundary instead of hiding it under the host data dir.
Structured wake callers can also pass `tool_result_storage_root` explicitly,
which gives TUI/gateway/MCP integrations a concrete root to use when process
cwd is not the intended workspace. TUI local model turns now use that override
with a startup-captured workspace root, so TUI worker turns no longer rely on a
later cwd guess for local spill files. This is still narrower than Hermes'
full environment abstraction for Docker, SSH, Modal, gateway, MCP, and
delegated execution.
API runs, MCP HTTP wake route, webhook agent dispatch, ACP stdio wake route,
Telegram live polling, and `zaion tg simulate` now use the same structured
override path, so local structured wake turns spill oversized tool output under
`cwd/.zaion/tool-results` rather than depending on a service cwd guess. This
is another verified local structured-caller slice, not full Hermes environment
parity. Signed wake `tool.receipt` events now also carry concise
`tool_result_storage` metadata for persisted oversized outputs, including path,
storage root, byte counts, truncation state, and tool-call identity while
preserving the permission proof. The newest receipt/proof slice adds a
structured `tool_result_storage_binding` that ties persisted storage to
storage-root-derived environment identity, permission scope, principal/session
provenance, parent output event id, tool identity, argument/output hashes, and
turn material. Wake turn proofs now also carry signed tool receipt event ids
and include them in proof lineage. Wake now also appends a later signed
`tool.receipt.proof_join` event after `turn.proof`, giving consumers an
append-only forward edge from receipt ids to proof ids/hashes without mutating
old receipt events. The ledger now also has an exact array-membership payload
lookup, so signed join events can be found by `tool_receipt_ids` without a
consumer having to scan unrelated event types. The local CLI now exposes that
path through `zaion tool receipts` event ids and
`zaion tool receipt-trace <pid> <receipt-event-id>`, including normalized
turn-proof hash verification. `zaion turn trace` also reports receipt join
presence/proof/hash status for proofs that contain `tool_receipt_ids`, and
native MCP now exposes a compact `tool_receipt_trace` diagnostic tool for the
same local trace. This improves Zaion's provenance surface, but
delegated/remote execution coverage, gateway/MCP propagation, and real
non-local backend environment identities remain open.
Delegated proof records now also have a dedicated local trace path:
`zaion agent receipt-trace <pid> <delegation-proof-event-id>` resolves a
`delegation.proof` event, recomputes the deterministic `merge_receipt`, and
verifies the signed A2A delegation message. This is intentionally not modeled
as a generic `tool.receipt`; live delegated execution and gateway/ACP/MCP
propagation remain open.
ACP stdio wake JSON-RPC results, webhook synchronous wake `agent_trigger`
results, Telegram live delivery traces, `zaion tg simulate`, MCP HTTP wake
responses, and API `/v1/runs` wake responses now also propagate tool receipt
ids and signed `tool.receipt.proof_join` summaries when a turn executes tools.
These response payloads expose receipt count, receipt ids, join event id, join
summary, join presence, and proof-hash verification state while leaving direct
MCP HTTP tool calls correctly scoped as `receipt_only`. `tg simulate --no-llm`
also writes explicit empty/default receipt/proof fields for no-tool local
delivery. This extends the local proof surface into the verified local
service/channel wake consumers, but delegated execution, remote sandbox paths,
broader gateway/channel adapters, and real non-local backend environment
identities remain open.
Persisted tool-result metadata and the signed wake receipt binding can now
also preserve an explicit backend environment identity/kind supplied by
`WakeRequest` or a `ToolResultStorageTarget`. Local/default targets still fall
back to the deterministic `storage-root:<hash>` identity, so old local behavior
remains stable while future Modal/Docker/Daytona/SSH callers can bind real
backend ids without custom receipt code. This is a binding-contract slice, not
remote sandbox parity.
The local service/channel wake response set now also returns persisted
tool-result storage receipt summaries. MCP HTTP wake responses, API `/v1/runs`
wake responses, ACP stdio wake results, webhook synchronous wake
`agent_trigger` results, and Telegram delivery payloads expose
`tool_result_storage_receipts` plus `tool_result_storage_receipt_count` beside
the existing receipt/proof join summary. No-storage local turns return stable
empty arrays/count `0`, and ACP stdio protocol coverage proves a non-empty
mock storage receipt can carry backend/environment binding metadata through
JSON. True large-output local wake E2E now also covers MCP HTTP wake, API
`/v1/runs`, webhook synchronous `agent_trigger`, ACP stdio wake, and `zaion tg
simulate` delivery: each path executes a native `fs_search` call large enough
to persist tool output, returns a non-empty `tool_result_storage_receipts`
array/count `1`, and verifies the stored output file exists under
workspace-visible `.zaion/tool-results`. The Telegram simulate regression also
pins the visible `tool_storage_count     : 1` trace and the
`telegram.delivery` ledger payload. Telegram live polling now has a one-poll
fake API E2E that exercises real `TelegramAdapter.receive(...)` through the
same wake/native-tool path and verifies
`telegram.delivery.tool_result_storage_receipt_count == 1`. The latest
Telegram slice also binds delivery proof traces to the current received
message's source hash, preserves real chat/update/message/topic/reply metadata,
denies supergroup noise from live adapter metadata without typing/reply sends,
and carries `resolved_addrs` through API runtime delivery JSON. Live Telegram
command-graph quick replies now also write explicit `telegram.delivery`
diagnostics labelled `runtime = "telegram.command_graph"` and record stale
topic reply-anchor fallback reports. Those delivery events set
`parent_event_id` to the command receipt and expose `command_receipt_event_id`
without pretending command replies are wake turns. Normal wake dispatch now
also recomputes the Telegram source hash after group-mention prompt stripping,
so the canonical wake envelope uses the same prompt/hash that enters the wake
runtime. Fake-API coverage records the same stale topic reply-anchor fallback
for a normal wake reply while keeping `runtime = "phase8b.unified_wake"`.
Normal live wake replies now also use Telegram `MarkdownV2`, and fake-API
coverage proves Markdown entity parse failures retry as plain text while
recording `markdown_v2_plain_text_retry` plus the successful Telegram message
id in `telegram.delivery.delivery_report`. Command-graph quick replies now
also request MarkdownV2 and have fake-API coverage for the same parse-error
plain-text retry path while preserving `runtime = "telegram.command_graph"`
and the command receipt parent edge. Access-gate denial replies now have the
same MarkdownV2 parse-error fallback coverage recorded in
`telegram.denied.delivery_report` while staying separate from normal
`telegram.delivery` wake events. Denied/noise events now also keep real
Telegram chat/topic/update/message/reply metadata, giving group-policy
debugging a concrete signed audit trail instead of only a reason string.
Live Telegram group/supergroup dispatch now also supports a verified
allowed-chat/topic gate through `ZAION_TELEGRAM_ALLOWED_CHATS` and
`ZAION_TELEGRAM_ALLOWED_TOPICS`, matching the latest Hermes `allowed_chats` /
`allowed_topics` behavior for this narrow slice. A fake-API live poll proves a
direct bot mention in an allowlisted group but disallowed topic is silently
denied as `telegram_topic_not_allowed`, preserves real chat/topic metadata,
sends no typing/reply request, and does not append `telegram.delivery`.
That policy gate is now also durable: Telegram `ChannelProfile` entries can
persist `allowed_chats` and `allowed_topics` in `channels.toml`, `zaion tg
  setup --token ... --allowed-chats ... --allowed-topics ...` writes them,
  `TelegramAccessPolicy::from_store` merges durable policy with env overrides,
  and `zaion tg doctor` prints the effective chat/topic gate. Zaion now also
  persists Telegram `guest_mode`, exposes `zaion tg setup --guest-mode true`,
  reports it in `zaion tg doctor`, and lets a non-allowlisted group dispatch
  only when the current bot is directly addressed with an explicit `@bot`
  mention; ordinary group replies still do not bypass the allowlist.
  A live fake-API poll now proves that durable guest-mode path through the real
  Telegram adapter: `getUpdates` receives a non-allowlisted supergroup
  `@zaion_bot` mention, the wake runtime strips the bot mention, sends typing
  and reply requests, and appends signed `telegram.delivery`. Delivery events
  now copy the same real Telegram chat/topic/update/message/reply metadata as
  denial events, and a companion live poll proves ordinary group replies
  outside the allowlist still deny silently as `telegram_group_not_allowed`
  without typing, reply, or `telegram.delivery`.
This is still a local response-contract slice; richer live Telegram semantics,
delegated execution, remote sandbox paths, and broader gateway/channel
adapters remain open.

Zaion cannot yet claim full Hermes-wide parity. Latest Hermes still leads in
TUI runtime depth, live gateway/channel behavior, MCP/ACP/tool breadth,
profile/session/context polish, and batch/environment maturity.

Status labels in this report are strict:

| Label | Meaning |
| --- | --- |
| `SURPASSED` | Implemented in Zaion, source-evidenced, locally verified, and stronger than latest Hermes for the same product slice. |
| `PARTIAL` | Present in Zaion but narrower, less polished, less verified, or not yet equivalent to latest Hermes. |
| `OPEN` | Missing, not wired into a user path, or not yet source-verified against latest Hermes. |

## 2026-05-23 Verified Slice Update

Additional 2026-05-28 Telegram observation-only group memory slice:

- Latest Hermes `gateway/platforms/telegram.py` exposes
  `_telegram_observe_unmentioned_group_messages()`,
  `_should_observe_unmentioned_group_message(...)`, and
  `_observe_unmentioned_group_message(...)`.
- Hermes accepts `observe_unmentioned_group_messages`, legacy
  `ingest_unmentioned_group_messages`, and
  `TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES`; observation remains
  group/supergroup scoped and respects allowlists, allowed topics, ignored
  threads, other-bot mentions, free-response chats, replies, direct mentions,
  and mention-pattern dispatch.
- Zaion now persists optional Telegram `observe_unmentioned_group_messages` on
  `ChannelProfile` entries in `channels.toml` with serde defaults for existing
  stores.
- `zaion tg setup --token ... --observe-unmentioned-group-messages true`
  writes the durable policy; `--ingest-unmentioned-group-messages` remains a
  compatibility alias, and `zaion tg doctor` reports the effective flag.
- `TelegramAccessPolicy::from_store` reads durable policy, env
  `ZAION_TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES`, and legacy env
  `ZAION_TELEGRAM_INGEST_UNMENTIONED_GROUP_MESSAGES`.
- Plain group/supergroup text can become `ObserveOnly` only after hard gates
  and dispatch triggers, and only for explicitly allowlisted group chats.
- A fake-API live poll proves observation writes signed `telegram.observed`
  with source hash, shared group thread id, attributed content, and Telegram
  metadata while sending no typing/reply and no `telegram.denied` or
  `telegram.delivery`.
- Verification covered the red/green env policy regression, grouped Telegram
  policy tests, live Telegram regressions, CLI setup persistence, and
  formatting.
- Overall latest-Hermes comparison remains `PARTIAL`; media/reactions,
  delegated/remote propagation, and multi-channel equivalents remain open.

Additional 2026-05-28 Telegram mention-patterns slice:

- Latest Hermes `gateway/platforms/telegram.py` exposes
  `_compile_mention_patterns()` and `_message_matches_mention_patterns(...)`,
  compiles regexes case-insensitively, skips invalid regexes, and applies
  allowed chat/topic, ignored-thread, and explicit other-bot gates before regex
  wake matching.
- Zaion now persists optional Telegram `mention_patterns` on `ChannelProfile`
  entries in `channels.toml` with serde defaults for existing stores.
- `zaion tg setup --token ... --mention-patterns ...` writes the durable policy
  and `zaion tg doctor` reports the effective list.
- `TelegramAccessPolicy::from_store` reads durable mention patterns and merges
  them with `ZAION_TELEGRAM_MENTION_PATTERNS`, deduping values.
- Plain group/supergroup text matching a configured case-insensitive regex can
  dispatch without a direct `@bot` mention and keeps the prompt unchanged.
- Focused regressions prove disallowed group chats, disallowed topics, ignored
  topics, and explicit other-bot mentions still deny before regex dispatch.
- A fake-API live poll now proves regex-matched plain group text sends typing
  and reply requests, writes signed `telegram.delivery` with real chat/topic
  metadata, and avoids `telegram.denied`.
- Verification covered the red/green policy regression, CLI setup persistence,
  the focused live poll evidence, grouped Telegram policy tests, live Telegram
  regressions, and formatting.
- Overall latest-Hermes comparison remains `PARTIAL`; observation-only group
  memory, media/reactions, broader propagation, and multi-channel equivalents
  remain open.

Additional 2026-05-28 Telegram free-response chats slice:

- Latest Hermes `gateway/platforms/telegram.py` exposes
  `_telegram_free_response_chats()` and applies the free-response check only
  after allowed topic, ignored thread, exclusive other-bot, guest-mode, and
  allowed-chat gates.
- Zaion now persists optional Telegram `free_response_chats` on
  `ChannelProfile` entries in `channels.toml` with serde defaults for existing
  stores.
- `zaion tg setup --token ... --free-response-chats ...` writes the durable
  policy and `zaion tg doctor` reports the effective list.
- `TelegramAccessPolicy::from_store` reads durable free-response chats and
  merges them with `ZAION_TELEGRAM_FREE_RESPONSE_CHATS`, deduping values.
- Plain group/supergroup text in an approved free-response chat dispatches
  without a direct `@bot` mention and keeps the prompt unchanged.
- Focused regressions prove disallowed group chats and ignored topics still
  deny before free-response dispatch.
- A fake-API live poll proves plain group text in a durable free-response chat
  sends typing/reply requests, writes signed `telegram.delivery` with real
  chat/topic metadata, and does not append `telegram.denied`.
- Verification covered the red/green policy regression, the hard-gate
  regression, the focused live poll regression, durable policy load, CLI setup
  persistence, and formatting.
- Overall latest-Hermes comparison remains `PARTIAL`; configurable mention
  patterns, observation-only group memory, media/reactions, broader
  propagation, and multi-channel equivalents remain open.

Additional 2026-05-28 Telegram allowed chat/topic gate slice:

- Latest Hermes `gateway/platforms/telegram.py` exposes
  `_telegram_allowed_chats()` and `_telegram_allowed_topics()`, applies
  `allowed_topics` before other group trigger checks, and treats a missing
  group topic id as General topic `1`.
- Zaion now reads `ZAION_TELEGRAM_ALLOWED_CHATS` and
  `ZAION_TELEGRAM_ALLOWED_TOPICS` into live Telegram access policy.
- Group/supergroup messages outside the allowed chat set are silently denied
  with `reason = "telegram_group_not_allowed"`.
- Group/supergroup messages outside the allowed topic set are silently denied
  with `reason = "telegram_topic_not_allowed"`.
- A fake-API live poll proves an explicit bot mention in an allowlisted group
  but disallowed topic writes `telegram.denied`, preserves
  `telegram_chat_id` and `message_thread_id`, sends no typing/reply request,
  and does not append `telegram.delivery`.
- Verification covered the red/green policy regression, the focused live poll
  regression, `telegram_live_` at 14 tests, `telegram_group_` at 11 tests, and
  `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; durable config,
  `group_allowed_chats`, guest mode, observation-only group memory,
  `free_response_chats`, ignored threads, media/reactions, and broader
  propagation remain open.

Additional 2026-05-28 Telegram durable chat/topic policy config slice:

- Zaion now persists Telegram `allowed_chats` and `allowed_topics` on
  `ChannelProfile` entries in `channels.toml` with serde defaults for existing
  stores.
- `zaion tg setup --token ... --allowed-chats ... --allowed-topics ...` writes
  those durable policy fields and reports the saved effective values.
- `TelegramAccessPolicy::from_store` reads durable policy and merges it with
  `ZAION_TELEGRAM_ALLOWED_CHATS` / `ZAION_TELEGRAM_ALLOWED_TOPICS`, deduping
  values.
- `zaion tg doctor` prints the effective allowed chat/topic lists.
- Verification covered the red/green store policy regression, the CLI setup
  persistence regression, `telegram_live_` at 14 tests, `telegram_group_` at
  11 tests, and `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; Hermes-style
  `group_allowed_chats`, guest/free-response/observation policy breadth,
  ignored threads, configurable mention patterns, media/reactions, and broader
  propagation remain open.

Additional 2026-05-28 Telegram guest-mode direct mention bypass slice:

- Latest Hermes `gateway/platforms/telegram.py` exposes
  `_telegram_guest_mode()` and `_is_guest_mention(...)`; in
  `_should_process_message(...)`, guest mode bypasses `allowed_chats` only for
  explicit bot mentions.
- Zaion now persists optional Telegram `guest_mode` on `ChannelProfile` with
  serde defaults for existing `channels.toml` files.
- `zaion tg setup --token ... --guest-mode true` writes the durable policy and
  `zaion tg doctor` reports the effective value.
- `TelegramAccessPolicy::from_store` reads durable `guest_mode`.
- A focused dispatch regression proves direct `@zaion_bot` messages outside
  `allowed_chats` dispatch when guest mode is true.
- A companion regression proves ordinary group replies outside `allowed_chats`
  remain denied as `telegram_group_not_allowed`.
- Verification covered the red/green guest-mode regression, durable policy
  load, CLI setup persistence, and the interactive gateway setup regression.
- Overall latest-Hermes comparison remains `PARTIAL`; live fake-API guest-mode
  proof, `free_response_chats`, ignored threads, observation-only group memory,
  configurable mention patterns, media/reactions, and broader propagation
  remain open.

Additional 2026-05-27 Telegram denied metadata audit slice:

- Live Telegram `telegram.denied` events now copy inbound Telegram metadata
  when available.
- A fake-API live poll proves a supergroup message without a bot trigger is
  denied silently while preserving `telegram_chat_id`, `telegram_chat_type`,
  `telegram_message_id`, `telegram_update_id`, `message_thread_id`,
  `telegram_message_thread_id`, `telegram_reply_to_message_id`, and
  `telegram_reply_to_text`.
- The denial remains separate from `telegram.delivery` and wake `turn.proof`.
- Verification covered the focused red/green regression, the full
  `telegram_live_` filter at 13 tests, and
  `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; group chat allowlists,
  allowed topics, guest-mode mention bypass, configurable mention patterns,
  observation-only group memory, media/reactions, and broader propagation
  remain open.

Additional 2026-05-27 Telegram access-gate Markdown parse fallback slice:

- Live Telegram access-gate denial replies now request Telegram `MarkdownV2`
  formatting through the existing adapter path.
- A fake-API live poll proves the first denial MarkdownV2 send can fail with
  Telegram's entity parse error, then retry without `parse_mode` as plain
  text.
- The retry restores the original visible denial text after MarkdownV2
  unescaping.
- `telegram.denied.delivery_report` records
  `parse_mode = "MarkdownV2"`,
  `fallbacks = ["markdown_v2_plain_text_retry"]`, and successful Telegram
  message id `884`.
- The denial remains an access-gate event with
  `reason = "sender_not_in_telegram_allowlist"` and does not append
  `telegram.delivery` or fabricate `turn.proof`.
- Verification covered the focused red/green regression, the full
  `telegram_live_` filter now at 13 tests, `zaion-adapters telegram_` at 18
  tests, and `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; richer channel policy,
  delegated execution, remote sandbox paths, and broader gateway/channel
  propagation remain open.

Additional 2026-05-27 Telegram command Markdown parse fallback slice:

- Live Telegram slash-command quick replies handled by `TelegramCommandGraph`
  now request Telegram `MarkdownV2` formatting through the existing adapter
  path.
- A fake-API live poll proves the first command MarkdownV2 send can fail with
  Telegram's entity parse error, then retry without `parse_mode` as plain
  text.
- The retry restores the original visible command reply text after MarkdownV2
  unescaping.
- `telegram.delivery.delivery_report` records
  `parse_mode = "MarkdownV2"`,
  `fallbacks = ["markdown_v2_plain_text_retry"]`, and successful Telegram
  message id `883`.
- The command delivery remains labelled
  `runtime = "telegram.command_graph"` / `status = "command_sent"` and keeps
  the command receipt parent edge without fabricating `turn.proof`.
- Verification covered the focused red/green regression, the full
  `telegram_live_` filter now at 12 tests, `zaion-adapters telegram_` at 18
  tests, and `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; media/reaction retry
  breadth, delegated execution, remote sandbox paths, and broader
  gateway/channel propagation remain open.

Additional 2026-05-27 Telegram wake Markdown parse fallback slice:

- Normal live Telegram wake replies now request Telegram `MarkdownV2`
  formatting through the existing adapter path.
- A fake-API live poll proves the first MarkdownV2 send can fail with
  Telegram's entity parse error, then retry without `parse_mode` as plain
  text.
- The retry restores the original visible reply text after MarkdownV2
  unescaping.
- `telegram.delivery.delivery_report` records
  `parse_mode = "MarkdownV2"`,
  `fallbacks = ["markdown_v2_plain_text_retry"]`, and successful Telegram
  message id `882`.
- Verification covered the focused red/green regression, the full
  `telegram_live_` filter now at 11 tests, `zaion-adapters telegram_` at 18
  tests, and `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; command/media/reaction
  retry breadth, delegated execution, remote sandbox paths, and broader
  gateway/channel propagation remain open.

Additional 2026-05-27 Telegram wake mention source-hash and fallback slice:

- Live Telegram group mentions recompute `source_hash` after dispatch strips
  the bot mention and settles the actual wake prompt.
- Canonical wake envelopes use the same stripped prompt and matching
  `source_hash`, avoiding raw-message hash mismatch after `@zaion_bot`
  removal.
- Denied/noise paths still use the original raw-message source hash.
- A fake-API live poll covers stale topic reply-anchor fallback for a normal
  wake reply: the first send with topic/reply metadata fails, the retry
  without the stale anchor succeeds, and the delivery report records
  `thread_reply_anchor_retry` plus successful Telegram message id `881`.
- Verification covered the focused wake fallback regression and the full
  `telegram_live_` filter, now 10 tests.
- Overall latest-Hermes comparison remains `PARTIAL`; richer Telegram/channel
  semantics and broader gateway/delegated/remote propagation remain open.

Additional 2026-05-27 Telegram command-graph delivery fallback slice:

- Live Telegram slash-command replies handled by `TelegramCommandGraph` now
  append `telegram.delivery` diagnostics beside the command receipt.
- Command delivery payloads are labelled with
  `runtime = "telegram.command_graph"` and `status = "command_sent"` or
  `command_send_failed`, while normal wake deliveries keep
  `phase8b.unified_wake`.
- Command replies remain non-turn receipts and do not fabricate a `turn.proof`.
- Command delivery events set `parent_event_id` to the command receipt and
  include `command_receipt_event_id`.
- A fake-API live poll covers stale topic reply-anchor fallback: the first
  send with topic/reply metadata fails, the retry without the stale anchor
  succeeds, and the delivery report records `thread_reply_anchor_retry` plus
  the successful Telegram message id.
- Verification covered the focused stale-topic fallback regression, the full
  `telegram_live_` filter, and `cargo fmt -p zaion-cli --check`.
- Overall latest-Hermes comparison remains `PARTIAL`; richer Telegram/channel
  semantics and broader gateway/delegated/remote propagation remain open.

Additional 2026-05-26 delegation receipt trace slice:

- `zaion agent receipt-trace <pid> <delegation-proof-event-id>` now resolves a
  signed `delegation.proof` event.
- The command recomputes `merge_receipt` from principal, delegate, task, scope,
  input hash, and output hash.
- The command verifies the stored A2A delegation message signature.
- The Phase 8 surface regression exercises
  `agent proof -> agent receipts -> agent receipt-trace`.
- Overall latest-Hermes comparison remains `PARTIAL`; live delegated
  execution, gateway, ACP, and MCP propagation still need equivalent proof
  traceability.

Additional 2026-05-26 service wake receipt/proof propagation slice:

- MCP HTTP `runtime_route=wake` responses now expose `tool_receipt_ids`,
  `tool_receipt_count`, `tool_receipt_proof_join_event_id`,
  `tool_receipt_proof_join`, `tool_receipt_join_found`, and
  `tool_receipt_proof_hash_verified`.
- API `/v1/runs` wake responses now expose the same receipt/proof join summary
  for tool-using turns.
- ACP stdio wake JSON-RPC results now expose the same receipt ids/count and
  signed proof-join summary.
- Webhook synchronous wake `agent_trigger` results now expose the same receipt
  ids/count and signed proof-join summary.
- Telegram live delivery traces and `zaion tg simulate` now expose the same
  receipt/proof summary.
- `tg simulate --no-llm` writes explicit empty/default receipt/proof fields.
- `crates/zaion-cli/src/commands/receipt_join.rs` is the shared lookup helper
  reused by ACP, webhook, MCP/API, and Telegram response builders.
- MCP HTTP and API run response builders now use that shared helper instead of
  private duplicate proof-join lookup/summary implementations.
- Populated service/channel proof extractors decode `TurnProof`, locate the signed
  `tool.receipt.proof_join` by exact `tool_receipt_ids` membership, and verify
  that the join points back to the returned `turn.proof` event/hash.
- Direct MCP HTTP tool calls remain `receipt_only` and do not fabricate a turn
  proof outside wake.
- Overall latest-Hermes comparison remains `PARTIAL`; delegated execution,
  remote sandbox paths, and broader gateway/channel adapters still need
  equivalent response propagation.

Additional 2026-05-26 explicit tool-result environment identity slice:

- `ToolResultStorageTarget` now exposes optional `environment_id()` and
  `environment_kind()` methods.
- `ToolResultMetadata` records optional environment identity/kind for persisted
  oversized tool outputs.
- `HostToolResultStorageTarget::with_environment(...)` creates a host-backed
  storage target with named backend identity.
- `WakeRequest` now carries optional `tool_result_environment_id` and
  `tool_result_environment_kind`.
- Wake uses `wake_tool_result_storage_target(...)` so structured callers can
  bind explicit backend identity into the host storage target.
- Wake receipt `tool_result_storage_binding.environment` prefers explicit
  metadata identity/kind and falls back to `storage-root:<hash>` /
  `storage_target` for local/default targets.
- Verification covered runtime metadata propagation, wake request target
  construction, signed receipt binding preference, existing storage metadata
  behavior, and active environment spill.
- Overall latest-Hermes comparison remains `PARTIAL`; real remote backend
  selectors, delegated execution, and broader gateway/channel propagation still
  need this identity path wired end to end.

Additional 2026-05-26 service wake storage receipt summary slice:

- `receipt_join.rs` now exposes `tool_result_storage_receipts(...)`, a shared
  helper for summarizing returned `tool.receipt` events with non-null
  `tool_result_storage`.
- MCP HTTP wake responses, API `/v1/runs` wake responses, ACP stdio wake
  results, webhook synchronous wake `agent_trigger` results, and Telegram
  delivery payloads expose `tool_result_storage_receipts` and
  `tool_result_storage_receipt_count`.
- No-storage local tool turns and `tg simulate --no-llm` expose stable empty
  arrays/count `0`.
- ACP stdio injected-runtime coverage includes a non-empty mock storage
  receipt with backend/environment binding metadata.
- Verification covered the helper, MCP/API/Webhook/ACP/Telegram response
  paths, the adapter protocol shape, the source gate, formatting, and later
  true large-output local service E2E for MCP/API/webhook/ACP plus `tg
  simulate`.
- Overall latest-Hermes comparison remains `PARTIAL`; delegated execution,
  remote sandbox paths, broader gateway/channel adapters, and richer live
  Telegram behavior remain open.

Additional 2026-05-27 Telegram live polling storage receipt E2E slice:

- A one-poll fake Telegram API regression exercises real
  `TelegramAdapter.receive(...) -> process_live_telegram_message_once(...) ->
  cmd_wake_with_request(...) -> native fs_search large output -> persisted
  storage receipt summary`.
- The test verifies the resulting `telegram.delivery` ledger event carries
  `tool_result_storage_receipt_count == 1`.
- The production live loop remains a forever-polling daemon path; the one-poll
  helper and Telegram API base URL override are test-only.
- Overall latest-Hermes comparison remains `PARTIAL`; bot mention trigger
  context, allowlist/group nuances, batching, media, Markdown/reactions,
  retry behavior, topic fallback, delegated execution, remote sandbox paths,
  and broader gateway adapters remain open.

Additional 2026-05-27 Telegram proof binding and real update metadata slice:

- Telegram delivery proof traces are now source-bound: candidate Telegram
  `turn.proof` events are decoded and their `user_event_id` must point back to
  a `channel.received` event with the current message's `source_hash`.
- A same-thread failure regression proves `wake_failed` Telegram delivery no
  longer inherits a stale prior proof id, tool receipt ids, or storage receipt
  count from a previous successful turn.
- `TelegramAdapter.receive(...)` now preserves real update metadata for chat
  type, Telegram chat/update/message ids, topic/thread id, and reply-to id/text.
- A live fake-API poll verifies `supergroup` noise is denied from real adapter
  metadata without sending typing or reply calls.
- API runtime webhook delivery JSON now preserves `resolved_addrs` so gateway
  consumers can inspect concrete resolved delivery targets.
- Overall latest-Hermes comparison remains `PARTIAL`; richer Telegram mention
  behavior, batching, media, Markdown/reactions, retry semantics, topic/reply
  fallback, delegated execution, remote sandbox paths, and broader gateway
  adapters remain open.

Additional 2026-05-25 wake receipt/proof join slice:

- Added `EventType::ToolReceiptProofJoin` with wire string
  `tool.receipt.proof_join`.
- Wake writes a signed `tool.receipt.proof_join` event after `turn.proof` when
  signed receipt ids exist, parented to the proof event.
- Join payloads record receipt ids/count, proof event id/hash,
  answer/output/user event ids, lineage, and `join_hash`.
- `EventLedger::list_events_by_payload_string_array_contains(...)` can now
  find signed join events by exact `tool_receipt_ids` array membership,
  newest-first, without relying on SQLite JSON1.
- `zaion tool receipts <pid>` now exposes local receipt event ids, and
  `zaion tool receipt-trace <pid> <receipt-event-id>` follows
  receipt -> `tool.receipt.proof_join` -> `turn.proof` while verifying the
  normalized turn-proof hash.
- `zaion turn trace <proof-event-id> --pid <pid>` reports receipt join
  presence, join-to-proof linkage, and join/proof hash match.
- Native MCP registers `tool_receipt_trace`, a compact diagnostic lookup for
  local receipt -> join -> proof hash verification.
- Turns without tool receipts skip the join event.
- Overall latest-Hermes comparison remains `PARTIAL`; delegated, remote
  sandbox, gateway, and MCP execution paths still need equivalent receipt/proof
  binding.

Additional 2026-05-25 structured-wake caller slice:

- Wake exposes `workspace_tool_result_storage_root()`, the shared local default
  for workspace-visible tool-result spill.
- API runs, MCP HTTP wake route, webhook agent dispatch, ACP stdio wake route,
  Telegram live polling, and `zaion tg simulate` now attach that root
  explicitly to structured `WakeRequest` values.
- RED/GREEN/source-gate coverage proves the structured callers use the shared
  canonical-envelope helper path and fail when the helper/root proof drifts.
- Overall latest-Hermes comparison remains `PARTIAL`; delegated execution,
  remote sandbox, and non-local environment-root plumbing remain open.

Additional 2026-05-25 wake receipt metadata slice:

- `ToolExecutionRecord` now retains optional `ToolResultMetadata` from
  per-result and aggregate tool-result budgeting.
- Successful wake todo/native/MCP tool paths carry that metadata into signed
  `tool.receipt` ledger events.
- Persisted outputs now produce a compact `tool_result_storage` receipt object
  with schema, tool name, tool call id, stored/truncated flags, byte counts,
  persisted path, and storage root.
- The receipt intentionally omits the full preview to avoid re-inflating the
  signed ledger payload.

Latest live-runtime slices:

- Wake now applies aggregate tool-result turn budgeting after each native/MCP/
  todo tool batch returns and before the batch is pushed back into provider
  context as `ChatMessage::tool_result(...)`.
- `CompressionSplitter` now exposes a todo-aware compression split method, and
  wake uses it with the current session-local `TodoStore`.
- Current-turn active todos can therefore be protected in compressed child
- Successful wake `todo` calls now persist a full-store durable snapshot as a
  signed `zaion.session_todo.state.v1` event after `channel.sent`, and later
  wake turns hydrate from the latest matching event before history fallback.
- Compression session splits snapshot todo state into the child namespace even
  when the current turn did not execute a fresh `todo` tool call.
- `EventLedger::list_events_by_payload_string(...)` now supports newest-first
  exact string payload lookup without depending on SQLite JSON1; wake uses it
  for thread-scoped durable todo hydration.
- Wake durable todo-state writes now sanitize `state_json` before ledger
  append: title/content fields are redacted and capped at 512 characters,
  notes fields are redacted and capped at 2048 characters, and structured
  `state` plus `state_hash` are derived from the same sanitized JSON.
- Runtime now exposes target-aware tool-result storage APIs:
  `ToolResultStorageTarget`, `HostToolResultStorageTarget`,
  `maybe_store_tool_result_with_target(...)`, and
  `enforce_turn_budget_with_target(...)`.
- Wake helper coverage proves oversized tool output can spill through a fake
  active environment target for both per-result and aggregate turn-budget
  paths, with no host fallback file written in those target-backed cases.
- Wake native tool execution helpers now accept a shared
  `ToolResultBudgetConfig` plus `ToolResultStorageTarget`, and a regression
  test proves successful native tool output can spill through an
  active-environment target before provider re-entry.
- Default local live wake now resolves its budget storage root to
  `cwd/.zaion/tool-results`; regression coverage proves the old
  `data_dir()/tool-results` host-hidden default is no longer used for local
  live wake spills.
- `WakeRequest::tool_result_storage_root` plus
  `with_tool_result_storage_root(...)` let structured callers override that
  root before live wake constructs the storage target.
- TUI local model-turn requests now pass a startup-captured
  `workspace_root/.zaion/tool-results` storage root through that structured
  override.
- Remaining todo gaps: gateway/channel hydration parity and richer sealed
  storage for full oversized content if future workflows require it.
- Remaining tool-result gaps: non-local sandbox targets, gateway/MCP/delegated
  environment selection, other service-launched project-root detection,
  explicit environment identity, and cross-path receipt/proof joins.

Verification evidence:

- `cargo test -p zaion-runtime tool_result_storage -- --nocapture`: 8 passed.
- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: failed first on missing `tool_result_storage`, then passed.
- `cargo test -p zaion-cli wake_tool_context -- --nocapture`: 4 passed.
- `cargo test -p zaion-cli wake_todo_state_event_redacts_and_caps_durable_strings_before_ledger_write -- --nocapture`: failed first on the old unsanitized ledger write, then passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed after
  `wake_live_tool_result_budget_defaults_to_workspace_visible_dir` first failed on the
  old host data-dir default.
- `cargo test -p zaion-cli wake_request_tool_result_storage_root_overrides_default_budget_root -- --nocapture`:
  failed first on the missing structured override API, then passed.
- `cargo test -p zaion-cli tui_model_turn_request_ -- --nocapture`: 2 passed
  after failing first on missing TUI request-root plumbing.
- `cargo test -p zaion-cli structured_wake_request_from_envelope_defaults_to_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli api_run_structured_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_runtime_route_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli doctor_source_gate_locks_acp_canonical_envelope_ingress -- --nocapture`: failed in RED on the stale ACP source gate, then passed.
- `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix -- --nocapture`: passed.
- `cargo test -p zaion-ledger test_list_events_by_payload_string_returns_latest_exact_matches -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_parser_tool_call_records_permission_receipt -- --nocapture`: passed with local `tool receipts` -> `tool receipt-trace` -> `tool verify` coverage.
- `cargo test -p zaion-mcp tool_receipt_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli chat_executes_native_tool_call_without_mcp -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_todo_state_hydration_is_not_shadowed_by_newer_other_threads -- --nocapture`: passed after failing first on the old bounded-window implementation.
- `cargo test -p zaion-cli wake_tool_context_batch_enforces_aggregate_turn_budget_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_context_output_spills_large_results_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.
- `cargo test -p zaion-ledger -- --nocapture`: 29 passed.
- `cargo test -p zaion-runtime compression_split_reinjects_active_todos_before_child_branch -- --nocapture`: passed.
- `cargo fmt -p zaion-runtime -p zaion-cli --check`: passed.
- `cargo fmt -p zaion-cli -p zaion-ledger --check`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.
- `cargo check -p zaion-ledger`: passed.

Additional verified slices after review hardening:

- ACP protocol events now route through a sink abstraction, and ACP session
  lifecycle calls reject unsafe or cross-principal load/resume/fork access.
- MCP `refresh_server_tools()` now preserves prior tools if rediscovery fails,
  replacing only after a successful `tools/list` refresh.
- Telegram group dispatch now treats bare slash commands and commands for other
  bots as noise unless the command is explicitly targeted to Zaion; the busy
  guard also releases active state after post-begin envelope rejection.
- TUI `/gateway-close` now detaches local gateway transport state after sending
  `session.close`, so later prompts do not wait forever on a closed session.

Verification evidence:

- `cargo test -p zaion-cli gateway_close -- --nocapture`: 5 passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 23 matching tests passed.
- `cargo test -p zaion-runtime mcp -- --nocapture`: 26 passed.
- `cargo test -p zaion-a2a acp -- --nocapture`: 11 passed, 0 failed, 14
  filtered out.

Latest verified Zaion slices:

- TUI `/approve`, `/deny`, and `/clarify` now respond to pending gateway
  approval/clarify frames through `approval.respond` and `clarify.respond`.
- Telegram adapter chunking now respects Telegram's 4096 UTF-16-unit limit.
- Telegram outbound send bodies now preserve `message_thread_id` /
  `reply_to_message_id` metadata, including General topic fallback and
  chunked-send behavior.
- Telegram live loop has a per-thread busy guard with one replaceable pending
  ordinary message slot.
- ACP now has `zaion.acp.event.v1` protocol event DTOs, initialize
  advertisement, and stdio `protocol/event` JSON-RPC notification helpers.
- MCP reporting now exposes dynamic `mcp-<server>` toolsets, raw server aliases,
  and `tools.dynamic_mcp_toolsets` in capability JSON.

Verification evidence:

- `cargo test -p zaion-cli gateway -- --nocapture`: 34 gateway-filtered tests
  passed plus matching filtered integration/stable tests.
- `cargo test -p zaion-cli tui -- --nocapture`: 52 passed.
- `cargo test -p zaion-cli telegram_busy_guard -- --nocapture`: 3 passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 15 passed across filters.
- `cargo test -p zaion-adapters telegram -- --nocapture`: 14 passed.
- `cargo test -p zaion-a2a acp -- --nocapture`: 8 passed, 0 failed, 14
  filtered out.
- `cargo test -p zaion-runtime mcp_registry_reports_dynamic_server_toolsets -- --nocapture`:
  passed.
- `cargo test -p zaion-cli tools_list_reports_mcp_server_toolset_aliases -- --nocapture`:
  passed.
- `cargo test -p zaion-cli mcp_toolset_alias_does_not_shadow_builtin_toolset -- --nocapture`:
  passed.
- `cargo test -p zaion-cli capability_manifest_includes_dynamic_mcp_toolsets -- --nocapture`:
  passed.
- `cargo check -p zaion-runtime`: passed.
- `cargo check -p zaion-cli`: passed with existing warnings.

Status impact: TUI runtime, Telegram/live channels, and ACP/MCP/tooling remain
`PARTIAL`. The next high-value gaps are ACP live runtime event egress,
Telegram mention/allowlist/media/reaction/retry depth, MCP sampling and
`list_changed` refresh, TUI session lifecycle depth, and WebSocket attach.

## Source Coverage

This report cites latest Hermes `main`, not the old `2026.4.8` zip. The
required source areas have current evidence as follows:

| Area | Latest Hermes source anchors |
| --- | --- |
| CLI entry and command registration | `hermes`, `cli.py`, `hermes_cli/main.py`, `hermes_cli/commands.py` |
| Setup, onboarding, config | `hermes_cli/setup.py`, `hermes_cli/config.py`, `cli-config.yaml.example`, `.env.example` |
| Workspace, session, profile | `hermes_state.py`, `hermes_cli/profiles.py`, `gateway/session.py`, `website/docs/developer-guide/session-storage.md` |
| Agent loop and runtime | `run_agent.py`, `agent/*`, `tools/registry.py`, `toolsets.py`, `toolset_distributions.py` |
| Tools and approval | `tools/*`, especially `terminal_tool.py`, `file_tools.py`, `mcp_tool.py`, `memory_tool.py`, `browser_tool.py`, `code_execution_tool.py`, `delegate_tool.py`, `todo_tool.py`, `approval.py`, `tool_result_storage.py` |
| TUI and display | `agent/display.py`, `hermes_cli/curses_ui.py`, `hermes_cli/skin_engine.py`, `ui-tui/`, `tui_gateway/` |
| Gateway and channels | `gateway/run.py`, `gateway/config.py`, `gateway/session.py`, `gateway/platforms/base.py`, `gateway/platforms/telegram.py`, plus Slack/Discord/Webhook/API adapters |
| ACP/MCP | `acp_adapter/*`, `mcp_serve.py`, `hermes_cli/mcp_config.py`, `tools/mcp_tool.py` |
| Memory, context, compression | `agent/memory_manager.py`, `agent/memory_provider.py`, `agent/context_compressor.py`, `agent/prompt_builder.py` |
| Batch, environment, trajectory | `tools/environments/*`, `batch_runner.py`, `trajectory_compressor.py`, `mini_swe_runner.py`, current batch/trajectory docs |

Zaion anchors used for the current comparison include
`crates/zaion-cli/src/commands/process/tui/app.rs`,
`crates/zaion-cli/src/commands/panel_render.rs`,
`crates/zaion-runtime/src/panel_sink.rs`,
`crates/zaion-cli/src/commands/network/telegram.rs`,
`crates/zaion-cli/src/commands/network/telegram_commands.rs`,
`crates/zaion-cli/src/commands/launcher.rs`,
`crates/zaion-cli/src/commands/capability.rs`,
`crates/zaion-mcp/src/builtin_tools.rs`,
`crates/zaion-mcp/src/dispatcher.rs`,
`crates/zaion-runtime/src/turn_proof.rs`,
`crates/zaion-runtime/src/operation_stream.rs`,
`crates/zaion-runtime/src/architecture_graph.rs`, and the current ledgers.

## Hermes Architecture Map

### 1. CLI, Profile, Config, And First-Run Shell

Hermes starts through the `hermes` shim and Python CLI layers in `cli.py` and
`hermes_cli/main.py`. `hermes_cli/main.py` documents the primary commands:
default chat, `chat`, `gateway`, `gateway start/stop/status/install/uninstall`,
`setup`, `honcho setup`, and `acp`. It pre-parses `--profile` and `-p` before
normal argument parsing so `HERMES_HOME` points to the selected profile before
config, logging, providers, and tools are imported.

Command vocabulary is centralized in `hermes_cli/commands.py`. That registry is
not just CLI help text: it drives gateway slash exposure, config-gated command
availability, platform manifests, and the set of commands that must have
explicit gateway handling.

Setup and config are split across `hermes_cli/setup.py`,
`hermes_cli/config.py`, `cli-config.yaml.example`, and `.env.example`.
`config.py` owns config paths, config migration, `.env` loading, secure env
file writes, provider/profile env var injection, and config show/edit/set/check
commands. The sample config exposes a large first-class schema: terminal
backends, compression, memory, session reset, skills, toolsets, MCP servers,
gateway/platform options, and display behavior.

### 2. Workspace, Session, And Profile State

Hermes persists conversation state in `hermes_state.py`. The schema includes
sessions, messages, parent session linkage, indexes, FTS search, session title
deduplication, message/tool counts, deletion/pruning/export, and compression
lineage. It also contains hygiene for orphaned TUI/compression sessions.

Gateway session routing is in `gateway/session.py`. It models message source
context, channel/thread/topic metadata, session keys, reset policy, active
process guards, suspend/resume-pending behavior, session switching, and
session context text injected into the agent.

Profile isolation is handled by `hermes_cli/profiles.py` and the profile logic
called from `hermes_cli/main.py`. A profile can own its home, config, env,
sessions, memory files, skills, logs, plans, workspace, cron, and other runtime
state. This is a product-level model, not just a config flag.

### 3. Agent Loop, Tool Registry, And Toolsets

Hermes runtime execution is centered in `run_agent.py`, `cli.py`, and the
`agent/*` modules. The agent loop resolves model/runtime settings, builds the
system prompt, streams responses, executes tools, handles interruptions,
updates session state, records trajectories, and triggers compression.

Tools are registered through `tools/registry.py`, grouped through `toolsets.py`,
and sampled for batch runs through `toolset_distributions.py`. The latest tree
has broad built-ins: terminal, file, browser/computer-use, MCP, memory,
delegate, todo, code execution, web/search, TTS/transcription/vision/image,
cron, messaging, skills, approvals, tool-result storage, and security helpers.

### 4. TUI Runtime

Hermes latest TUI is a React/Ink frontend in `ui-tui/` plus a Python
`tui_gateway/` backend. `ui-tui/src/gatewayClient.ts` attaches through
WebSocket or stdio, waits for `gateway.ready`, sends JSON-RPC requests, and
publishes `gateway.protocol_error` when framing is malformed.

The backend in `tui_gateway/server.py` validates JSON-RPC requests and exposes
methods such as `session.create`, `prompt.submit`, `session.steer`,
`session.interrupt`, approval/clarify responses, session resume, config reads,
slash execution, and event streaming. It separates control protocol from
visible chat text.

The frontend runtime uses `ui-tui/src/hooks/useQueue.ts`,
`ui-tui/src/app/useSubmission.ts`, `ui-tui/src/app/useMainApp.ts`,
`ui-tui/src/app/useInputHandlers.ts`, and
`ui-tui/src/app/createGatewayEventHandler.ts` for busy modes
`queue|steer|interrupt`, queue edit/dequeue, approval overlays, clarify
overlays, subagent progress trees, protocol warnings, finalization, status
markers, and queue drain. The `ui-tui/src/__tests__/*` suite covers these as
first-class UI/runtime behavior.

### 5. Gateway And Channels

Hermes channel runtime is concentrated in `gateway/run.py`,
`gateway/config.py`, `gateway/session.py`, and platform adapters under
`gateway/platforms/`. `gateway/run.py` binds channel events to session state,
agent execution, typing indicators, approvals, clarifications, pending message
merge, interrupt monitoring, timeout handling, session splitting, media
delivery, final reply delivery, and follow-up queue processing.

`gateway/platforms/base.py` defines the adapter contract for sending messages,
media, typing/processing state, edits, reactions, metadata, and session-aware
delivery. `gateway/platforms/telegram.py` adds Telegram-specific depth:
MarkdownV2, message splitting, BotCommand exposure, mention and allowlist
gates, guest/group observation modes, allowed topics, media extraction/cache,
reactions, topic lanes, and reply fallback.

### 6. MCP And ACP

Hermes MCP client support is implemented through `tools/mcp_tool.py`,
`hermes_cli/mcp_config.py`, and `mcp_serve.py`. The user guide for MCP confirms
local stdio servers and remote HTTP servers, automatic startup discovery,
per-server filtering, utility wrappers for resources/prompts, dynamic
`tools/list_changed` refresh, runtime `mcp-<server>` toolsets, parallel-call
opt-in, sampling support, and a Hermes MCP server bridge.

ACP is implemented in `acp_adapter/server.py`, `acp_adapter/session.py`,
`acp_adapter/events.py`, and `acp_adapter/permissions.py`. Hermes wraps the
synchronous `AIAgent` as an async JSON-RPC stdio server with new/load/resume/
fork/list/cancel session methods, permission bridge, event bridge, tool
progress rendering, model switching, persistent sessions in `state.db`, and
editor-cwd binding for file/terminal tools.

### 7. Memory, Prompt Assembly, And Compression

`agent/prompt_builder.py` builds the system prompt from agent identity,
memory/user profile, platform hints, project context, skills, tool/runtime
context, timestamps, and session overlays. The prompt assembly docs specify
that `SOUL.md` is loaded into the identity slot, while project context files
are loaded through a priority system.

`agent/memory_provider.py` defines the provider lifecycle: initialize,
prefetch, queued prefetch, turn sync, provider tools, shutdown, session end,
session switch, pre-compression extraction, and delegation hooks.
`agent/memory_manager.py` sanitizes provider context and orchestrates providers
as a non-fatal runtime layer.

`agent/context_compressor.py` is the in-loop compressor. It decides when to
compress, protects head and recent tail context, summarizes middle turns,
tracks previous summaries, avoids tool-pair corruption, strips historical
media, handles failure cooldowns, and records compression effectiveness. The
developer docs also describe gateway session hygiene at a higher 85 percent
threshold as a pre-agent safety net.

### 8. Batch, Trajectory, And Execution Environments

Latest Hermes environment evidence is not old top-level `environments/*`.
Current evidence is under `tools/environments/*`, `batch_runner.py`,
`trajectory_compressor.py`, and `mini_swe_runner.py`.

`tools/environments/*` implements local, Docker, SSH, Singularity, Modal,
Managed Modal, Daytona, Vercel Sandbox, file sync, persistent shell, and common
environment abstractions. `batch_runner.py` uses multiprocessing workers,
checkpointing, toolset distribution sampling, isolated agent sessions, and
ShareGPT-compatible trajectory output. `trajectory_compressor.py` post-processes
trajectory JSONL with token-aware compression, sampling, metrics, and timeouts.
`mini_swe_runner.py` shows how Hermes execution environments can be used for
SWE-style trajectory generation.

## Config-Complete To First-Start Sequence

Hermes' mature path is:

1. User invokes `hermes`, `hermes chat`, `hermes --tui`, `hermes gateway`, or
   `hermes acp`.
2. `hermes_cli/main.py` pre-parses `--profile` / `-p`, resolves
   `HERMES_HOME`, and lets config/env/session code load under the selected
   profile.
3. `hermes_cli/config.py` loads `config.yaml` and `.env`, performs migrations,
   injects provider and plugin env metadata, and exposes missing setup checks.
4. If first-run requirements are missing, `cmd_chat` guides the user to
   `hermes setup` and can ask to run setup immediately when a TTY is present.
5. `hermes_cli/setup.py` runs provider/model/credential setup and writes the
   selected provider/model/config/env state.
6. CLI chat creates or resumes an `AIAgent` directly. TUI chat launches
   `ui-tui` and connects it to `tui_gateway`.
7. `tui_gateway` emits `gateway.ready`; the frontend requests
   `session.create` or resume. The gateway can return a quick skeleton and
   defer expensive `AIAgent` construction until prompt submission.
8. `prompt.submit` enters the agent loop. Prompt assembly injects identity,
   memory, context, platform/session overlays, tools, skills, and runtime
   hints.
9. Tools execute through the registry, toolsets, MCP bridge, environment
   backend, approval bridge, and result storage as applicable.
10. Responses stream back to CLI/TUI/gateway surfaces. Memory sync,
    compression, session DB writes, trajectories, channel delivery, and
    pending follow-up queue handling run around the turn.

Zaion has a clearer top-level product entry split today:
`zaion` opens the terminal neural TUI, `zaion dashboard` opens the browser UI,
`zaion start` starts full runtime/channels, and `zaion gateway start` starts
the HTTP gateway only. That entry contract is a `SURPASSED` slice. Hermes still
leads on setup/profile/config polish and the TUI/gateway first-run runtime
depth behind those entries.

## Workspace, Session, And Profile Model

Hermes model:

- Profile first: selected profile controls home directory, config, env, logs,
  sessions, memories, skills, workspace, cron, and runtime defaults.
- Session DB first: `hermes_state.py` persists sessions/messages with parent
  session lineage, titles, counts, FTS, deletion/pruning/export, and
  compression continuation semantics.
- Gateway session key first: `gateway/session.py` maps channel/user/thread/topic
  origins to a durable session key, with group-per-user, thread/topic
  isolation, reset policy, suspend, resume-pending, and active-process
  protection.
- Prompt overlay first: gateway and ACP do not mutate permanent prompt
  identity. They add source/session/workspace context as runtime overlays.

Zaion model:

- Principal first: Ed25519 principal identity and signed event chains are the
  root of runtime truth.
- Proof first: turn proofs, operation streams, capability manifests, ledger
  replay, and provenance records are part of the product contract.
- Surface separation first: callable tools and surfaces are separated through
  `capability_status` and `surface_status`.
- Current gap: profile-scoped workspace/session/memory behavior exists in
  pieces, but is not yet as product-complete and polished as Hermes' profile
  home plus gateway session model.

## Collaboration Model

Hermes collaboration flow:

| Surface | Collaboration path |
| --- | --- |
| CLI chat | `hermes_cli/main.py` / `cli.py` create `AIAgent`, build prompt context, execute tool loop, persist session state. |
| TUI | `ui-tui` sends JSON-RPC/WebSocket requests to `tui_gateway`; backend owns session/create/submit/interrupt/steer/approval/clarify and streams events back. |
| Gateway channels | Platform adapter normalizes incoming events, `gateway/session.py` resolves session, `gateway/run.py` runs or resumes the agent, adapter sends final text/media/status. |
| Tools | Agent uses `tools/registry.py` with static and dynamic toolsets; tools can request approval, store results, call MCP, run in configured environments, or delegate. |
| Memory/context | Prompt builder snapshots identity/context; memory manager prefetches and syncs providers; compressor and gateway hygiene preserve long sessions. |
| ACP/MCP | ACP exposes Hermes as an editor-facing JSON-RPC stdio agent; MCP lets Hermes consume external tools and serve a tool bridge. |
| Batch/eval | Batch runner creates isolated worker sessions, samples toolsets, records ShareGPT trajectories, and compression tools post-process outputs. |

Zaion collaboration flow today:

| Surface | Current behavior |
| --- | --- |
| CLI/start | `zaion`, `zaion dashboard`, `zaion start`, and `zaion gateway start` have stable role separation and launch-check coverage. |
| TUI | Chat-first terminal TUI exists with neural observability, right rail, slash suggestions, visible-reply isolation, busy FIFO queue drain, local queue edit/delete, local steer/interrupt busy controls, gateway event-frame ingress, and an opt-in stdio JSON-RPC transport that can bootstrap `session.create` and route `prompt.submit`, `session.steer`, and `session.interrupt`. It is still not a full Hermes-grade WebSocket/session/approval/clarify/subagent gateway. |
| Telegram | Token/status/doctor/start and simulated reply paths exist; final provider text fallback is fixed; simulation and live polling now carry receipt/storage proof slices; `TelegramAdapter.receive(...)` preserves real chat/topic/reply metadata; a live fake-API poll denies supergroup noise from that metadata without typing/reply sends; and another live poll proves env-configured allowed chat/topic gates silently deny disallowed topics. Full live Telegram behavior still needs durable config, richer mention/allowlist, guest/free-response/observation policy, batching, media, Markdown/reactions/retry, and topic/reply fallback polish. |
| Tools/MCP | Native built-ins include `fs_read`, `fs_list`, `fs_search`, `shell_exec`, `memory_search`, `capability_status`, `surface_status`, and `ledger_recent`; dispatcher records signed/provenance-aware results. Hermes still has broader dynamic MCP/toolset/sampling behavior. |
| Memory/proofs | Zaion's signed ledger, memory atom search, operation stream, and turn proofs are stronger truth primitives. Product-level profile/session/context ergonomics remain behind Hermes. |

## Detailed Comparison

| Workstream | Label | Judgment |
| --- | --- | --- |
| Top-level product entry | `SURPASSED` | Zaion has clearer roles for `zaion`, `dashboard`, `start`, and `gateway start`, verified by launch-check ledgers. |
| Ed25519 identity and signed proofs | `SURPASSED` | Zaion's principal identity, signed ledger, turn proofs, provenance, and replay contract are core advantages Hermes does not match structurally. |
| Neural/topology observability | `SURPASSED` | Zaion's evidence packets, risk records, topology/timeline/inspector/control concepts, and honest observed/estimated/unavailable labels are product differentiators. |
| Hermes latest source coverage | `PARTIAL` | Source coverage now spans required surfaces, but full module-by-module local verification is not complete enough for Hermes-wide `SURPASSED`. |
| Setup/config/profile first-run | `PARTIAL` | Zaion has provider/config/doctor paths, but Hermes still has deeper profile-scoped setup, config migration, env hygiene, and first-run UX. |
| TUI visual concept | `PARTIAL` | Zaion's chat-first neural TUI is differentiated, but runtime behavior is not yet Hermes-grade. |
| TUI JSON-RPC/event gateway | `PARTIAL` | Hermes has `ui-tui` plus `tui_gateway` JSON-RPC/WebSocket. Zaion now has a local gateway event-frame reducer, `/gateway-event <json>` ingress, structured stdio process attach, initial `session.create`, `result.session_id` tracking, and ready-session `prompt.submit` routing. Hermes still leads on WebSocket attach mode, setup/status gating, session resume/close/dequeue depth, approval/clarify responses, subagent controls, protocol recovery, deferred agent-build behavior, and broad React/Ink tests. |
| TUI busy queue, local edit/delete, and steer/interrupt UX | `PARTIAL` | Zaion queues busy plain input, drains one prompt after settlement, previews queued prompts, edits/replaces/deletes selected queued items, cancels edit before turn cancellation, pauses drain while editing, supports local `/busy queue|steer|interrupt`, records active-turn steer injections without creating user turns, queues interrupt replacements at the front after cancellation is requested, and routes busy steer/interrupt via gateway `session.steer` / `session.interrupt` when a gateway session is ready. Hermes still leads on double-tap behavior, approvals, clarify/subagent overlays, protocol warnings, and broad React/Ink tests. |
| TUI visible reply isolation | `SURPASSED` | Lifecycle-only operation events no longer render as chat text in Zaion, and tests cover the slice. |
| Approval, clarify, subagent UI events | `PARTIAL` | Hermes has full TUI event overlays and response RPCs. Zaion now records local pending approval/clarify state and subagent event state from gateway frames, but still needs real response/control wiring and richer overlays. |
| Live Telegram/channel behavior | `PARTIAL` | Zaion fixed final-content fallback, simulation, one-poll live storage receipt proof, source-bound delivery proof lookup, real chat/topic/reply receive metadata, live supergroup-noise denial without typing/reply sends, and an env-configured allowed chat/topic gate with silent signed denial evidence. Hermes still leads on durable config, richer live batching, media, reactions, retry behavior, guest/free-response/observation policy depth, Markdown polish, and topic/reply fallback. |
| Platform adapter breadth | `PARTIAL` | Zaion has multi-platform code, but Hermes' `BasePlatformAdapter` and channel runtime have deeper product semantics and proof coverage. |
| Native MCP built-ins | `PARTIAL` | Zaion's eight built-ins and signed dispatcher are valuable, but Hermes has broader built-ins and dynamic MCP/toolset behavior. |
| MCP client/server parity | `PARTIAL` | Zaion has MCP work in progress; Hermes leads on stdio/HTTP discovery, filters, runtime toolsets, dynamic refresh, sampling, utility wrappers, and MCP serving. |
| ACP parity | `PARTIAL` | Zaion has ACP surfaces, but latest-Hermes level new/load/resume/fork/cancel, event replay, permissions, editor cwd, and persistence need interoperability proof. |
| Tool breadth and result storage | `PARTIAL` | Hermes has broader production tools and environment-backed tool-result storage. Zaion now has per-result spill, live wake aggregate turn-budget enforcement before model re-entry, target-aware storage APIs for active-environment-visible spill, local live wake workspace-visible `.zaion/tool-results` spill, a structured storage-root override, local wake response propagation for receipt ids/proof-join/storage receipt summaries, an explicit backend identity hook, TUI local turns wired to a captured startup workspace root, live Telegram polling storage-receipt proof beyond simulation, and API runtime delivery JSON that preserves `resolved_addrs` for gateway target diagnostics; non-local sandbox, gateway, MCP, and delegated environment target selection remain open. |
| Session/profile/workspace model | `PARTIAL` | Zaion is principal/proof-centered; Hermes is currently more mature for profile homes, workspace-scoped state, session keys, resume, reset, and pruning. |
| Prompt assembly and memory lifecycle | `PARTIAL` | Zaion has memory/provenance direction; Hermes currently leads on prompt layer ordering, memory provider lifecycle, provider failure hygiene, and pre-compression hooks. |
| Context compression | `PARTIAL` | Zaion has compression components and now uses todo-aware compression split for current-turn active todos. Hermes still leads on durable todo/session persistence, anti-thrashing, tool-pair hygiene, media stripping, and failure behavior. |
| Batch, trajectory, environments | `PARTIAL` | Zaion has chain-gated OPD/evolve and Rust batch work; Hermes latest still leads on mature environment backends, batch runner, ShareGPT output, checkpointing, and trajectory compression. |
| ACI/AST-aware code actions | `PARTIAL` | This remains a Zaion differentiator, but latest-Hermes comparison needs a current source/test pass before it can be promoted in this report. |
| Ouroboros/self-healing | `PARTIAL` | Zaion has self-healing direction and watchdog pieces; Hermes parity work should not depend on this being complete. |
| Chain-gated self-evolution | `PARTIAL` | OPD/evolve is promotable only when the append-only Ed25519 chain verifies a latest `ConfirmedStable` record. |

## Acceptance Gates For Future `SURPASSED`

Do not upgrade the full latest-Hermes comparison to `SURPASSED` until every
target area has:

1. Latest Hermes source evidence from commit
   `9c0807070388c4f612a827230f1314ebbf24e857` or a newer explicitly recorded
   mirror commit.
2. Zaion source evidence identifying the exact implementation files.
3. Local verification commands and results, not only static inspection.
4. User-visible behavior proof for CLI/TUI/gateway/channel flows.
5. Ledger updates in this order:
   `plans/openclaw_latest_gap_report.md`,
   `plans/hermes_surpass_master_plan.md`,
   `MASTER_PLAN.md`,
   and this document if the comparison changed.

Broad verification commands remain:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -j1
cargo audit
bash scripts/check-release-assets.sh
zaion compare matrix --verify
zaion phase8b contract --all --stage paradigm --verify
zaion phase8b proof --all --stage paradigm --verify
```

For narrow slices, run the smallest relevant cargo tests first, then graduate to
the broader commands above when the slice affects shared runtime behavior.

## Next Implementation Order

1. TUI runtime parity beyond the stdio transport slice:
   approval/clarify response RPCs, subagent controls, protocol recovery,
   session lifecycle depth, WebSocket attach parity, streaming finalization,
   and terminal regression tests.
2. Live Telegram/channel parity:
   expand beyond the current source-bound proof, real receive metadata, and
   group-noise denial slice into richer mention/allowlist behavior, media
   cache, batching, reactions, retry semantics, Markdown polish,
   topic/reply fallback, and visible final replies.
3. Tools/MCP/ACP/profile/session/context parity:
   dynamic MCP discovery, runtime MCP toolsets, sampling guardrails, ACP
   load/resume/fork/permission bridge, active-environment-bound tool-result storage,
   profile-scoped workspace, prompt assembly, memory provider lifecycle, and
   compression lineage.
4. Batch/environment maturity:
   compare against `tools/environments/*`, `batch_runner.py`,
   `trajectory_compressor.py`, and `mini_swe_runner.py`; preserve Zaion's
   chain-gated OPD and signed-promotion constraints.
5. Macro-module maturity:
   only after the comparison gates above are satisfied, harden Zaion's macro
   modules until they are product-complete, documented, tested, and usable at
   Hermes maturity.

## Current Boundary

This document is a calibration and acceptance contract. It is not a victory
claim. Zaion should keep its differentiated architecture central, but the
latest-Hermes comparison remains `PARTIAL` until the remaining runtime,
channel, tool, session, context, and batch gaps are closed with source evidence
and local verification.
