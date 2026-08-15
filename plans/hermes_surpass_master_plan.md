# Hermes Surpass Master Plan (Zaion)

> Status: legacy Hermes comparison evidence ledger, not the default execution
> roadmap. Current measured facts live in `docs/PROJECT_STATUS.md`; active
> priorities live in `ROADMAP.md`; the current narrative comparison lives in
> `docs/zaion_vs_hermes.md`. Do not append routine project checkpoints here.
> Update this ledger only during explicit Hermes comparison/recalibration work
> or source-backed recovery of its historical evidence. This notice supersedes
> older rules below that require routine updates to every ledger.

## 2026-07-13 Whole-Project Organization Stage [PARTIAL]

This stage establishes a current Zaion repository map and health baseline
before further Hermes parity work. It makes no new `SURPASSED` claim; overall
latest-Hermes comparison remains `PARTIAL`.

Zaion evidence:

- 36 workspace crates, 195,899 crate-source lines, and 38 Rust files at or
  above 1,000 lines.
- `zaion-cli` remains the dominant composition root with 30 internal crate
  dependencies.
- Active interactive launch uses inline streaming chat in
  `process/tui/mod.rs`; `process/tui/app.rs::run_tui_app` is implemented but
  unreachable from production code.
- `zaion-gateway`, `zaion-opd`, and `zaion-telemetry` currently have no
  workspace consumers and require explicit leaf-product or integration
  contracts.

Repository changes:

- Added `docs/PROJECT_MAP.md`, `docs/PROJECT_STATUS.md`, `docs/README.md`,
  `plans/README.md`, and `scripts/project-audit.ps1`.
- Corrected README and execution-entry facts, refreshed the local Hermes mirror
  commit, added license/contribution files, and removed tracked MCP test output.
- Hardened CI with `**` branch matching, `--locked`, and explicit serial test
  threads.
- Updated Docker to Rust 1.93 and a locked Cargo build.
- Unified Docker, systemd, and Homebrew service startup on the foreground full
  runtime entry `zaion _daemon_run`.

Verification:

- Read-only project audit: passed; the website/hook retirement produced no
  shape warning. Remaining warnings cover the absent Git remote, damaged
  historical ledger text, and tracked machine-local settings.
- Locked/offline Cargo metadata: passed.
- Focused foundation tests: 31 passed; focused clippy: passed.
- `git diff --check`: passed.
- Claude settings JSON parsing and release validation passed after intentional
  website and repository-local hook retirement.
- Full rustfmt gate: failed on 73 pre-existing files.
- Full workspace check/test/clippy: not yet verified.

Plan impact:

| Workstream | Label | Next requirement |
| --- | --- | --- |
| Project navigation and audit | `PARTIAL` | Keep current facts in concise status/map documents and stop adding duplicate truth to every historical plan. |
| Default TUI | `PARTIAL` | Choose inline chat or the full ratatui observability app as the authoritative product path and add entry behavior tests. |
| Runtime/kernel boundary | `PARTIAL` | Move active wake/turn ownership from CLI into `zaion-runtime` incrementally. |
| Gateway/WebUI | `PARTIAL` | Choose one Rust server library; the separate public website is retired. |
| Overall Hermes surpass | `PARTIAL` | Continue latest-source parity only after repository truth and build gates are reliable. |

Remaining organization work:

- Isolate rustfmt, advisory upgrades, giant-file splits, and corrupted-ledger
  recovery into separate changes.

## 2026-06-03 Telegram Native Bare Local MEDIA Path Extraction Stage [PARTIAL SLICE]

This stage adds conservative Hermes-style bare local file extraction to
Telegram outbound media delivery. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Hermes source evidence:

- Latest Hermes `gateway/platforms/base.py::extract_local_files` detects bare
  local file paths in response text for native delivery, validates candidates
  with `os.path.isfile`, strips raw paths from visible text, and skips fenced
  and inline code spans.
- Latest Hermes `gateway/platforms/base.py::filter_local_delivery_paths` runs
  extracted paths through `validate_media_delivery_path` before dispatch.
- Latest Hermes dispatch calls `extract_local_files` after explicit `MEDIA:`
  extraction, helping small models that emit plain local file paths instead of
  `MEDIA:` syntax.

Zaion implementation:

- `TelegramAdapter::send_with_report` now scans non-code plain text lines for
  existing absolute local bare paths with allowlisted media/document
  extensions.
- Matched bare paths are removed from the user-visible text and converted into
  `TelegramOutboundMedia` entries, reusing the existing native media routing
  for photos, videos, audio/voice, documents, albums, and album fallback.
- The slice is conservative: no `~/` expansion, no relative paths, no remote
  URLs, no broad archive/document extension list, no richer allowed-root
  policy, and no cross-platform abstraction claim.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_bare_local_media_path_uploads_and_cleans_text -- --nocapture`: failed first because only the text message id `897` appeared and media id `898` was missing, then passed; fresh rerun passed, 1 test.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 7 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 40 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests, with existing dead-code/unused warnings.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram native bare local media path extraction | `PARTIAL` | Zaion can now turn existing absolute bare local paths in Telegram reply text into native uploads, reducing one outbound media gap against Hermes. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers home-relative/space-containing paths, richer safety roots, remote media policy, cross-platform propagation, cancellation ownership, and broader runtime/tool consumption. |

Next actions:

- Decide whether to broaden bare-path extraction with allowed roots and `~/`
  expansion before adding remote media downloads.
- Keep per-file policy and cross-platform outbound media delivery separate
  until the safety model is explicit.
- Continue outbound media delivery parity beyond Telegram.

## 2026-06-03 Telegram Native MEDIA Album Fallback Stage [PARTIAL SLICE]

This stage hardens the narrow Telegram album path by falling back to individual
photo uploads when Telegram `sendMediaGroup` fails. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Multi-image local `MEDIA:` replies still try Telegram `sendMediaGroup` first.
- If the album request fails, the same images are retried as individual
  `sendPhoto` uploads instead of aborting delivery.
- `TelegramDeliveryReport.fallbacks` records
  `media_group_fallback_to_photos`, and fallback photo message ids are included
  in `telegram_message_ids`.
- Existing single-image, `[[as_document]]`, audio/voice, video, and non-image
  document routing remains unchanged.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_album_failure_falls_back_to_photos -- --nocapture`: failed first because `sendMediaGroup` `ok=false` aborted delivery, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 7 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 39 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram native `MEDIA:` album fallback | `PARTIAL` | Zaion now preserves multi-image local `MEDIA:` replies when Telegram album sending fails by falling back to individual photo uploads. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers mixed-media grouping, remote/bare-path media extraction, richer safety roots, cross-platform propagation, cancellation ownership, and broader runtime/tool consumption. |

Next actions:

- Decide whether mixed-media albums or remote media should be handled before a
  broader cross-platform media abstraction.
- Add richer safety-root policy before broadening automatic path detection.
- Continue outbound media delivery parity beyond Telegram.

## 2026-06-02 Telegram Native MEDIA As-Document Policy Stage [PARTIAL SLICE]

This stage adds a narrow Hermes-style `[[as_document]]` policy to outbound
Telegram `MEDIA:` image delivery. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Standalone `[[as_document]]` directives are stripped from user-visible
  Telegram reply text.
- Local image `MEDIA:` files (`.png/.jpg/.jpeg/.gif/.webp`) marked by
  `[[as_document]]` route to Telegram `sendDocument` with multipart field
  `document`, preserving original bytes instead of using `sendPhoto`.
- Ordinary image `MEDIA:` delivery still routes to `sendPhoto`, and the
  previous video/audio/explicit-voice/document routing remains unchanged.
- The slice is conservative: existing absolute local files only, 50 MiB max,
  no remote URL delivery, no bare-path auto-detection, no media grouping, no
  per-file policy granularity, and no cross-platform abstraction claim.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_as_document -- --nocapture`: failed first because `[[as_document]]` leaked into visible text and the image was not yet delivered as a document, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 5 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 37 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram native `MEDIA:` as-document image policy | `PARTIAL` | Zaion can now preserve local image outputs as original-byte Telegram documents when explicitly requested, reducing one lossless-media gap against Hermes. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media grouping, remote/bare-path media extraction, richer safety roots, cross-platform propagation, cancellation ownership, and broader runtime/tool consumption. |

Next actions:

- Add media grouping/albums and richer media safety roots before broadening
  automatic path detection.
- Decide whether per-file media policy belongs in structured tool results
  rather than plain response directives.
- Continue cross-platform outbound media delivery parity beyond Telegram.

## 2026-06-02 Telegram Native MEDIA Audio/Voice Routing Stage [PARTIAL SLICE]

This stage extends the narrow Telegram outbound `MEDIA:` upload path to native
audio and explicit voice routing. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Standalone `[[audio_as_voice]]` directives are stripped from user-visible
  Telegram reply text and mark outbound `MEDIA:` files in the same response as
  voice-intended.
- Local `.mp3/.wav/.m4a/.flac/.ogg/.opus` `MEDIA:` files route to Telegram
  `sendAudio` with multipart field `audio` by default.
- Local `.ogg/.opus` files marked with `[[audio_as_voice]]` route to Telegram
  `sendVoice` with multipart field `voice`, avoiding accidental conversion of
  ordinary audio into voice messages.
- Existing image/video/document `MEDIA:` routing, absolute local-file
  validation, 50 MiB max file limit, reply/topic metadata, cleaned text
  delivery, and media message-id reporting remain intact.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_routes_audio -- --nocapture`: failed first because `.mp3` still routed to `sendDocument` and `[[audio_as_voice]]` remained in visible text, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 4 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 36 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram native `MEDIA:` audio/voice routing | `PARTIAL` | Zaion can now turn local audio `MEDIA:` directives into native Telegram audio or explicit voice uploads, reducing one outbound media gap against Hermes. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media grouping, lossless document policy, remote/bare-path media extraction, richer safety roots, cross-platform propagation, cancellation ownership, and broader runtime/tool consumption. |

Next actions:

- Add `[[as_document]]` / lossless delivery policy and richer media safety
  roots before broadening automatic path detection.
- Decide whether bare path media extraction belongs in model-output text
  parsing or a structured tool-result channel.
- Continue cross-platform outbound media delivery parity beyond Telegram.

## 2026-06-02 Telegram Native MEDIA Tag Delivery Stage [PARTIAL SLICE]

This stage adds a narrow Hermes-style outbound media delivery path for
Telegram replies. It does not promote the whole-plan verdict: latest-Hermes
parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::send_with_report` now extracts standalone
  `MEDIA:<absolute-path>` local file tags from outbound text and removes those
  internal tags from the user-visible text message.
- Cleaned text still uses the existing Telegram `sendMessage` path with
  Markdown fallback, topic metadata, and reply anchoring.
- Existing reply/topic metadata is reused for media uploads, and media message
  ids are included in `TelegramDeliveryReport`.
- Local image extensions route to `sendPhoto`, local video extensions route to
  `sendVideo`, and other accepted local files route to `sendDocument`.
- The slice is conservative: existing absolute local files only, 50 MiB max,
  no remote URL delivery, no bare-path auto-detection, no media grouping, no
  audio voice routing, and no cross-platform outbound media abstraction claim.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_uploads_local_image_and_cleans_text -- --nocapture`: failed first because only `sendMessage` ran and no native media upload occurred, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 34 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram native `MEDIA:` tag delivery | `PARTIAL` | Zaion can now turn local `MEDIA:` file directives into native Telegram photo/video/document uploads, reducing one outbound media gap against Hermes. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers richer file safety roots, media grouping, audio/voice routing, remote media, cross-platform propagation, cancellation ownership, and broader runtime/tool consumption. |

Next actions:

- Add richer media safety roots and policy knobs before broadening path
  detection.
- Decide whether audio should route to voice/audio and whether images need an
  `[[as_document]]` lossless delivery directive.
- Continue cross-platform outbound media delivery parity beyond Telegram.

## 2026-06-02 Telegram Cached Video Vision Context Stage [PARTIAL SLICE]

This stage extends opt-in cached Telegram media vision from images into
provider-backed video description. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_wake_request` can now include a `Telegram media vision analysis`
  context block for cached Telegram `video/*` files when
  `ZAION_TELEGRAM_MEDIA_VISION` is enabled.
- The video path reads only the local cached file and sends it as a
  `data:<mime>;base64,...` multimodal `video_url` item to an OpenAI-compatible
  `/v1/chat/completions` endpoint.
- Native Telegram videos and video documents preserve their existing cached
  MIME/type evidence, Telegram `file_id`, delivery metadata, and canonical
  envelope/source-hash semantics.
- This is provider-backed video description only; local video decoding, frame
  extraction, OCR, and rich temporal scene analysis remain follow-ups.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_video_vision_context_reaches_llm -- --nocapture`: failed first because no media video vision request was sent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached video vision context | `PARTIAL` | Zaion can now pass cached Telegram videos to an OpenAI-compatible multimodal video endpoint behind the existing media-vision gate; local video understanding, OCR, and general video tooling remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers broader media/document breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Decide whether local frame extraction/OCR belongs in Telegram ingress or a
  general video-analysis tool surface.
- Continue outbound native media delivery parity.
- Keep latest-Hermes parity labeled `PARTIAL` until broader runtime/channel
  gaps close.

## 2026-06-02 Telegram Cached PDF Document Context Stage [PARTIAL SLICE]

This stage extends the opt-in cached Telegram document text path from plain
text and Office documents into bounded PDF literal-text extraction. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_wake_request` can now include clipped text extracted from cached
  Telegram `.pdf` documents when `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled.
- PDF extraction reads only the local cached file, scans at most 1 MiB, requires
  a `%PDF` header near the start, decodes common PDF literal-string escapes,
  and collects uncompressed literal strings used by basic `Tj` / `TJ` text
  operators into the existing `Telegram document text` context block.
- Existing text, DOCX, PPTX, and XLSX extraction remains intact, while
  compressed streams, complex encodings, OCR, and provider-backed rich document
  analysis remain follow-ups.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain
  stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_pdf_document_context_reaches_llm -- --nocapture`: failed first because no `Telegram document text` context reached the first LLM request for a cached `.pdf`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 38 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached PDF document context | `PARTIAL` | Zaion now extracts bounded uncompressed literal text from cached Telegram PDFs behind the existing opt-in document-text gate; rich PDF parsing/OCR and general document tooling remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers broader document/media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Decide whether richer document parsing belongs in Telegram ingress or a
  general document tool surface.
- Add provider-backed document analysis and/or OCR for PDFs that do not expose
  simple literal text.
- Continue outbound native media delivery parity.

## 2026-06-02 Telegram Cached XLSX Document Context Stage [PARTIAL SLICE]

This stage extends the opt-in cached Telegram document text path from plain
text, DOCX, and PPTX into bounded XLSX worksheet text extraction. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_wake_request` can now include clipped text extracted from cached
  Telegram `.xlsx` documents when `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled.
- XLSX extraction reads only the local cached file, opens the ZIP central
  directory, accepts store/deflate entries, rejects ZIP64 and oversized XML
  entries, reads `xl/sharedStrings.xml` when present, scans
  `xl/worksheets/sheet*.xml` in path order, and extracts shared-string,
  inline-string, and basic cell values into the existing
  `Telegram document text` context block.
- Existing text, DOCX, and PPTX document extraction remains intact, while
  cached PDFs and richer spreadsheet semantics continue to stay
  metadata/cached-path only.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain
  stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_xlsx_document_context_reaches_llm -- --nocapture`: failed first because no `Telegram document text` context reached the first LLM request for a cached `.xlsx`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 37 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached XLSX document context | `PARTIAL` | Zaion now extracts bounded worksheet text from cached Telegram spreadsheets behind the existing opt-in document-text gate; PDF extraction, richer Office parsing, and tool-mediated document analysis remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers broader document/media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add sandboxed PDF extraction or a provider-backed document extraction seam.
- Decide whether richer Office parsing belongs in Telegram ingress or a
  general document tool surface.
- Continue outbound native media delivery parity.

## 2026-06-02 Telegram Cached PPTX Document Context Stage [PARTIAL SLICE]

This stage extends the opt-in cached Telegram document text path from plain
text and DOCX into bounded PPTX slide-text extraction. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_wake_request` can now include clipped text extracted from cached
  Telegram `.pptx` documents when `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled.
- PPTX extraction reads only the local cached file, opens the ZIP central
  directory, accepts store/deflate entries, rejects ZIP64 and oversized XML
  entries, scans `ppt/slides/slide*.xml` in path order, and extracts `<a:t>`
  slide text into the existing `Telegram document text` context block.
- Existing text and DOCX document extraction remains intact, while cached PDFs
  and richer spreadsheet/document formats continue to stay metadata/cached-path
  only.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain
  stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_pptx_document_context_reaches_llm -- --nocapture`: failed first because no `Telegram document text` context reached the first LLM request for a cached `.pptx`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 36 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached PPTX document context | `PARTIAL` | Zaion now extracts bounded PPTX slide text from cached Telegram documents behind the existing opt-in document-text gate; PDF, XLSX, richer Office parsing, and tool-mediated document analysis remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers broader document/media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add sandboxed PDF extraction or a provider-backed document extraction seam.
- Extend Office handling to XLSX where safe.
- Continue outbound native media delivery parity.

## 2026-06-02 Telegram Cached DOCX Document Context Stage [PARTIAL SLICE]

This stage extends the opt-in cached Telegram document text path from plain
text-like files to a bounded DOCX extraction path. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_wake_request` can now include clipped text extracted from cached
  Telegram `.docx` documents when `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled.
- DOCX extraction reads only the local cached file, opens the ZIP central
  directory, accepts store/deflate entries, rejects ZIP64 and oversized XML
  entries, and extracts `word/document.xml` `<w:t>` text into the existing
  `Telegram document text` context block.
- Existing plain text document extraction remains unchanged, while cached PDFs
  continue to stay metadata/cached-path only.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain
  stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_docx_document_context_reaches_llm -- --nocapture`: failed first because no `Telegram document text` context reached the first LLM request for a cached `.docx`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 35 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached DOCX document context | `PARTIAL` | Zaion now extracts bounded DOCX paragraph text from cached Telegram documents behind the existing opt-in document-text gate; PDF, XLSX, PPTX, richer Office parsing, and tool-mediated document analysis remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers broader document/media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add sandboxed PDF extraction or a provider-backed document extraction seam.
- Extend Office handling beyond DOCX where safe, especially XLSX/PPTX.
- Continue outbound native media delivery parity.

## 2026-06-02 Telegram Cached Text Document Context Stage [PARTIAL SLICE]

This stage adds opt-in cached text-document extraction without promoting the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_wake_request` can append a `Telegram document text` system context
  block when `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled and cached media metadata
  points at a text-like Telegram `document`.
- Text extraction reads only the local cached file, accepts text MIME types plus
  common text extensions, strips NUL bytes, and clips each preview to 16 KiB.
- Non-text cached documents such as PDFs keep the existing metadata/cached-path
  behavior without accidental prompt injection.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_text_document_context_reaches_llm -- --nocapture`: failed first because no `Telegram document text` context reached the first LLM request, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_generic_document_dispatches_and_records_media_metadata -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_transcription_context_reaches_llm -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 34 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached text document context | `PARTIAL` | Zaion now carries cached text-like document previews into live wake context behind an explicit opt-in gate; PDF/Office extraction, video analysis, and broader document tooling remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers production media/document breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add sandboxed PDF/Office document extraction paths.
- Decide image-document and video-document analysis gates.
- Continue outbound native media delivery parity.

## 2026-05-30 Telegram Cached Audio Transcription Context Stage [PARTIAL SLICE]

This stage adds opt-in cached voice/audio transcription without promoting the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Cached Telegram audio transcription now uses a narrow OpenAI-compatible
  `/audio/transcriptions` client.
- `telegram_wake_request` can append a `Telegram audio transcription` system
  context block when `ZAION_TELEGRAM_AUDIO_TRANSCRIPTION` is enabled and cached
  media metadata points at `audio/*` voice/audio files.
- The transcription request posts cached bytes as multipart form data and uses
  explicit audio-transcription env overrides for base URL, model, and API key.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain
  stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_transcription_context_reaches_llm -- --nocapture`: failed first because no audio transcription request was sent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_dispatches_and_records_media_metadata -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_wake_request -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_vision_context_reaches_llm -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 33 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached audio transcription context | `PARTIAL` | Zaion now performs opt-in cached voice/audio transcription and carries the generated transcript into live wake context; document extraction, video analysis, and outbound native media remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers production media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add document extraction paths.
- Decide how image documents and videos should expose analysis in model/tool context.
- Continue outbound native media delivery parity.
## 2026-05-30 Telegram Cached Photo Vision Context Stage [PARTIAL SLICE]

This stage adds opt-in cached-photo vision analysis without promoting the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Static sticker vision and cached Telegram photo vision now share a reusable
  OpenAI-compatible image vision client.
- `telegram_wake_request` can append a `Telegram media vision analysis` system
  context block when `ZAION_TELEGRAM_MEDIA_VISION` is enabled and cached media
  metadata points at `image/*` non-sticker files.
- The media vision request sends cached bytes as a multimodal data URL to
  `/v1/chat/completions` and uses explicit media-vision env overrides for base
  URL, model, and API key.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound caption/text, so signed ingress and duplicate semantics remain stable.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_vision_context_reaches_llm -- --nocapture`: failed first because no media vision request was sent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_sticker_vision_describer_reaches_llm_delivery_and_cache -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_wake_request -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 32 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached photo vision context | `PARTIAL` | Zaion now performs opt-in cached image vision analysis and carries the generated description into live wake context; wider media consumption, transcription, document extraction, and outbound native media remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers production media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add audio transcription and document extraction paths.
- Decide how image documents and videos should expose analysis in model/tool context.
- Continue outbound native media delivery parity.

**Version**: v1.3
**Date**: 2026-05-29 latest-main continuation
**Status**: long-horizon execution entry; the gap ledger remains the source of truth.
**Project root**: `D:/zaion-rust`
**Latest update**: Telegram captioned photo updates now dispatch through the
live wake path, preserve caption/photo metadata, cache Telegram photos under
Zaion's managed media root with signed cached-path evidence, merge same-batch
`media_group_id` photo albums, debounce same-album photos across adjacent Bot
API polls, cache Telegram image documents delivered as Bot API
`message.document` when their MIME type starts `image/`, cache inbound
Telegram voice/audio files under the audio cache root, cache inbound Telegram
native video files plus video documents under the video cache root, cache
inbound generic Telegram documents under the document cache root, preserve
Telegram sticker metadata through live dispatch and signed delivery evidence,
cache static Telegram sticker binaries under the image cache root, inject
cached sticker descriptions into model-visible live Telegram turns with signed
delivery/envelope evidence, and generate/write back static sticker descriptions
through a deterministic provider seam on cache misses. Whole surpass state
remains `PARTIAL`; production sticker vision provider wiring, model-visible
media consumption, task-handle cancellation, bounded join/unwind,
delegated/remote sandbox paths, and broader gateway/channel propagation remain
open.

---

## 2026-05-30 Telegram Cached Media Model Context Stage [PARTIAL SLICE]

This stage exposes cached Telegram media references to live wake model context
without promoting the whole-plan verdict: latest-Hermes parity remains
`PARTIAL`.

Zaion implementation:

- `WakeRequest` now carries `extra_model_context` into the wake prompt as
  system context before the user message.
- Live Telegram wake requests add a `Telegram cached media` block from canonical
  envelope metadata when cached paths are available.
- The block includes cached path, media type, MIME type, Telegram `file_id`,
  and `file_unique_id` where present, while keeping media bytes out of the
  prompt.
- Canonical Telegram envelopes and source hashes remain bound to original
  inbound text/fallback text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: failed first because the first LLM request lacked `Telegram cached media`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 31 tests.
- `cargo test -j 1 -p zaion-cli telegram_wake_request -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached media model-visible references | `PARTIAL` | Zaion now gives the model signed cached media references without disrupting canonical ingress; direct media-byte analysis/transcription and document extraction remain follow-ups. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers production media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add safe non-sticker image analysis and audio/document extraction paths.
- Continue outbound native media delivery parity.
- Keep latest-Hermes parity labeled `PARTIAL` until broader runtime/channel gaps close.

## 2026-05-30 Telegram Sticker Production Vision Stage [PARTIAL SLICE]

This stage wires the sticker description seam to an explicit OpenAI-compatible
production vision provider. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram_adapter_for_runtime` can attach `OpenAiStickerDescriber` behind
  the `ZAION_TELEGRAM_STICKER_VISION` opt-in gate.
- The describer posts cached static sticker bytes as a data-URL multimodal
  `image_url` to an OpenAI-compatible `/v1/chat/completions` endpoint.
- Sticker-specific env overrides configure base URL, model, and API key before
  falling back to OpenAI config/provider maps.
- Live Telegram delivery and canonical wake envelopes propagate the
  vision-generated description into signed evidence, runtime context, and the
  persisted sticker description cache.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_sticker_vision_describer_reaches_llm_delivery_and_cache -- --nocapture`: failed first because no production vision request was sent, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 15 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 31 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram static sticker production vision provider | `PARTIAL` | Zaion now has opt-in production vision analysis with signed prompt/cache/evidence propagation, but media consumption breadth and animated/video sticker policy remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers production media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Carry cached media/documents into model/tool-visible context where appropriate.
- Decide animated/video sticker handling.
- Continue outbound native media delivery parity.

## 2026-05-30 Telegram Sticker Description Generation Stage [PARTIAL SLICE]

This stage adds generated sticker description write-back after cached sticker
description injection. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now caches static sticker bytes before deriving
  sticker text, so uncached stickers can be described from the local cached
  image path.
- `TelegramStickerDescriber` is a narrow provider seam carrying cached path,
  MIME type, emoji, set name, and Telegram `file_unique_id`.
- Generated descriptions are persisted to `sticker_descriptions.json` by
  `file_unique_id` and injected into model-visible Telegram text.
- Description metadata is preserved as `telegram_sticker_description` and
  `telegram_sticker_description_source: "generated"`.
- Live Telegram delivery and canonical wake envelopes propagate generated
  descriptions into signed evidence and runtime context.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_generates_and_caches_static_sticker_description -- --nocapture`: failed first because the adapter emitted the old fallback text, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_generated_sticker_description_reaches_llm_delivery_and_cache -- --nocapture`: failed first because the LLM request/delivery path lacked generated description metadata, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 15 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 30 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram generated sticker description write-back | `PARTIAL` | Zaion now has cache-miss generation/write-back and prompt-visible signed propagation, but production vision provider wiring remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers production media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Wire a real sticker vision/model provider to `TelegramStickerDescriber`.
- Carry cached media/documents into model/tool-visible context where appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Sticker Description Cache Stage [PARTIAL SLICE]

This stage adds cached sticker description prompt injection after static sticker
binary caching. It does not promote the whole-plan verdict: latest-Hermes
parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now reads
  `sticker_descriptions.json` from the Telegram media cache root and looks
  up descriptions by Telegram `file_unique_id`.
- Cache hits produce model-visible sticker description text while retaining
  sticker emoji/set context.
- Description metadata is preserved as `telegram_sticker_description` and
  `telegram_sticker_description_source: "cache"`.
- Live Telegram delivery and canonical wake envelopes propagate the cached
  description into signed evidence and runtime context.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_injects_cached_sticker_description -- --nocapture`: failed first because the adapter emitted the old fallback text, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_cached_sticker_description_reaches_llm_and_delivery -- --nocapture`: failed first because live delivery/envelope metadata lacked the description, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 14 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 29 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed after formatting.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cached sticker description injection | `PARTIAL` | Cached sticker descriptions now survive receive/dispatch/proof as prompt-visible context, but Hermes still has automatic vision analysis and cache write-back for newly seen static stickers. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add real sticker vision analysis and description cache write-back.
- Carry cached media/documents into model/tool-visible context where appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Static Sticker Cache Stage [PARTIAL SLICE]

This stage closes the next sticker-processing gap by downloading and caching
static Telegram sticker binaries. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now downloads static, non-animated, non-video
  sticker files through Telegram `getFile` when media caching is configured.
- Static sticker file paths are validated through the existing safe relative
  path policy before download.
- Sticker image bytes are cached in the image cache tier, preserving `.webp`
  / `image/webp` evidence and common image fallback extensions.
- Live Telegram delivery and canonical wake envelopes preserve static-sticker
  cached-path evidence through the existing generic media metadata propagation
  path.
- Animated/video sticker handling remains a metadata-only fallback.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_static_sticker -- --nocapture`: failed first because no cached sticker path existed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_static_sticker_dispatches_and_records_cached_media_metadata -- --nocapture`: failed first because live dispatch made no sticker `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 13 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 28 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram static sticker cache | `PARTIAL` | Static sticker binaries now survive receive/dispatch/proof as cached media, but Hermes still has sticker vision description and cached prompt injection that Zaion lacks. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add sticker vision-description injection and description caching.
- Carry cached media/documents into model/tool-visible context where appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Sticker Metadata Stage [PARTIAL SLICE]

This stage closes the first sticker-processing gap by preserving Telegram
sticker facts through receive, wake, and signed delivery. It does not promote
the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now gives sticker-only messages a stable
  fallback text so private-chat sticker updates can dispatch through the live
  wake path.
- Telegram sticker type, dimensions, emoji, set name, animation/video flags,
  file size, and custom emoji id are recorded as inbound metadata when present.
- Live Telegram delivery and canonical wake envelopes copy the sticker-specific
  metadata into signed evidence and runtime context.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_preserves_sticker_media_metadata -- --nocapture`: failed first because sticker-only text was empty and sticker-specific metadata was absent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_sticker_dispatches_and_records_media_metadata -- --nocapture`: failed first because the sticker-only update did not reach LLM/sendMessage delivery, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 12 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 27 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram sticker metadata/evidence | `PARTIAL` | Sticker facts now survive live receive/dispatch/proof, but Hermes still has sticker image analysis and cached description injection that Zaion lacks. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add static sticker cache plus model-visible description injection.
- Carry cached media/documents into model/tool-visible context where appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Generic Document Cache Stage [PARTIAL SLICE]

This stage closes the next Telegram media-cache gap after native/video document
caching, covering generic Telegram documents such as PDFs. It does not promote
the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now downloads inbound generic Telegram
  document files through the safe `getFile` + file download path when media
  caching is configured.
- Image and video documents continue to use their specialized cache paths before
  the generic document policy runs.
- Generic documents use allowlisted extension inference, preserve Telegram MIME
  metadata when present, and default unknown files to safe binary metadata.
- Document bytes are cached through `MediaCacheManager`'s document cache,
  producing `telegram_media_cached_paths` and
  `telegram_media_cached_mime_types`.
- Live Telegram delivery and canonical wake envelopes preserve generic-document
  cached-path evidence through the existing generic media metadata propagation
  path.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_generic_document -- --nocapture`: failed first because generic documents had no cached media path, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_generic_document_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no generic-document `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 11 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 26 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram generic document cache | `PARTIAL` | Generic documents now share the safe cache/evidence path with photos, image documents, voice/audio, and videos, but stickers, outbound media, and direct model/tool media consumption remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add explicit cache/processing policy for stickers.
- Carry cached documents into model/tool-visible context where appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Video Cache Stage [PARTIAL SLICE]

This stage closes the next Telegram media-cache gap after voice/audio caching,
covering both native Telegram video and video documents. It does not promote
the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `MediaCacheManager` now includes a dedicated `videos` cache tier plus
  video byte/URL cache helpers and cleanup coverage.
- `TelegramAdapter::receive()` now downloads inbound Telegram native video
  files and `video/*` Telegram documents through the safe `getFile` + file
  download path when media caching is configured.
- Native video messages and video documents use common video extension
  inference, default to `.mp4` / `video/mp4`, and retain Telegram `video/*`
  MIME metadata.
- Video bytes are cached through the video cache, producing
  `telegram_media_cached_paths` and `telegram_media_cached_mime_types`.
- Live Telegram delivery and canonical wake envelopes preserve native-video and
  video-document cached-path evidence through the existing generic media
  metadata propagation path.

Verification:

- `cargo test -j 1 -p zaion-adapters media_cache_four_tier_structure -- --nocapture`: failed first because `cache_video_from_bytes` did not exist, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_video_message -- --nocapture`: failed first because video messages had no cached media path, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_video_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no video `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_video_document -- --nocapture`: failed first because video documents were still generic documents, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_video_document_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no video-document `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 10 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 25 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram native video and video-document cache | `PARTIAL` | Native video and video documents now share the safe cache/evidence path with photos, image documents, and voice/audio, but stickers, generic documents, outbound media, and direct model/tool media consumption remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add explicit cache/processing policy for stickers and generic documents.
- Carry cached native-video and video-document paths into model/tool-visible
  context where appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Voice/Audio Cache Stage [PARTIAL SLICE]

This stage closes the next Telegram media-cache gap after image-document
caching. It does not promote the whole-plan verdict: latest-Hermes parity
remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now downloads inbound Telegram voice/audio
  files through the safe `getFile` + file download path when media caching is
  configured.
- Voice notes default to `.ogg` / `audio/ogg`; audio messages use common
  audio extension inference and retain Telegram `audio/*` MIME metadata.
- Audio bytes are cached through `MediaCacheManager`'s audio cache, producing
  `telegram_media_cached_paths` and `telegram_media_cached_mime_types`.
- Live Telegram delivery and canonical wake envelopes preserve voice cached-path
  evidence through the existing generic media metadata propagation path.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_voice_message -- --nocapture`: failed first because no cached voice path existed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no voice `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 8 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 23 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram voice/audio cache | `PARTIAL` | Voice/audio now share the safe cache/evidence path with photos and image documents, but transcription/model-visible audio consumption, video, stickers, generic documents, and outbound media remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add explicit cache/processing policy for video, stickers, and generic
  documents.
- Carry cached audio paths into transcription/model/tool-visible context where
  appropriate.
- Continue outbound native media delivery parity.

## 2026-05-29 Telegram Image-Document Cache Stage [PARTIAL SLICE]

This stage closes the next Telegram media-cache gap after native photo caching
and album batching. It does not promote the whole-plan verdict: latest-Hermes
parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now distinguishes `image/*` Telegram documents
  as `document_image` media instead of generic `document` media.
- Image documents use the existing safe Telegram `getFile` path validation,
  file download, and `MediaCacheManager` image cache flow.
- Inbound metadata now includes document filename/MIME evidence plus cached
  path/MIME arrays.
- Live Telegram delivery and canonical wake envelopes copy the new
  image-document metadata into signed evidence and runtime context.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_image_document -- --nocapture`: failed first because image documents were generic documents without cached paths, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_image_document_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no image-document `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 7 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 22 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram image-document cache | `PARTIAL` | Image documents now share the safe cache/evidence path with photos, but Zaion still trails Hermes on voice/audio, video, stickers, generic document policy, outbound media, and direct model/tool media consumption. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers media breadth, channel propagation, cancellation ownership, bounded unwind, and broader runtime/tool consumption. |

Next actions:

- Add explicit cache/processing policy for voice/audio, video, stickers, and
  generic documents.
- Carry cached media paths into model/tool-visible context where appropriate.
- Continue outbound native media delivery parity.

---

## 2026-05-29 Telegram Cross-Poll Album Debounce Stage [PARTIAL SLICE]

This stage closes the next album-batching gap for live Telegram photo
messages split across `getUpdates` calls. It does not promote the whole-plan
verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Live Telegram runtime now keeps a `TelegramAlbumDebounceBuffer` across polls,
  keyed by chat, topic, and `telegram_media_group_id`.
- Single-photo album fragments are held, merged with later adjacent-poll
  fragments, and flushed after a bounded quiet window into the normal wake
  path.
- The merged cross-poll album preserves first caption/trigger text,
  `telegram_album_message_ids`, `telegram_album_update_ids`, cached paths,
  MIME types, file ids, unique ids, and summed photo counts.
- Pending album state temporarily lowers Telegram `getUpdates.timeout` to one
  second so the flush window is not hidden by the default long-poll delay.
- Already adapter-merged same-batch albums still dispatch immediately.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_debounces_photo_album_across_polls_before_dispatch -- --nocapture`: failed first because the first poll dispatched immediately, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_uses_configured_get_updates_timeout -- --nocapture`: failed first because `getUpdates.timeout` was fixed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_merges_photo_album_before_dispatch -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 6 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 21 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cross-poll photo album debounce | `PARTIAL` | Cross-poll photo albums now produce one wake turn and signed delivery, but media breadth and sub-second production flushing still trail Hermes. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers channel breadth, media types, cancellation ownership, bounded unwind, and broader gateway/runtime propagation. |

Next actions:

- Extend media cache/debounce handling to image documents and mixed-media
  policies.
- Carry cached album paths into model/tool-visible context where appropriate.
- Continue media parity for voice/audio, video, stickers, documents, and
  outbound native media.

---

## 2026-05-29 Telegram Photo Album Merge Stage [PARTIAL SLICE]

This stage closes the first album-batching gap for live Telegram photo
messages. It does not promote the whole-plan verdict: latest-Hermes parity
remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now merges same-batch photo messages sharing
  chat, topic, and `telegram_media_group_id`.
- The merged message keeps the first caption/trigger as the canonical prompt
  and records `telegram_album_message_ids` plus `telegram_album_update_ids`.
- Photo metadata and cached media arrays are appended across the album, and
  `telegram_photo_count` is summed.
- Live Telegram delivery/envelope propagation now carries the album metadata,
  so a same-batch album produces one wake turn, one reply, and one signed
  `telegram.delivery` with multiple cached paths.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_merges_photo_album_metadata_and_cached_paths -- --nocapture`: failed first because same-album updates emitted two inbound messages, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_merges_photo_album_before_dispatch -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 5 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 20 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram photo album merge | `PARTIAL` | Same-batch albums are now merged and cached-path evidence survives live dispatch; cross-poll debounce is covered by the newer slice, while mixed media breadth remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until latest-source parity covers channel breadth, media types, cancellation ownership, bounded unwind, and broader gateway/runtime propagation. |

Next actions:

- Extend album debounce/cache handling to mixed media and image documents.
- Extend cache and evidence handling to image documents, voice/audio, video,
  stickers, and generic documents.
- Feed cached album paths into model/tool-visible context where appropriate.

---

## 2026-05-28 Telegram Photo Download Cache Stage [PARTIAL SLICE]

This stage adds the first Hermes-style Telegram media cache path. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter` now accepts a media cache root and, for photo updates,
  calls Telegram Bot API `getFile` for the largest photo.
- Returned Telegram `file_path` values are accepted only as safe relative
  paths before constructing the `/file/bot<TOKEN>/<file_path>` download URL.
- Downloaded photo bytes are cached through the existing `MediaCacheManager`
  image cache, producing `telegram_media_cached_paths` and
  `telegram_media_cached_mime_types` metadata.
- Live Telegram runtime uses `data_dir()/cache/telegram`, and
  `telegram.delivery` plus the canonical wake envelope preserve cached paths
  and MIME evidence.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_largest_photo -- --nocapture`: failed first because the adapter had no media cache root/download path, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: failed first because the live caption-photo path had no `getFile`/download request or cached-path delivery evidence, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 4 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 19 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 21 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 23 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram photo media cache | `PARTIAL` | Zaion now has safe `getFile` download/cache for incoming photos and signed cached-path evidence, but still needs album batching, image-document/voice/video/document/sticker support, and model-visible media consumption depth. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Implement `media_group_id` album debounce/merge before wake dispatch.
- Extend cache handling to image documents, voice/audio, video, stickers, and
  generic documents.
- Add prompt/tool consumption of cached media paths rather than only evidence
  propagation.

---

## 2026-05-28 Telegram Caption Photo Metadata Stage [PARTIAL SLICE]

This stage makes Telegram captioned photo messages visible to Zaion's live
wake path and signed proof chain. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAdapter::receive()` now treats `message.caption` as inbound text
  when `message.text` is absent.
- Caption entities flow through the existing Telegram mention extraction path,
  allowing direct bot mentions in captions to pass group trigger gating.
- Telegram receive metadata records caption, media group id, media type,
  largest photo `file_id`, largest photo `file_unique_id`, and photo-size
  count for incoming photo messages.
- `telegram.delivery` and the canonical wake envelope now copy those media
  metadata fields so downstream runtime and audit consumers can see that the
  source turn included a photo.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_preserves_caption_photo_media_metadata -- --nocapture`: failed first because caption/photo metadata was missing, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: failed first because the live caption-photo path did not reach signed media evidence, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 3 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 19 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 20 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 23 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram caption/photo metadata | `PARTIAL` | Zaion now has live caption-triggered photo dispatch and signed media metadata evidence, but still lacks Hermes' media download/cache, album batching, sticker/voice/video/document processing, and model-visible cached media files. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Add safe media cache roots and Telegram file download for photos/image
  documents.
- Implement album/rapid-photo batching with `media_group_id` before wake
  dispatch.
- Extend media evidence and processing to voice, video, sticker, and document
  messages.

---

## 2026-05-28 Telegram Stop Guard Release Stage [PARTIAL SLICE]

This stage adds bounded local guard-release semantics to the interruptible
Telegram runner. It does not promote the whole-plan verdict: latest-Hermes
parity remains `PARTIAL`.

Zaion implementation:

- `TelegramTaskRunner` records active background/held task owner metadata by
  Telegram thread/message.
- `/stop` sends its command response before cancellation cleanup, then
  synthesizes signed `status: "cancelled"` completions for unfinished active
  tasks.
- Synthetic cancelled completions release the busy guard and return the latest
  queued follow-up once.
- Late completions from already-cancelled background owners are dropped instead
  of writing duplicate delivery events or releasing queues twice.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_stop_synthesizes_cancelled_completion_for_unfinished_task_and_releases_pending -- --nocapture`: failed first because `/stop` did not release the queued follow-up, then passed.
- `cargo test -j 1 -p zaion-cli telegram_task_runner_accepts_stop_while_active_turn_is_in_flight -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_stop_command -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-cli telegram_cancelled_turn_completion_suppresses_reply_and_records_cancelled_delivery -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 22 tests.
- `cargo fmt -p zaion-cli --check`: passed.
- `git diff --check -- crates/zaion-cli/src/commands/network/telegram.rs`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram stop guard release | `PARTIAL` | Zaion now prevents stopped Telegram turns from wedging the local busy guard and dedupes stale late completions, but still needs true task-handle cancel/join semantics to match Hermes. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Add real background task ownership and timeout-bounded join/unwind where the
  Rust runtime allows it.
- Propagate the same owner/cancel model into delegated/remote execution and
  broader platform adapters.

---

## 2026-05-28 Telegram Cancelled Completion Stage [PARTIAL SLICE]

This stage tightens the interruptible Telegram runner by making cooperative
cancellation visible in completion semantics. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `collect_wake_reply(...)` now preserves `StreamEvent::Cancelled`.
- `run_telegram_turn_task(...)` checks the stream cancellation event and the
  active `StreamCallback` cancel flag after wake returns.
- Cancelled Telegram turns skip outbound `sendMessage`, complete as
  `status: "cancelled"`, append signed `telegram.delivery`, and use
  `TelegramProcessingOutcome::Cancelled` so in-progress reactions clear.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_cancelled_turn_completion_suppresses_reply_and_records_cancelled_delivery -- --nocapture`: failed first because the completion status was still `sent`, then passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cancelled completion outcome | `PARTIAL` | Zaion now avoids stale post-cancel Telegram replies and records a signed cancelled outcome, but still needs Hermes-style owned async task cancellation and bounded unwind. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Add bounded join/unwind around background Telegram turn execution.
- Propagate cancelled completion semantics into delegated/remote execution and
  broader platform adapters.

---

## 2026-05-28 Telegram Interruptible Wake Runner Stage [PARTIAL SLICE]

This stage gives Zaion's live Telegram receive loop an initial control lane
while wake/model/tool execution is active. It does not promote the whole-plan
verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `run_telegram_loop` now uses `TelegramTaskRunner::background(...)` for live
  wake turns.
- The active turn's `StreamCallback` cancel handle is registered before
  background execution starts, so `/stop` and the running wake share one
  cooperative cancellation flag.
- The receive loop drains background completions, writes the existing signed
  `telegram.delivery` audit, unregisters active markers, and releases queued
  follow-up messages.
- A test-only held runner proves `/stop` dispatch can happen while an active
  turn remains in flight.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_task_runner_accepts_stop_while_active_turn_is_in_flight -- --nocapture`: failed first because the runner API was absent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_stop_command -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 22 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram interruptible wake runner | `PARTIAL` | Zaion now has receive-loop progress during active wake work and a shared cancel flag, but still lacks Hermes' owned async task cancellation and bounded unwind semantics. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Turn cooperative cancellation into a production cancellation outcome with
  bounded task join/unwind and deterministic command response ordering.
- Continue Telegram/channel parity across media batching/cache, retry
  behavior, and delegated/remote propagation.

---

## 2026-05-28 Telegram Stop Active Wake Cancel Stage [PARTIAL SLICE]

This stage connects Telegram `/stop` to Zaion's existing wake cancellation
flag. It does not promote the whole-plan verdict: latest-Hermes parity remains
`PARTIAL`.

Zaion implementation:

- Telegram processing registry entries can now store the active wake
  `StreamCallback` cancel handle.
- Live Telegram wake setup registers that cancel handle before calling
  `cmd_wake_with_request`.
- `/stop` sets all registered active wake cancel flags to `true`, records
  `cancel_requested` on the signed command delivery audit, and still supports
  marker-only reaction cleanup.
- Normal success/failure completion still unregisters the active source
  marker.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_stop_command_requests_active_wake_cancellation -- --nocapture`: failed first because `register_active_turn` did not exist, then passed.
- `cargo test -j 1 -p zaion-cli telegram_stop_command_clears_registered_in_progress_reactions -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_processing_reaction_completion_clears_on_cancelled_when_enabled -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram `/stop` active wake cancel hook | `PARTIAL` | Zaion now shares the TUI/wake cancel primitive with Telegram `/stop`, but the synchronous Telegram polling loop still needs a concurrent control lane before it can match Hermes' active task cancellation semantics. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Make live Telegram processing interruptible while the receive loop continues
  accepting control commands.
- Continue Telegram/channel parity across media batching/cache, retry
  behavior, and delegated/remote propagation.

---

## 2026-05-28 Telegram Stop Command Reaction Cleanup Stage [PARTIAL SLICE]

This stage connects the previous cancellation reaction primitive to a real
Telegram control command. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Live Telegram processing reactions register active source messages after the
  eyes reaction is successfully posted.
- Normal success/failure completion unregisters active source messages.
- `/stop` is now a stable Telegram command-graph command with a signed
  `telegram.command.stop` receipt and safe non-turn response.
- The `/stop` command clears all registered in-progress reactions by sending
  `setMessageReaction(..., None)`.
- The `/stop` command delivery audit records `telegram_reactions:
  ["cleared"]` and remains parented to its command receipt.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_stop_command_clears_registered_in_progress_reactions -- --nocapture`: failed first because the registry and `/stop` clear hook did not exist, then passed.
- `cargo test -j 1 -p zaion-cli telegram_processing_reaction_completion_clears_on_cancelled_when_enabled -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_command_reply_preserves_topic_metadata_for_send -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram `/stop` reaction cleanup | `PARTIAL` | Zaion now has a real command-state hook that clears registered processing reactions, but still lacks Hermes-style async live task cancellation while wake/model/tool execution is running. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Implement real live Telegram interrupt/cancel propagation into active
  wake/model/tool execution.
- Continue Telegram/channel parity across media batching/cache, retry
  behavior, and delegated/remote propagation.

---

## 2026-05-28 Telegram Cancellation Reaction Clear Stage [PARTIAL SLICE]

This stage adds the cancellation cleanup primitive behind Telegram reaction
lifecycle handling. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Telegram live reaction handling now flows through explicit
  start/complete helpers and a `TelegramProcessingOutcome` enum.
- `TelegramProcessingOutcome::Cancelled` clears the in-progress reaction by
  calling `set_message_reaction(..., None)`.
- Cancellation cleanup records a `cleared` lifecycle audit label in the local
  reaction event list.
- Existing success/failure/default-disabled reaction paths continue through
  the same helper surface.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_processing_reaction_completion_clears_on_cancelled_when_enabled -- --nocapture`: failed first because the helper/outcome was missing, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram cancellation reaction cleanup | `PARTIAL` | Zaion has the clear primitive, outcome test, and `/stop` command cleanup hook, but still needs real mid-flight cancellation during wake/model/tool processing. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Extend the real live Telegram `/stop` command into mid-flight
  wake/model/tool cancellation.
- Continue Telegram/channel parity across media batching/cache, retry
  behavior, and delegated/remote propagation.

---

## 2026-05-28 Telegram Processing Reactions Stage [PARTIAL SLICE]

This stage adds latest-Hermes processing lifecycle reactions for Telegram. It
does not promote the whole-plan verdict: latest-Hermes parity remains
`PARTIAL`.

Zaion implementation:

- `TelegramAdapter` can now call Telegram Bot API `setMessageReaction` with
  emoji reaction objects.
- Live Telegram wake processing reads `TELEGRAM_REACTIONS` and keeps reactions
  disabled by default.
- When enabled, live polling sets an in-progress reaction before model/tool
  processing and swaps it to a success or failure reaction after reply
  delivery.
- Signed `telegram.delivery` events include `telegram_reactions` audit labels,
  preserving proof-chain visibility for the channel lifecycle signal.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_mark_processing_lifecycle_when_enabled -- --nocapture`: failed first because no `setMessageReaction` calls were made, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-adapters telegram_set_message_reaction_posts_bot_api_payload -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 22 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 19 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram processing reactions | `PARTIAL` | Zaion now covers opt-in start/success/failure reactions through live polling and signed delivery evidence, but cancellation cleanup and broader channel/media behavior remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Add cancellation/interrupt reaction clearing and carry lifecycle diagnostics
  into delegated/remote execution paths.
- Continue Telegram/channel parity across media batching/cache, retry
  behavior, and multi-platform equivalents.

---

## 2026-05-28 Telegram Observation-Only Group Memory Stage [PARTIAL SLICE]

This stage adds latest-Hermes observation-only handling for unmentioned group
messages. It does not promote the whole-plan verdict: latest-Hermes parity
remains `PARTIAL`.

Zaion implementation:

- `ChannelProfile` now carries optional Telegram
  `observe_unmentioned_group_messages`, with serde defaults for old
  `channels.toml` files.
- `zaion tg setup --token ... --observe-unmentioned-group-messages true`
  writes durable observation policy through the Telegram profile; the legacy
  `--ingest-unmentioned-group-messages` alias is accepted.
- `TelegramAccessPolicy::from_store` reads durable policy, merges env
  `ZAION_TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES`, and falls back to
  legacy env `ZAION_TELEGRAM_INGEST_UNMENTIONED_GROUP_MESSAGES`.
- `zaion tg doctor` and JSON status expose the effective observe flag.
- Plain group/supergroup text can become `ObserveOnly` only after hard gates
  and dispatch triggers are checked, and only for explicitly allowlisted group
  chats.
- A live fake-API poll proves the adapter writes signed `telegram.observed`
  with shared group thread id, source hash, attributed content, and Telegram
  metadata while sending no typing/reply and no denial/delivery events.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_observe_unmentioned_groups_from_env -- --nocapture`: failed first because policy did not read observe env, then passed.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 18 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 20 tests after adding mention-pattern live dispatch evidence.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram observation-only group memory | `PARTIAL` | Zaion now covers the narrow Hermes observation-only group memory gate in durable policy, diagnostics, env fallback, dispatch, and live signed evidence, but broader channel semantics remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry observation-only diagnostics into broader gateway/channel and
  delegated/remote execution paths.
- Continue Telegram/channel parity across media batching, reactions, retry
  behavior, and multi-platform equivalents.

---

## 2026-05-28 Telegram Mention Patterns Stage [PARTIAL SLICE]

This stage adds latest-Hermes `mention_patterns` regex wake dispatch. It does
not promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `ChannelProfile` now carries optional Telegram `mention_patterns`, with
  serde defaults for old `channels.toml` files.
- `zaion tg setup --token ... --mention-patterns ...` writes durable regex
  wake policy through the Telegram profile.
- `TelegramAccessPolicy::from_store` reads durable mention patterns, merges
  them with `ZAION_TELEGRAM_MENTION_PATTERNS`, and dedupes the effective list.
- `zaion tg doctor` and JSON status expose mention patterns.
- Plain group/supergroup text matching a configured case-insensitive regex
  can dispatch without a direct `@zaion_bot` mention and keeps the prompt
  unchanged.
- The gate order remains Hermes-aligned: allowed chat/topic, ignored-thread,
  and explicit other-bot denials still happen before regex wake dispatch.
- A live fake-API poll now proves regex-matched plain group text performs
  `getUpdates`, sends typing and reply requests, appends signed
  `telegram.delivery` with real chat/topic metadata, and avoids
  `telegram.denied`.

Verification:

- `cargo test -j 1 -p zaion-cli mention_pattern -- --nocapture`: failed first because `TelegramAccessPolicy` had no `mention_patterns` field and `ChannelStore::upsert_telegram_profile_with_policy` lacked the extra argument, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_mention_pattern_dispatches_plain_group_text -- --nocapture`: passed, adding live fake-API evidence over the existing production path.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 16 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 20 tests.
- `cargo fmt -p zaion-cli --check`: passed after formatting.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram mention-pattern regex wake policy | `PARTIAL` | Zaion now covers the narrow Hermes regex wake gate in durable policy, diagnostics, env merge, and dispatch, but broader group/channel semantics remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Evaluate Hermes-style observation-only group memory.
- Carry mention-pattern diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-28 Telegram Free-Response Chats Live Poll Stage [PARTIAL SLICE]

This stage adds latest-Hermes `free_response_chats` dispatch. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `ChannelProfile` now carries optional Telegram `free_response_chats`, with
  serde defaults for old `channels.toml` files.
- `zaion tg setup --token ... --free-response-chats ...` writes durable
  free-response policy through the Telegram profile.
- `TelegramAccessPolicy::from_store` reads durable free-response chats, merges
  them with `ZAION_TELEGRAM_FREE_RESPONSE_CHATS`, and dedupes the effective
  list.
- `zaion tg doctor` and JSON status expose free-response chats.
- Plain group/supergroup text in an approved free-response chat dispatches
  without a direct `@zaion_bot` mention and keeps the prompt unchanged.
- A live fake-API poll proves the adapter sends typing/reply requests and
  writes signed delivery metadata for plain free-response group text.
- The gate order remains Hermes-aligned: allowed chat/topic and ignored-thread
  denials still happen before free-response dispatch.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_group_free_response_chat_dispatches_plain_text_without_mention -- --nocapture`: failed first because `TelegramAccessPolicy` had no `free_response_chats` field, then passed.
- `cargo test -j 1 -p zaion-cli telegram_group_free_response_chat_still_respects_hard_group_gates -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_free_response_chat_dispatches_plain_group_text -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_free_response_chats_from_channel_profile -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram free-response chat policy | `PARTIAL` | Zaion now covers the narrow Hermes free-response group dispatch gate in durable policy, diagnostics, dispatch, and live polling, but broader group/channel semantics remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Evaluate Hermes-style configurable mention patterns and observation-only
  group memory.
- Carry free-response delivery/denial diagnostics into broader
  gateway/channel and delegated/remote execution paths.

---

## 2026-05-28 Telegram Ignored Threads Live Poll Stage [PARTIAL SLICE]

This stage adds latest-Hermes `ignored_threads` gating. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `ChannelProfile` now carries optional Telegram `ignored_threads`, with serde
  defaults for old `channels.toml` files.
- `zaion tg setup --token ... --ignored-threads ...` writes durable ignored
  thread/topic policy through the Telegram profile.
- `TelegramAccessPolicy::from_store` reads durable ignored threads, merges them
  with `ZAION_TELEGRAM_IGNORED_THREADS`, and dedupes the effective list.
- `zaion tg doctor` and JSON status expose ignored threads.
- Group/supergroup messages in ignored Telegram topics are silently denied as
  `telegram_thread_ignored`, even if the text directly mentions `@zaion_bot`.
- A live fake-API poll proves the adapter sends no typing/reply request and
  only writes signed denial metadata for ignored-thread direct mentions.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_group_ignored_thread_is_denied_even_with_direct_mention -- --nocapture`: failed first because `TelegramDispatchReason::GroupThreadIgnored` did not exist, then passed after adding the policy gate.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: failed first because setup/doctor did not persist or print `ignored_threads`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ignored_thread_denies_direct_mention_silently -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram ignored thread/topic policy | `PARTIAL` | Zaion now covers the narrow Hermes ignored-thread hard gate in durable policy, diagnostics, and live polling, but broader group/channel semantics remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Evaluate Hermes-style `free_response_chats`, configurable mention patterns,
  and observation-only group memory.
- Carry ignored-thread denial diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-28 Telegram Guest-Mode Live Poll Stage [PARTIAL SLICE]

This stage adds live fake-API evidence for the latest-Hermes `guest_mode`
bypass. It does not promote the whole-plan verdict: latest-Hermes parity
remains `PARTIAL`.

Zaion implementation:

- A real one-poll Telegram adapter path now proves a non-allowlisted
  supergroup message can dispatch when durable `guest_mode=true` and the text
  directly mentions the configured bot with `@zaion_bot`.
- The proof exercises `getUpdates`, model/tool execution, `sendChatAction`,
  `sendMessage`, prompt mention stripping, and signed `telegram.delivery`.
- `telegram.delivery` payloads now copy real Telegram chat/topic/update/
  message/reply metadata, matching the audit metadata already present on
  `telegram.denied`.
- A companion live poll proves ordinary group replies outside the allowlist
  still deny silently as `telegram_group_not_allowed`, send no typing/reply
  request, and do not append `telegram.delivery`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_guest_mode_allows_direct_mention_outside_group_allowlist -- --nocapture`: failed first because `telegram.delivery.telegram_chat_id` was `Null`, then passed after delivery events copied Telegram metadata.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_guest_mode_denies_group_reply_outside_allowlist -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram guest-mode live poll evidence | `PARTIAL` | Zaion now proves the narrow Hermes guest-mode direct-mention bypass through live polling and signed delivery metadata, but broader group policy semantics and multi-channel propagation remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Evaluate Hermes-style `free_response_chats`, `ignored_threads`,
  observation-only group memory, and configurable mention patterns.
- Carry guest-mode delivery/denial diagnostics into broader gateway/channel
  and delegated/remote execution paths.

---

## 2026-05-28 Telegram Guest-Mode Direct Mention Bypass Stage [PARTIAL SLICE]

This stage adds the narrow latest-Hermes `guest_mode` bypass. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `ChannelProfile` now carries optional Telegram `guest_mode`, with serde
  defaults for old `channels.toml` files.
- `zaion tg setup --token ... --guest-mode true` writes the durable guest-mode
  value through the Telegram profile.
- `TelegramAccessPolicy::from_store` reads durable `guest_mode`, and
  `zaion tg doctor` reports the effective value.
- Group/supergroup messages outside the allowed chat list can dispatch only
  when `guest_mode` is true and the current bot is directly addressed with an
  explicit `@bot` mention.
- Ordinary group replies outside the allowed chat list remain denied as
  `telegram_group_not_allowed`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_guest_mode_allows_direct_bot_mention_outside_group_allowlist -- --nocapture`: failed first because `TelegramAccessPolicy` had no `guest_mode` field, then passed.
- `cargo test -j 1 -p zaion-cli telegram_guest_mode_does_not_allow_group_reply_outside_allowlist -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_guest_mode_from_channel_profile -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli setup_gateway_collects_telegram_owner_allowlist_and_home_channel -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram guest-mode direct mention bypass | `PARTIAL` | Zaion now covers the narrow Hermes guest-mode direct-mention bypass in durable policy and dispatch, but still needs live fake-API proof plus broader group policy semantics. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Evaluate Hermes-style `free_response_chats`, `ignored_threads`,
  observation-only group memory, and configurable mention patterns.
- Add live fake-API polling evidence for guest-mode allowed and denied events.
- Continue carrying Telegram policy diagnostics into broader gateway/channel
  and delegated/remote execution paths.

---

## 2026-05-28 Telegram Durable Chat/Topic Policy Config Stage [PARTIAL SLICE]

This stage productizes the previously verified group policy gate. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `ChannelProfile` now carries optional `allowed_chats` and `allowed_topics`
  values for Telegram channel entries, with serde defaults for old
  `channels.toml` files.
- `zaion tg setup --token ... --allowed-chats ... --allowed-topics ...` writes
  the durable group chat/topic policy values through the Telegram profile.
- `TelegramAccessPolicy::from_store` merges durable channel policy with
  `ZAION_TELEGRAM_ALLOWED_CHATS` and `ZAION_TELEGRAM_ALLOWED_TOPICS`, then
  dedupes the effective allowlists.
- `zaion tg doctor` reports the effective allowed chat/topic values.
- Existing live Telegram allowed-topic denial and group dispatch tests remain
  green.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_group_gates_from_channel_profile -- --nocapture`: failed first because `upsert_telegram_profile` and `ChannelProfile` had no durable group policy fields, then passed.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 11 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 14 tests.
- `cargo fmt -p zaion-cli --check`: passed after formatting.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram durable allowed chat/topic config | `PARTIAL` | Zaion now has config-file and setup exposure for the verified group policy gate, but the broader Hermes group-policy model is still not complete. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Evaluate Hermes-style `group_allowed_chats`, `free_response_chats`,
  `guest_mode`, `ignored_threads`, observation-only group memory, and
  configurable mention patterns.
- Continue carrying Telegram policy diagnostics into broader gateway/channel
  and delegated/remote execution paths.

---

## 2026-05-28 Telegram Allowed Chat/Topic Gate Stage [PARTIAL SLICE]

This stage adds a latest-Hermes-aligned group policy gate. It does not promote
the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `TelegramAccessPolicy` now carries `group_allowed_chats` and
  `allowed_topics` loaded from `ZAION_TELEGRAM_ALLOWED_CHATS` and
  `ZAION_TELEGRAM_ALLOWED_TOPICS`.
- Group/supergroup messages outside the allowed chat set are silently denied
  as `telegram_group_not_allowed`.
- Group/supergroup messages outside the allowed topic set are silently denied
  as `telegram_topic_not_allowed`.
- Missing Telegram group topic ids match General topic `1`, following Hermes'
  latest source behavior for forum topic filtering.
- A fake-API live poll proves a bot mention in an allowlisted group but a
  disallowed topic produces only `telegram.denied`, keeps real metadata, and
  sends no typing/reply request or `telegram.delivery`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_group_allowed_chat_and_topic_can_dispatch_mention -- --nocapture`: failed first because the policy had no group/topic gate fields, then passed.
- `cargo test -j 1 -p zaion-cli telegram_group_disallowed_topic_is_denied_even_with_mention -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_group_disallowed_chat_is_denied_even_with_mention -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_group_allowed_topic_gate_denies_other_topics_silently -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 14 tests.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 11 tests.
- `cargo fmt -p zaion-cli --check`: passed after formatting.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram allowed chat/topic gate evidence | `PARTIAL` | Zaion now has verified live gate behavior for group chat/topic allowlists, but the surface is env-only and still narrower than Hermes' full group policy model. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Add durable config/onboarding exposure for allowed chats/topics and evaluate
  Hermes-style `group_allowed_chats`, `free_response_chats`, `guest_mode`,
  `ignored_threads`, observation-only group memory, and configurable mention
  patterns.
- Continue carrying Telegram policy diagnostics into broader gateway/channel
  and delegated/remote execution paths.

---

## 2026-05-27 Telegram Denied Metadata Audit Stage [PARTIAL SLICE]

This stage adds denial metadata audit evidence. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Live Telegram `telegram.denied` events now copy real Telegram metadata from
  the inbound update when available.
- Denial events can expose chat id/type, update id, message id, topic/thread
  id, reply-to id, and reply-to text.
- The focused fake-API regression proves a supergroup message without a bot
  trigger is denied silently while the signed denial event preserves the
  concrete Telegram context operators need to debug group policy.
- The denial remains separate from normal delivery and wake proof state.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_group_noise_is_denied_from_real_update_metadata -- --nocapture`: failed first because `telegram.denied.telegram_chat_id` was `Null`, then passed after denied events copied Telegram metadata.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 13 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram denial metadata audit evidence | `PARTIAL` | Denied/noise events now keep real Telegram chat/topic/reply context, making the next group-policy parity slices easier to verify and debug. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand live Telegram/channel parity across group chat allowlists, allowed
  topics, guest-mode mention bypass, configurable mention patterns,
  observation-only group memory, batching, media, reactions, and retry
  semantics.
- Carry denied/delivery metadata diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-27 Telegram Access-Gate Markdown Parse Fallback Stage [PARTIAL SLICE]

This stage adds access-gate Markdown retry evidence. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Live Telegram access-gate denial replies now request `MarkdownV2`
  formatting through the existing adapter delivery path.
- Telegram Markdown entity parse failures now trigger the adapter's existing
  plain-text retry fallback on the access-denial reply path.
- The signed denial report preserves `parse_mode = "MarkdownV2"`, records
  `markdown_v2_plain_text_retry`, and captures the successful retried Telegram
  message id.
- The focused regression verifies the retry removes `parse_mode`, restores the
  unescaped visible denial text, and writes the fallback evidence to
  `telegram.denied`.
- Access-denial events remain access-gate diagnostics with
  `reason = "sender_not_in_telegram_allowlist"`; they do not append a normal
  `telegram.delivery` event or fabricate wake proof state.
- Group-noise denials stay silent as before.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_access_denial_markdown_parse_error_retries_plain_text_and_reports_fallback -- --nocapture`: failed first because access-denial replies did not request MarkdownV2 and only one send occurred, then passed after enabling MarkdownV2 on the access-gate reply path.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 13 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 18 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram access-gate Markdown parse fallback reporting | `PARTIAL` | Access-denial replies now prove MarkdownV2 parse-error recovery and signed ledger-visible fallback reporting; broader channel policy breadth remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand live Telegram/channel parity across mention/allowlist depth,
  batching, media, reactions, retry semantics, and topic/reply fallback
  combinations.
- Carry access-gate/command/wake delivery diagnostics into broader
  gateway/channel and delegated/remote execution paths.

---

## 2026-05-27 Telegram Command Markdown Parse Fallback Stage [PARTIAL SLICE]

This stage adds command-graph Markdown retry evidence. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Live Telegram slash-command quick replies handled by
  `TelegramCommandGraph` now request `MarkdownV2` formatting through the
  existing adapter delivery path.
- Telegram Markdown entity parse failures now trigger the adapter's existing
  plain-text retry fallback on the command reply path.
- The delivery report preserves `parse_mode = "MarkdownV2"`, records
  `markdown_v2_plain_text_retry`, and captures the successful retried Telegram
  message id.
- The focused regression verifies the retry removes `parse_mode`, restores the
  unescaped visible command reply text, and writes the fallback evidence to
  `telegram.delivery`.
- Command delivery stays a command-graph diagnostic with
  `runtime = "telegram.command_graph"`, `status = "command_sent"`, and the
  command receipt parent edge; it remains separate from wake `turn.proof`.
- Access-denial replies stay on the existing plain-text path for this stage.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_command_markdown_parse_error_retries_plain_text_and_reports_fallback -- --nocapture`: failed first because command replies did not request MarkdownV2 and only one send occurred, then passed after enabling MarkdownV2 on the command reply path.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 12 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 18 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram command Markdown parse fallback reporting | `PARTIAL` | Command deliveries now prove MarkdownV2 parse-error recovery and ledger-visible fallback reporting; media/reaction/retry policy breadth remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand live Telegram/channel parity across mention/allowlist depth,
  batching, media, reactions, retry semantics, access-denial formatting, and
  topic/reply fallback combinations.
- Carry command/wake delivery diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-27 Telegram Wake Markdown Parse Fallback Stage [PARTIAL SLICE]

This stage adds live wake-path Markdown retry evidence. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Normal live Telegram wake replies now request `MarkdownV2` formatting through
  the existing adapter delivery path.
- Telegram Markdown entity parse failures now trigger the adapter's existing
  plain-text retry fallback on the live wake path.
- The delivery report preserves `parse_mode = "MarkdownV2"`, records
  `markdown_v2_plain_text_retry`, and captures the successful retried Telegram
  message id.
- The focused regression verifies the retry removes `parse_mode`, restores the
  unescaped visible reply text, and writes the fallback evidence to
  `telegram.delivery`.
- Command quick replies and access-denial replies stay on the existing
  plain-text paths for this stage.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_wake_markdown_parse_error_retries_plain_text_and_reports_fallback -- --nocapture`: failed first because live wake replies did not retry after Telegram's Markdown parse error, then passed after enabling MarkdownV2 on the wake reply path.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 11 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 18 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram wake Markdown parse fallback reporting | `PARTIAL` | Normal wake deliveries now prove MarkdownV2 parse-error recovery and ledger-visible fallback reporting; command/media/reaction/retry policy breadth remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Extend Markdown/retry diagnostics to command replies and broader channel
  adapters.
- Expand live Telegram/channel parity across mention/allowlist depth,
  batching, media, reactions, retry semantics, and topic/reply fallback
  combinations.
- Carry wake delivery diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-27 Telegram Wake Mention Source-Hash Fallback Stage [PARTIAL SLICE]

This stage closes the wake-path follow-up to the command-reply diagnostic path.
It does not promote the whole-plan verdict: latest-Hermes parity remains
`PARTIAL`.

Zaion implementation:

- Live Telegram group mention dispatch now recomputes `source_hash` after the
  bot mention has been stripped and the actual wake prompt is known.
- The canonical wake envelope now uses the same stripped prompt and matching
  `source_hash`, avoiding raw-message hash mismatch after `@zaion_bot`
  removal.
- Denied/noise paths still keep the original raw-message hash for audit
  fidelity.
- Fake-API coverage now proves stale topic reply-anchor fallback reporting for
  normal wake replies, not only command quick replies.
- The wake fallback delivery remains a normal wake delivery with
  `runtime = "phase8b.unified_wake"`, `status = "sent"`,
  `thread_reply_anchor_retry`, and successful Telegram message id `881`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_wake_reply_stale_topic_anchor_fallback_is_recorded -- --nocapture`: passed, 1 test.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 10 tests.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram wake mention source-hash canonicalization | `PARTIAL` | Normal wake dispatch now has a source-hash/envelope match after mention stripping, but broader mention and group policy parity remains open. |
| Telegram reply fallback reporting | `PARTIAL` | Stale topic reply fallback reporting now covers command and wake paths; richer retry and topic semantics remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand live Telegram/channel parity across mention/allowlist depth,
  batching, media, Markdown/reactions, retry semantics, and topic/reply
  fallback beyond the verified command and wake slices.
- Carry command/wake delivery diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-27 Telegram Command-Graph Delivery Fallback Stage [PARTIAL SLICE]

This stage closes the interrupted command-reply diagnostic path. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- Live Telegram slash-command replies handled by `TelegramCommandGraph` now
  append a `telegram.delivery` event alongside the command receipt.
- Command delivery payloads use `runtime = "telegram.command_graph"` and
  `status = "command_sent"` or `command_send_failed`.
- Command replies remain non-turn receipts and do not fabricate a `turn.proof`;
  normal wake deliveries keep `phase8b.unified_wake`.
- Command delivery events set `parent_event_id` to the command receipt and
  include `command_receipt_event_id`, preserving a direct receipt-to-delivery
  audit edge.
- Fake-API coverage proves stale topic reply-anchor fallback reporting: the
  first `sendMessage` attempt with topic/reply metadata fails, the retry
  succeeds without the stale anchor, and `telegram.delivery.delivery_report`
  records `thread_reply_anchor_retry` plus the successful Telegram message id.

Verification:

- `cargo test -p zaion-cli telegram_live_poll_stale_topic_reply_fallback_is_recorded_in_delivery_report -- --nocapture`: failed first on the wrong wake runtime label, failed again while delivery lacked a parent command receipt edge, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 9 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram command delivery evidence | `PARTIAL` | Command quick replies now have explicit delivery diagnostics and fallback reporting without polluting wake proof semantics. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand live Telegram/channel parity across mention/allowlist depth,
  batching, media, Markdown/reactions, retry semantics, and topic/reply
  fallback beyond command quick replies.
- Carry command/delivery diagnostics into broader gateway/channel and
  delegated/remote execution paths.

---

## 2026-05-27 Telegram Source-Bound Proof and Receive Metadata Stage [PARTIAL SLICE]

This stage hardens the Telegram live polling proof path and closes the first
real-update metadata follow-up. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `telegram.delivery` proof extraction now decodes candidate Telegram
  `turn.proof` events and follows `user_event_id` to the exact
  `channel.received` event before attaching a proof trace.
- The proof trace must match the current message's `channel_id`, `thread_id`,
  and `source_hash`, so a wake failure cannot inherit an older same-thread
  proof, receipt ids, or storage receipt summaries.
- `TelegramAdapter.receive(...)` now preserves real update metadata for
  chat type, Telegram chat/update/message ids, message topic/thread id, and
  reply-to id/text.
- `run_telegram_poll_once(...)` has live fake-API coverage for a `supergroup`
  message without a bot trigger, proving real adapter metadata drives
  `telegram.denied` and prevents typing/reply sends.
- `runtime_delivery_result_to_value(...)` now carries `resolved_addrs` into API
  runtime webhook delivery JSON.

Verification:

- `cargo test -p zaion-cli telegram_live_wake_failure_does_not_inherit_prior_thread_proof -- --nocapture`: failed first on stale proof inheritance, then passed.
- `cargo test -p zaion-cli telegram_live_ -- --nocapture`: passed with `CARGO_BUILD_JOBS=1` / `cargo test -j 1`.
- `cargo test -p zaion-adapters telegram_receive_preserves_topic_and_reply_metadata -- --nocapture`: failed first on missing metadata, then passed.
- `cargo test -p zaion-cli telegram_live_poll_group_noise_is_denied_from_real_update_metadata -- --nocapture`: passed.
- `cargo test -p zaion-cli api_runtime_delivery_result_preserves_resolved_addrs -- --nocapture`: passed.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_large_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: passed.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram proof binding and metadata | `PARTIAL` | Live Telegram delivery traces are now source-bound and real update metadata drives one group-noise denial path, but this is not full Hermes channel parity. |
| Gateway delivery diagnostics | `PARTIAL` | API runtime delivery JSON now exposes resolved addresses; broader gateway delivery semantics remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand live Telegram/channel parity across mention/allowlist depth,
  batching, media, Markdown/reactions, retry semantics, and topic/reply
  fallback.
- Carry source-bound proof/storage summaries through delegated execution,
  remote sandbox paths, and broader gateway/channel adapters.

---

## 2026-05-27 Telegram Live Polling Storage Receipt E2E Stage [PARTIAL SLICE]

This stage proves the live polling storage receipt path that was previously
open after the service/channel receipt-summary work. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `run_telegram_loop(...)` now delegates each inbound message to
  `process_live_telegram_message_once(...)`, preserving the production
  forever-polling loop while making the live handler directly testable.
- Tests can run one real `TelegramAdapter.receive(...)` batch through
  `run_telegram_poll_once(...)` and a test-only Telegram API base URL override.
- The one-poll fake Telegram API covers `getUpdates`, `sendChatAction`, and
  `sendMessage`.
- The wake turn executes native `fs_search` with output large enough to persist
  under workspace-visible `.zaion/tool-results`.
- The `telegram.delivery` ledger event is verified to carry
  `tool_result_storage_receipt_count == 1` and the corresponding storage
  receipt summary for the native tool call.

Verification:

- `cargo fmt -p zaion-cli --check`: passed.
- `cargo test -p zaion-cli telegram_live_ -- --nocapture`: passed after a
  broad parallel run first hit rustc OOM/stack-overrun during compilation; the
  same filter passed with `CARGO_BUILD_JOBS=1` / `cargo test -j 1`.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_large_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Telegram live polling storage receipt E2E | `PARTIAL` | The live polling path now has proof beyond simulation for persisted large native tool output, but this is not full Hermes gateway/channel parity. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Expand Telegram/channel parity beyond preserved real update metadata and one
  verified group-noise denial path: bot mention trigger context,
  allowlist/group nuances, batching, media, Markdown/reactions, retry behavior,
  and topic/reply fallback.
- Carry equivalent receipt summaries through delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

---

## 2026-05-26 Service Wake Tool-Result Storage Receipt Summary Stage [PARTIAL SLICE]

This stage extends verified local wake response payloads with persisted
tool-result storage receipt summaries. It does not promote the whole-plan
verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `receipt_join.rs` now provides `tool_result_storage_receipts(...)`, a shared
  helper that reads returned `tool.receipt` ids and summarizes only receipts
  with non-null `tool_result_storage`.
- MCP HTTP wake responses, API `/v1/runs` wake responses, ACP stdio wake
  results, webhook synchronous wake `agent_trigger` results, and Telegram
  delivery payloads return `tool_result_storage_receipts` and
  `tool_result_storage_receipt_count`.
- No-storage local turns return stable empty arrays/count `0`; `tg simulate
  --no-llm` writes the same default fields.
- ACP stdio protocol coverage includes a non-empty mock storage receipt so
  backend/environment binding summaries are proven serializable through JSON.
- True large-output local wake E2E now covers MCP HTTP wake, API `/v1/runs`,
  webhook synchronous `agent_trigger`, and ACP stdio wake. Each path executes a
  native `fs_search` call large enough to persist tool output, returns a
  non-empty `tool_result_storage_receipts` array/count `1`, and verifies the
  stored output file exists under workspace-visible `.zaion/tool-results`.
- True large-output local wake E2E now also covers `zaion tg simulate`
  delivery, including the visible `tool_storage_count     : 1` trace and the
  persisted storage receipt summary written to the `telegram.delivery` ledger
  event.

Verification:

- `cargo test -p zaion-cli tool_result_storage_receipts_summarizes_persisted_storage_and_environment_binding -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_wake_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli api_create_run_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli api_create_run_wake_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-a2a acp_stdio_create_run_can_route_through_injected_wake_runtime -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_large_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed after failing first when the test used an absolute `fs_search` path outside the tool workspace boundary, then passed with the command run from the temporary workspace and `path="."`.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: passed.
- `cargo test -p zaion-adapters test_agent_handler_can_be_attached -- --nocapture`: passed.
- `cargo test -p zaion-cli --test cli_stable_surface doctor_source_gate_locks_shared_receipt_join_helper_for_service_wake_surfaces -- --nocapture`: passed.
- `cargo fmt -p zaion-a2a -p zaion-adapters -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local service/channel storage receipt summaries | `PARTIAL` | Verified local wake consumers now see storage receipt arrays/counts, and MCP HTTP/API/webhook/ACP plus `tg simulate` have true large-output non-empty E2E coverage; Telegram live polling storage receipt E2E is now covered separately, while richer Telegram semantics and non-local/delegated propagation remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Extend live Telegram behavior beyond the one-poll storage receipt proof.
- Carry equivalent summaries through delegated execution, remote sandbox paths,
  and broader gateway/channel adapters.

---

## 2026-05-26 Explicit Tool-Result Environment Identity Stage [PARTIAL SLICE]

This stage adds a named-backend identity hook to the persisted tool-result
contract. It does not promote the whole-plan verdict: latest-Hermes parity
remains `PARTIAL`.

Zaion implementation:

- `ToolResultStorageTarget` now exposes optional `environment_id()` and
  `environment_kind()` methods.
- `ToolResultMetadata` records optional environment identity/kind beside
  persisted path, storage root, byte counts, and truncation state.
- `HostToolResultStorageTarget::with_environment(...)` creates a host-backed
  target with a named backend identity.
- `maybe_store_tool_result_with_target(...)` copies target identity into
  persisted-output metadata.
- `WakeRequest` now carries optional `tool_result_environment_id` and
  `tool_result_environment_kind`.
- Wake constructs its host storage target through
  `wake_tool_result_storage_target(...)`, preserving explicit identity when
  supplied.
- Wake receipt `tool_result_storage_binding.environment` prefers explicit
  metadata identity/kind and retains the local fallback of
  `storage-root:<hash>` plus `storage_target`.

Verification:

- `cargo fmt -p zaion-runtime -p zaion-cli --check`: passed.
- `cargo test -p zaion-runtime tool_result_metadata_records_explicit_environment_identity_from_target -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_request_tool_result_environment_identity_reaches_host_storage_target -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_receipt_binding_prefers_explicit_environment_identity -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: passed.
- `cargo test -p zaion-runtime tool_result_large_output_can_spill_through_active_environment_storage_target -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Explicit persisted-output environment identity | `PARTIAL` | Wake can now preserve named backend identity when a caller supplies it; remote environment selection, delegated execution, and gateway/channel propagation remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Thread real Modal/Docker/Daytona/SSH backend ids into the callers that select
  those environments.
- Carry the same explicit identity through delegated execution and broader
  gateway/channel adapters.

---

## 2026-05-26 ACP/Webhook/Telegram Wake Receipt/Proof Propagation Stage [PARTIAL SLICE]

This stage extends the local receipt/proof join contract into the verified
service/channel response payloads. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- MCP HTTP `runtime_route=wake` responses now return `tool_receipt_ids`,
  `tool_receipt_count`, `tool_receipt_proof_join_event_id`,
  `tool_receipt_proof_join`, `tool_receipt_join_found`, and
  `tool_receipt_proof_hash_verified`.
- API `/v1/runs` wake responses now return the same receipt/proof join summary
  for tool-using turns.
- ACP stdio wake JSON-RPC results now return the same receipt/proof join
  summary.
- Webhook synchronous wake `agent_trigger` results now return the same
  receipt/proof join summary.
- Telegram live delivery traces and `zaion tg simulate` now return the same
  receipt/proof join summary.
- `tg simulate --no-llm` now writes explicit empty/default receipt/proof fields.
- Populated extractors decode `TurnProof`, find the signed
  `tool.receipt.proof_join` by exact `tool_receipt_ids` array membership, and
  verify the join's proof hash and proof event id.
- Direct MCP HTTP tool calls remain scoped as `receipt_only`.
- `crates/zaion-cli/src/commands/receipt_join.rs` provides the shared lookup
  helper reused by ACP, webhook, MCP/API, and Telegram surfaces.
- MCP HTTP and API run response builders now use the shared helper instead of
  private duplicate proof-join lookup/summary implementations.

Verification:

- `cargo fmt -p zaion-a2a -p zaion-cli -p zaion-adapters --check`: passed.
- `cargo test -p zaion-a2a acp_stdio_create_run_can_route_through_injected_wake_runtime -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_runtime_route_wake_joins_stable_turn_proof_chain -- --nocapture`: passed.
- `cargo test -p zaion-cli direct_mcp_http_call_executes_builtin_tool_with_signed_receipt -- --nocapture`: passed.
- `cargo test -p zaion-cli api_create_run_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: failed first on missing API receipt fields, then passed.
- `cargo test -p zaion-cli acp_create_run_executes_wake_runtime_and_returns_turn_proofs -- --nocapture`: passed.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_tool_call_exposes_receipt_proof_trace -- --nocapture`: failed first on missing Telegram `tool_receipt_count`, then passed.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: failed first on omitted no-LLM/default receipt fields, then passed.
- `cargo test -p zaion-cli --test cli_stable_surface doctor_source_gate_locks_shared_receipt_join_helper_for_service_wake_surfaces -- --nocapture`: failed first on private MCP/API helpers, then passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_parser_tool_call_records_permission_receipt -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local service/channel receipt propagation | `PARTIAL` | ACP stdio, webhook synchronous wake, Telegram delivery/simulate, MCP HTTP wake, and API runs can now see receipt ids and proof-join verification state; delegated, remote sandbox, and broader gateway/channel adapters still need the same contract. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry receipt/proof response summaries into delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.
- Keep `receipt_join.rs` as the shared service helper as additional response
  surfaces adopt the same contract.

---

## 2026-05-26 Delegation Receipt Trace Stage [PARTIAL SLICE]

This stage makes delegated proof records inspectable while keeping delegation
semantically separate from generic tool receipts. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `zaion agent receipt-trace <pid> <delegation-proof-event-id>` resolves a
  signed `delegation.proof` ledger event.
- The command recomputes `merge_receipt` from principal, delegate, task, scope,
  input hash, and output hash.
- The command verifies the stored A2A delegation message signature using the
  local principal key that created the proof.
- The Phase 8 surface regression now exercises
  `agent proof -> agent receipts -> agent receipt-trace` and asserts
  `merge_receipt_verified : yes`,
  `message_signature_valid: yes`, and
  `runtime_scope          : delegation_proof`.

Verification:

- `cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Delegation proof trace | `PARTIAL` | Zaion can now inspect a local signed delegation proof and verify its merge receipt plus A2A signature; live delegated execution and gateway/ACP/MCP propagation remain open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry receipt/proof traceability into live delegated execution and
  structured gateway/API/webhook/Telegram/ACP/MCP paths.
- Keep `delegation.proof` separate from `tool.receipt` unless a delegated path
  actually emits a tool execution receipt.

---

## 2026-05-25 Tool Receipt Trace Surfaces Stage [PARTIAL SLICE]

This stage makes the local receipt/proof join inspectable from the CLI, turn
trace, and native MCP diagnostics. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Zaion implementation:

- `zaion tool receipts <pid>` now prints local receipt ledger event ids.
- `zaion tool receipt-trace <pid> <receipt-event-id>` validates a
  `tool.receipt`, follows the signed `tool.receipt.proof_join` via receipt-id
  array membership, resolves the linked `turn.proof`, and verifies the
  normalized `TurnProof` hash.
- The beginner golden path exercises the operator flow and requires
  `join_found`, `proof_found`, and `proof_hash_verified` to be `yes`.
- `zaion turn trace <proof-event-id> --pid <pid>` reports receipt count, join
  presence, join-to-proof linkage, and join/proof hash match.
- Native MCP now registers `tool_receipt_trace`, a compact diagnostic tool for
  local receipt -> join -> proof hash verification.

Verification:

- `cargo test -p zaion-cli wake_parser_tool_call_records_permission_receipt -- --nocapture`: passed.
- `cargo test -p zaion-mcp tool_receipt_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli chat_executes_native_tool_call_without_mcp -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local receipt/proof trace surfaces | `PARTIAL` | Zaion now has local operator, turn-inspection, and MCP diagnostic paths for receipt-to-proof lookup and hash verification; non-local execution propagation remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry receipt/proof joins through delegated execution, remote sandbox
  runners, gateway, and MCP execution paths.

---

## 2026-05-25 Ledger Receipt Proof Join Lookup Stage [PARTIAL SLICE]

This stage adds the ledger lookup follow-up for the local receipt/proof join
work. It does not promote the whole-plan verdict: latest-Hermes parity remains
`PARTIAL`.

Zaion implementation:

- `crates/zaion-ledger/src/ledger.rs` adds
  `EventLedger::list_events_by_payload_string_array_contains(...)`.
- The helper lists newest events whose top-level payload array contains an
  exact string value, with SQL narrowing by namespace and event type and Rust
  payload parsing to avoid SQLite JSON1 dependence.
- `crates/zaion-ledger/src/tests.rs` proves
  `tool.receipt.proof_join` lookup by `tool_receipt_ids` membership returns
  newest exact matches while excluding scalar lookalikes and other event types.
- Adjacent scalar payload lookup coverage remains green.

Verification:

- `cargo test -p zaion-ledger test_list_events_by_payload_string_array_contains_returns_latest_exact_matches -- --nocapture`: failed first on the missing helper, then passed.
- `cargo test -p zaion-ledger test_list_events_by_payload_string_returns_latest_exact_matches -- --nocapture`: passed.
- `cargo test -p zaion-ledger -- --nocapture`: 30 passed.
- `cargo check -p zaion-ledger`: passed.
- `cargo fmt -p zaion-ledger -p zaion-types -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Ledger receipt/proof lookup | `PARTIAL` | The ledger now has a reusable receipt-id array-membership query for signed join events; local CLI, turn trace, and MCP diagnostic lookups exist, while non-local execution propagation remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry the same receipt/proof join contract through delegated execution,
  remote sandbox runners, gateway, and MCP execution paths.

---

## 2026-05-25 Wake Tool Receipt Proof Join Stage [PARTIAL SLICE]

This stage resolves the local append-only proof join follow-up from the
previous receipt/provenance work. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Tool-result storage evidence: `tools/tool_result_storage.py`.
- Tool-output cap evidence: `tools/tool_output_limits.py`.
- Tool execution handoff: `agent/tool_executor.py`.
- Active environment evidence: `tools/environments/base.py`.

Zaion implementation:

- `crates/zaion-types/src/event.rs` adds
  `EventType::ToolReceiptProofJoin` with wire string
  `tool.receipt.proof_join`.
- `crates/zaion-cli/src/commands/process/wake.rs` now appends a signed
  `tool.receipt.proof_join` event after `turn.proof` when signed tool receipt
  ids exist.
- The join event is parented to the `turn.proof` event and records receipt
  ids/count, proof event id/hash, answer/output/user event ids, lineage, and a
  deterministic `join_hash`.
- Turns without tool receipts do not write join events.
- `crates/zaion-types/tests/invariants.rs` locks the new event wire string.

Verification:

- `cargo test -p zaion-cli wake_tool_receipt_proof_join_event_links_receipts_to_turn_proof -- --nocapture`: failed first on missing join support, then passed.
- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: passed.
- `cargo test -p zaion-runtime turn_proof_records_tool_receipt_ids_in_lineage -- --nocapture`: passed.
- `cargo test -p zaion-types event -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Wake receipt/proof join | `PARTIAL` | Local wake now has append-only signed receipt-to-proof join events; continue with delegated, remote sandbox, gateway, and MCP execution bindings plus query ergonomics. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry the same receipt/proof join contract through delegated execution,
  remote sandbox runners, gateway, and MCP execution paths.
- Replace storage-root-derived local environment ids with real backend
  environment identities once non-local sandbox selection is wired.
- Keep local query surfaces aligned as the join contract expands beyond wake.

---

## 2026-05-25 Wake Tool Receipt Provenance Binding Stage [PARTIAL SLICE]

This stage builds on the persisted-output receipt metadata work. It does not
promote the whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Tool-result storage evidence: `tools/tool_result_storage.py`.
- Tool-output cap evidence: `tools/tool_output_limits.py`.
- Tool execution handoff: `agent/tool_executor.py`.
- Active environment evidence: `tools/environments/base.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/wake.rs` now builds a
  `tool_result_storage_binding` object for signed wake receipts whose tool
  output was persisted.
- The binding records environment identity derived from storage root,
  storage root/path, permission id/class/effect/sandbox scope, permission proof
  hash, principal/namespace/channel/thread provenance, parent output event id,
  tool identity, argument/output hashes, turn material, and a binding hash.
- `append_tool_receipts(...)` returns signed receipt event ids, and wake
  returns them in `RuntimeOutput.tool_receipt_ids`.
- `crates/zaion-runtime/src/turn_proof.rs` now carries `tool_receipt_ids` and
  `tool_receipt_count`; receipt ids are included in `event_lineage`.
- `crates/zaion-cli/src/commands/process_unified.rs` passes an empty receipt
  list for its current no-tool-receipt path.
- Receipt-side `turn_proof_event_id` and `turn_proof_hash` stay `null` because
  receipts are appended before `turn.proof`; the proof now links back to the
  receipts through append-only lineage.

Verification:

- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: failed first on missing `tool_result_storage_binding`, then passed.
- `cargo test -p zaion-runtime turn_proof_records_tool_receipt_ids_in_lineage -- --nocapture`: failed first on missing receipt fields, then passed.
- `cargo test -p zaion-runtime turn_proof -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_context -- --nocapture`: 4 passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed.
- `cargo fmt -p zaion-runtime -p zaion-cli --check`: passed after formatting.
- `cargo check -p zaion-runtime`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Wake persisted-output provenance | `PARTIAL` | Wake receipt/proof lineage now binds persisted output storage to permission, provenance, and turn material for local wake paths; continue with remote/delegated/gateway/MCP execution bindings and optional proof join events. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Carry the same storage/provenance binding through delegated execution,
  remote sandbox runners, gateway, and MCP execution paths.
- Replace storage-root-derived local environment ids with real backend
  environment identities once non-local sandbox selection is wired.

---

## 2026-05-25 Wake Tool Receipt Storage Metadata Stage [PARTIAL SLICE]

This stage connects the target-aware tool-result spill work to Zaion's signed
wake receipt ledger path. It does not promote the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Tool-result storage evidence: `tools/tool_result_storage.py`.
- Tool-output cap evidence: `tools/tool_output_limits.py`.
- Tool execution handoff: `agent/tool_executor.py`.
- Active environment evidence: `tools/environments/base.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/wake.rs` now preserves
  `ToolResultMetadata` on live tool execution records after per-result spill
  and aggregate turn-budget enforcement.
- Wake todo, native, and MCP success paths now retain storage metadata returned
  by `maybe_store_tool_result_with_target(...)`.
- `append_tool_receipts(...)` emits `tool_result_storage` in signed
  `tool.receipt` payloads when the result was persisted, recording schema,
  tool name, tool call id, stored/truncated flags, byte counts, path, and
  storage root.
- Receipt payloads keep the persisted-output preview out of the signed ledger
  payload, preserving compact provenance while retaining permission proof.

Verification:

- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: failed first on missing `tool_result_storage`, then passed.
- `cargo test -p zaion-cli wake_tool_context -- --nocapture`: 4 passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed.
- `cargo fmt -p zaion-cli --check`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Wake persisted-output receipts | `PARTIAL` | Wake receipts now preserve persisted full-output storage metadata alongside permission proof; continue by binding receipts to explicit environment identity, provenance chain, and turn-proof material across delegated/remote paths. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Bind persisted tool-result receipts to environment identity, permission
  scope, provenance, and turn-proof references.
- Carry the same receipt metadata through delegated, remote sandbox, gateway,
  and MCP execution paths.

---

## 2026-05-25 Structured Wake Caller Tool-Result Root Stage [PARTIAL SLICE]

This stage carries the active-environment-visible tool-result storage work
across the local structured wake caller set. It does not promote the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Tool-result storage evidence: `tools/tool_result_storage.py`.
- Tool-output cap evidence: `tools/tool_output_limits.py`.
- Active environment evidence: `tools/environments/base.py`.
- Telegram gateway evidence: `gateway/platforms/telegram.py`, `gateway/run.py`.
- Webhook gateway evidence: `gateway/platforms/webhook.py`.
- ACP/MCP evidence: `acp_adapter/server.py`, `mcp_serve.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/wake.rs` now exposes
  `workspace_tool_result_storage_root()`, matching the local live default
  `cwd/.zaion/tool-results` and retaining `data_dir()/tool-results` as the cwd
  failure fallback.
- `crates/zaion-cli/src/commands/process/mod.rs` re-exports that helper for
  sibling structured channel callers.
- `crates/zaion-cli/src/commands/network/routes.rs`,
  `crates/zaion-cli/src/commands/mcp.rs`,
  `crates/zaion-cli/src/commands/webhook/webhook_serve.rs`, and
  `crates/zaion-cli/src/commands/system.rs` now build API, MCP HTTP, webhook,
  and ACP stdio wake requests through the same canonical helper path.
- `crates/zaion-cli/src/commands/network/telegram.rs` now builds Telegram
  live-loop and `zaion tg simulate` requests through
  `telegram_wake_request(...)`, which attaches both the canonical envelope and
  explicit workspace-visible tool-result root.
- `crates/zaion-cli/src/commands/system.rs` source gates now lock both MCP and
  ACP helper shapes to canonical-envelope structured wake construction.

Verification:

- `cargo test -p zaion-cli structured_wake_request_workspace_tool_result_root_matches_live_default -- --nocapture`: failed first on missing `workspace_tool_result_storage_root()`, then passed.
- `cargo test -p zaion-cli structured_wake_request_from_envelope_defaults_to_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli api_run_structured_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_runtime_route_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli telegram_live_wake_request_uses_workspace_tool_result_root -- --nocapture`: failed in RED with `tool_result_storage_root == None`, then passed.
- `cargo test -p zaion-cli telegram_simulate_wake_request_uses_workspace_tool_result_root -- --nocapture`: failed in RED with `tool_result_storage_root == None`, then passed.
- `cargo test -p zaion-cli doctor_source_gate_locks_acp_canonical_envelope_ingress -- --nocapture`: failed in RED on the stale ACP source gate, then passed.
- `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix -- --nocapture`: passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 25 matching Telegram-related tests passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed.
- `cargo test -p zaion-cli wake_request_tool_result_storage_root_overrides_default_budget_root -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local structured wake storage root | `PARTIAL` | API, MCP HTTP, webhook, ACP stdio, Telegram live, and Telegram simulate structured wake calls now match local live wake and TUI local turns for workspace-visible spill files; continue threading explicit environment roots through delegated and remote sandbox paths. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Thread caller-supplied active-environment storage roots into delegated
  execution, remote sandbox runners, and non-local environment-backed tool
  paths.
- Extend persisted tool-result receipts with environment identity, permission
  scope, provenance, and turn-proof references.

---

## 2026-05-23 Active-Environment Tool Result Storage Target Stage [PARTIAL SLICE]

This stage adds the target-aware storage boundary needed for Hermes-style
environment-visible tool-result spill. It does not promote the whole-plan
verdict: latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Tool-result storage evidence: `tools/tool_result_storage.py`.
- Tool-output cap evidence: `tools/tool_output_limits.py`.
- Active environment evidence: `tools/environments/base.py`,
  `agent/tool_executor.py`, `tools/terminal_tool.py`.

Zaion implementation:

- `crates/zaion-runtime/src/tool_result_storage.rs` now exposes
  `ToolResultStorageTarget`, `HostToolResultStorageTarget`,
  `maybe_store_tool_result_with_target(...)`, and
  `enforce_turn_budget_with_target(...)`.
- Existing host-backed APIs continue to use `HostToolResultStorageTarget`, so
  current callers remain compatible.
- Target-aware storage writes full oversized output under the target root and
  injects a model-visible persisted-output pointer to that path.
- `crates/zaion-cli/src/commands/process/wake.rs` has target-aware helper
  coverage proving both per-result and aggregate spill can go through a fake
  active environment target without writing a host fallback file.
- Wake native tool execution helpers now receive the shared budget config and
  storage target, so successful native/MCP/todo tool outputs can spill through
  that target before being returned to provider context.
- Default local live wake now resolves its budget storage root to
  `cwd/.zaion/tool-results`, making oversized local tool output readable from
  the same workspace boundary as native `fs_*` and `shell_exec` tools. The
  host data dir remains only a cwd-resolution fallback.
- `WakeRequest` now exposes an optional structured `tool_result_storage_root`
  override, so TUI/gateway/MCP callers can bind wake spills to the intended
  workspace or active environment root without depending on process cwd.
- `crates/zaion-cli/src/commands/process/tui/app.rs` now captures the TUI
  startup workspace root in `AppState` and passes
  `workspace_root/.zaion/tool-results` into every local model-turn
  `WakeRequest`.

Verification:

- `cargo test -p zaion-runtime tool_result_storage -- --nocapture`: 8 passed.
- `cargo test -p zaion-cli wake_tool_context -- --nocapture`: 4 passed.
- `cargo test -p zaion-cli wake_native_tool_calls_use_active_environment_target_for_per_result_spill -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed after
  `wake_live_tool_result_budget_defaults_to_workspace_visible_dir` first failed on the
  old host data-dir default.
- `cargo test -p zaion-cli wake_request_tool_result_storage_root_overrides_default_budget_root -- --nocapture`:
  failed first on the missing structured override API, then passed.
- `cargo test -p zaion-cli tui_model_turn_request_ -- --nocapture`: 2 passed
  after failing first on missing TUI request-root plumbing.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.
- `cargo fmt -p zaion-runtime -p zaion-cli --check`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Tool-result storage and budgeting | `PARTIAL` | Runtime, wake helper, and native-tool execution boundaries now support active-environment-visible spill, default local live wake uses workspace-visible `.zaion/tool-results`, TUI local turns pass a captured startup workspace root, and structured callers can override the root; real sandbox/gateway/MCP/delegated target selection remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

Next actions:

- Thread real sandbox/environment storage targets through non-local live wake,
  gateway, MCP, and delegated tool execution.
- Thread caller-supplied `tool_result_storage_root` through gateway, MCP,
  delegated, and other service-launched wake requests whose cwd is not the
  intended workspace.
- Extend persisted tool-result receipts with environment identity, permission
  scope, provenance, and turn-proof references.

---

## 2026-05-23 Wake Todo State Redaction and Size Caps Stage [PARTIAL SLICE]

This stage hardens the wake todo persistence path without promoting the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Todo/hydration evidence: `tools/todo_tool.py`, `run_agent.py`.
- Redaction/compression evidence: `agent/redact.py`,
  `agent/context_compressor.py`.
- Tool-output cap evidence: `tools/tool_output_limits.py`,
  `tools/tool_result_storage.py`, `tools/budget_config.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/wake.rs` now sanitizes durable todo
  state immediately before appending `zaion.session_todo.state.v1`.
- Sanitized `state_json`, structured `state`, and `state_hash` are derived
  from the same JSON string, preserving event-internal consistency.
- Todo `title`/`content` fields are redacted and capped at 512 characters;
  todo `notes` fields are redacted and capped at 2048 characters.
- Model-visible todo tool responses remain unchanged; the new hardening
  applies to append-only durable state writes.

Verification:

- `cargo test -p zaion-cli wake_todo_state_event_redacts_and_caps_durable_strings_before_ledger_write -- --nocapture`: failed first on the unsanitized ledger write, then passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Wake todo persistence/hydration | `PARTIAL` | Wake now carries todo state across turns through signed events, queryable thread lookup, and sanitized/capped durable writes; continue with gateway/channel parity and richer sealed storage if full oversized content must be retained. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

---

## 2026-05-23 Payload-Queryable Wake Todo State Lookup Stage [PARTIAL SLICE]

This stage hardens the durable wake todo-state work without promoting the
whole-plan verdict: latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Todo/hydration evidence: `tools/todo_tool.py`, `run_agent.py`.
- Compression evidence: `agent/conversation_compression.py`,
  `agent/context_compressor.py`, `tests/tools/test_todo_tool.py`,
  `tests/run_agent/test_compression_boundary.py`.

Zaion implementation:

- `crates/zaion-ledger/src/ledger.rs` exposes
  `EventLedger::list_events_by_payload_string(...)`, scanning newest-first
  inside the indexed namespace/event-type slice and filtering exact string
  payload matches in Rust rather than depending on SQLite JSON1.
- `crates/zaion-ledger/src/tests.rs` proves latest exact matches are returned,
  other event types are excluded, and non-string payload values do not match.
- `crates/zaion-cli/src/commands/process/wake.rs` now hydrates durable todo
  state via a latest matching `thread_id` lookup, with a regression test where
  600 newer other-thread state events cannot shadow the target thread.

Verification:

- `cargo test -p zaion-ledger test_list_events_by_payload_string_returns_latest_exact_matches -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_todo_state_hydration_is_not_shadowed_by_newer_other_threads -- --nocapture`: passed after failing first on the old bounded-window implementation.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 6 passed.
- `cargo test -p zaion-ledger -- --nocapture`: 29 passed.
- `cargo fmt -p zaion-cli -p zaion-ledger --check`: passed.
- `cargo check -p zaion-ledger`: passed.
- `cargo check -p zaion-cli`: passed with existing warnings.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Wake todo persistence/hydration | `PARTIAL` | Wake now carries todo state across turns and compression child sessions through signed ledger events and queryable thread-scoped lookup; later sanitation covers redaction/size caps, while gateway/channel parity remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

---

## 2026-05-23 Durable Wake Todo State Hydration Stage [PARTIAL SLICE]

This stage moves wake todo continuity from current-turn only to a signed
ledger-backed state handoff while preserving the whole-plan verdict:
latest-Hermes parity remains `PARTIAL`.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Todo/hydration evidence: `tools/todo_tool.py`, `run_agent.py`.
- Compression evidence: `agent/conversation_compression.py`,
  `agent/context_compressor.py`, `tests/tools/test_todo_tool.py`,
  `tests/run_agent/test_compression_boundary.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/wake.rs` now carries a full
  `todo_store.response()` JSON snapshot in `ToolExecutionRecord` after
  successful todo calls, independent of the model-visible filtered response.
- Wake appends a signed `zaion.session_todo.state.v1` event after
  `channel.sent`, parented to the sent event.
- Wake hydrates `TodoStore` from the latest matching durable todo event before
  using the older tool-message-history fallback.
- Compression session splits snapshot active todo state into the child
  namespace when no fresh todo call was made in the splitting turn.

Verification:

- `cargo test -p zaion-cli wake_todo -- --nocapture`: 5 passed.
- `cargo test -p zaion-cli wake_tool_context_batch_enforces_aggregate_turn_budget_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-runtime compression_split_reinjects_active_todos_before_child_branch -- --nocapture`: passed.
- `cargo fmt -p zaion-cli -p zaion-runtime`: passed.
- `cargo check -p zaion-cli`: passed with existing warnings.
- `cargo check -p zaion-runtime`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Wake todo persistence/hydration | `PARTIAL` | Wake can now carry todo state across turns and compression child sessions through signed ledger events and queryable thread-scoped lookup; later sanitation covers redaction/size caps, while gateway/channel parity remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

---

## 2026-05-23 Wake Aggregate Tool Budget and Todo-Aware Compression Split Stage [PARTIAL SLICES]

This stage connects already-present runtime primitives to the live wake path
without claiming full Hermes parity.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Tool-result evidence: `tools/tool_result_storage.py`,
  `tools/tool_output_limits.py`, `agent/tool_executor.py`, `run_agent.py`,
  `tests/tools/test_tool_result_storage.py`,
  `tests/tools/test_tool_output_limits.py`.
- Todo/compression evidence: `tools/todo_tool.py`, `toolsets.py`,
  `agent/conversation_compression.py`, `agent/context_compressor.py`,
  `tests/tools/test_todo_tool.py`, `tests/agent/test_context_compressor.py`,
  `tests/run_agent/test_compression_boundary.py`,
  `tests/run_agent/test_compression_persistence.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/wake.rs` now runs
  `enforce_tool_context_turn_budget(...)` after each batch of live tool
  results and before model re-entry.
- `crates/zaion-runtime/src/compression_split.rs` now exposes
  `compress_and_split_with_todo_reinjection(...)`, reusing the split/session
  branch path while preserving active todo context.
- The wake compression path now calls the todo-aware split variant with the
  current session-local `TodoStore`.

Verification:

- `cargo test -p zaion-cli wake_tool_context_batch_enforces_aggregate_turn_budget_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_context_output_spills_large_results_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 5 passed after the
  durable todo-state slice.
- `cargo test -p zaion-runtime compression_split_reinjects_active_todos_before_child_branch -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Tool-result storage and budgeting | `PARTIAL` | Wake now has Hermes-style aggregate budget enforcement before model re-entry, and later target-aware storage APIs can spill through an active environment target; continue by wiring real environment targets into live tool execution and adding richer storage receipts. |
| Todo/compression continuity | `PARTIAL` | Active todos can be reinjected during compression split, and wake now persists durable todo state with queryable thread-scoped lookup; later sanitation covers redaction/size caps, while gateway/channel hydration remains open. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until all macro-module groups have latest-source evidence and local verification. |

---

## 2026-05-23 ACP Sink, MCP list_changed, Telegram Mention Gate, TUI Close/Resume Stage [PARTIAL SLICES]

This stage closes four narrow reliability gaps found during the latest
Hermes-alignment pass while preserving the strict verdict: Zaion remains
`PARTIAL` overall.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- ACP evidence: `acp_adapter/events.py`, `acp_adapter/server.py`,
  `acp_adapter/session.py`, `tests/acp/test_events.py`.
- MCP evidence: `tools/mcp_tool.py`, `tests/tools/test_mcp_tool.py`.
- Telegram evidence: `gateway/platforms/telegram.py`,
  `tests/gateway/test_telegram_group_gating.py`,
  `tests/gateway/test_telegram_mention_boundaries.py`,
  `tests/gateway/test_telegram_noise_filter.py`.
- TUI lifecycle evidence: `ui-tui/src/app/useSessionLifecycle.ts`,
  `tui_gateway/server.py`.

Zaion implementation:

- `crates/zaion-a2a/src/stdio_service.rs` adds protocol-event sink/collector
  routing and rejects unsafe or cross-principal ACP session lifecycle access.
- `crates/zaion-runtime/src/mcp_tools.rs` makes `refresh_server_tools()` retain
  old tools on rediscovery failure and replace only after successful refresh.
- `crates/zaion-cli/src/commands/network/telegram.rs` treats bare group slash
  commands and commands for other bots as group noise, while preserving
  access-policy-first behavior and releasing busy state after post-begin
  envelope rejection.
- `crates/zaion-cli/src/commands/process/tui/app.rs` detaches gateway transport
  state after `/gateway-close`, preventing later prompts from being stranded in
  a pending gateway session queue.

Verification:

- `cargo test -p zaion-cli gateway_close -- --nocapture`: 5 passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 23 matching tests passed
  across unit and integration filters.
- `cargo test -p zaion-runtime mcp -- --nocapture`: 26 passed.
- `cargo test -p zaion-a2a acp -- --nocapture`: 11 passed, 0 failed, 14
  filtered out.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| ACP protocol/session lifecycle | `PARTIAL` | Sink and owner checks are in place; continue toward live runtime event egress/replay. |
| MCP list_changed refresh | `PARTIAL` | Failure-preserving refresh is in place; continue toward listener/sampling parity. |
| Telegram live behavior | `PARTIAL` | Mention/noise and busy cleanup are safer; continue toward media/reaction/retry/channel depth. |
| TUI lifecycle | `PARTIAL` | Close no longer strands pending prompts; continue with resume/dequeue/WebSocket attach. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until every macro-module has source evidence and local verification. |

Next recommended parallel slices outside current hot files: tool-result
spill-to-file budgeting, session todo tool with compression reinjection, and
context-compression active-task safety.

---

## 2026-05-23 Gateway Approval/Clarify, Telegram Topic Routing, ACP Events, Dynamic MCP Toolsets Stage [PARTIAL SLICES]

This stage closes several narrow Hermes-alignment gaps while preserving the
strict latest-source verdict: Zaion is still `PARTIAL` overall.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- TUI gateway approval/clarify evidence: `ui-tui/src/gatewayTypes.ts`,
  `ui-tui/src/app/createGatewayEventHandler.ts`, and `tui_gateway/server.py`.
- Telegram channel evidence: `gateway/platforms/telegram.py`,
  `gateway/platforms/base.py`, and `gateway/run.py`.
- ACP event evidence: `acp_adapter/events.py`, `acp_adapter/server.py`, and
  `tests/acp/test_events.py`.
- MCP dynamic toolset evidence: `tools/mcp_tool.py`, `toolsets.py`,
  `tools/registry.py`, `tests/tools/test_mcp_tool.py`,
  `tests/acp/test_session.py`, and `tests/acp/test_server.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/tui/app.rs` now provides
  `/approve`, `/deny`, and `/clarify` gateway response controls that write
  `approval.respond` and `clarify.respond` JSON-RPC frames without starting a
  local wake turn.
- `crates/zaion-cli/src/commands/network/telegram.rs` now has a per-thread
  live busy guard with one replaceable pending ordinary prompt slot.
- `crates/zaion-adapters/src/telegram_adapter.rs` chunks Telegram output by
  UTF-16 code units for Telegram's 4096-unit limit and maps outbound
  topic/reply metadata into Telegram `message_thread_id` /
  `reply_to_message_id` while preserving General topic fallback.
- `crates/zaion-a2a/src/acp.rs` and `crates/zaion-a2a/src/stdio_service.rs`
  define/advertise ACP protocol events and provide stdio `protocol/event`
  JSON-RPC notification helpers.
- `crates/zaion-runtime/src/mcp_tools.rs`,
  `crates/zaion-cli/src/commands/tool.rs`, and
  `crates/zaion-cli/src/commands/capability.rs` report dynamic
  `mcp-<server>` toolsets, raw aliases, and capability JSON entries without
  renaming MCP tool calls.

Verification:

- `cargo test -p zaion-cli gateway -- --nocapture`: 34 passed in the gateway
  filter, plus matching filtered integration/stable tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 52 passed.
- `cargo test -p zaion-cli telegram_busy_guard -- --nocapture`: 3 passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 15 passed across
  matching filters.
- `cargo test -p zaion-cli busy_ -- --nocapture`: 10 passed.
- `cargo test -p zaion-cli queue -- --nocapture`: 16 unit tests plus 3
  matching filtered tests passed.
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

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| TUI gateway approval/clarify responses | `PARTIAL` | Approval and clarify replies now work over stdio JSON-RPC; continue with subagent controls, protocol recovery, session close/resume/dequeue, and WebSocket attach. |
| Telegram live behavior | `PARTIAL` | UTF-16 chunking, outbound topic/reply metadata routing, and per-thread busy guard are in place; continue with mention/allowlist depth, reactions, media, and retry semantics. |
| ACP protocol events | `PARTIAL` | Event DTOs, advertisement, and notification helper are present; continue with live runtime event egress and replay. |
| MCP dynamic toolsets | `PARTIAL` | Dynamic `mcp-<server>` reporting and raw aliases are present; continue with sampling and `list_changed` refresh. |
| Overall Hermes surpass | `PARTIAL` | Do not promote until each macro-module has latest-source evidence and local verification. |

---

## 2026-05-23 TUI Gateway Stdio JSON-RPC Transport Stage [PARTIAL SLICE]

This stage moves Zaion from a local gateway-event reducer toward an actual
Hermes-style TUI gateway transport. The terminal TUI can now attach an explicit
stdio gateway process with structured argv, send newline-framed JSON-RPC 2.0
requests, bootstrap a session with `session.create`, route prompts through
`prompt.submit`, and route busy steer/interrupt controls through
`session.steer` and `session.interrupt` once the gateway session id is known.
It also preserves startup correctness: prompts entered after transport attach
but before `session.create` returns are queued locally instead of falling back
to Zaion's local wake path.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Transport and RPC evidence: `ui-tui/src/gatewayClient.ts`,
  `ui-tui/src/app/useSessionLifecycle.ts`,
  `ui-tui/src/app/useSubmission.ts`,
  `ui-tui/src/app/turnController.ts`, `tui_gateway/entry.py`, and
  `tui_gateway/server.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/tui/mod.rs` parses
  `--gateway-stdio <program>` and repeated `--gateway-arg <arg>` into a
  `GatewayStdioConfig` without shell command-string construction.
- `crates/zaion-cli/src/commands/process/tui/app.rs` starts the configured
  process with piped stdin/stdout, attaches the existing event reader, runs a
  JSON-RPC writer thread, sends initial `session.create`, records
  `result.session_id`, drains startup-queued prompts after session readiness,
  and routes prompt/steer/interrupt control requests over the gateway session.

Verification:

- `cargo test -p zaion-cli gateway_transport_without_session_queues_prompt_instead_of_falling_back_to_local_wake -- --nocapture`: 1 passed, 0 failed.
- `cargo test -p zaion-cli gateway -- --nocapture`: 28 passed, 0 failed in the unit filter, plus matching filtered integration/stable tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 46 passed, 0 failed.
- `cargo test -p zaion-cli busy_ -- --nocapture`: 7 passed, 0 failed.
- `cargo test -p zaion-cli queue -- --nocapture`: 16 unit tests plus 3 matching filtered integration/slash tests passed, 0 failed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local TUI stdio JSON-RPC transport | `PARTIAL` | Transport attach, session bootstrap, prompt submit, and gateway-backed steer/interrupt routing are now implemented and regression-tested. |
| TUI runtime parity | `PARTIAL` | Continue with WebSocket attach mode, setup/status gating, session resume/close/dequeue depth, approval/clarify responses, subagent controls, protocol recovery, deferred agent-build parity, finalization, and broader tests. |

---

## 2026-05-23 TUI Gateway Event Frame Ingress Stage [PARTIAL SLICE]

This stage starts the next mainline after local queue and steer/interrupt
semantics: Hermes-style gateway event handling. Zaion's terminal TUI now has a
local event-frame reducer and `/gateway-event <json>` dogfood helper that can
ingest newline-framed JSON event frames for `gateway.ready`,
`gateway.protocol_error`, `approval.request`, `clarify.request`, `subagent.*`,
`message.delta`, and `message.complete` without routing them through the user
prompt path.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Event semantics evidence: `ui-tui/src/gatewayTypes.ts`,
  `ui-tui/src/gatewayClient.ts`, `ui-tui/src/app/createGatewayEventHandler.ts`,
  `tui_gateway/entry.py`, `tui_gateway/server.py`, and `tui_gateway/ws.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/tui/app.rs` now tracks gateway-ready
  state, skin hints, bounded protocol warnings, pending approval/clarify
  prompts, local subagent progress records, gateway-delivered assistant text,
  and gateway usage counters. The agents overlay reports this gateway state,
  and `/gateway-event <json>` can apply a frame locally for tests and manual
  dogfooding.

Verification:

- `cargo test -p zaion-cli gateway_event -- --nocapture`: 2 passed, 0 failed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local TUI gateway event reducer | `PARTIAL` | Gateway event frames now land in local state/observability/overlays and assistant text without becoming user turns. |
| TUI runtime parity | `PARTIAL` | Continue with actual JSON-RPC/WebSocket/stdio transport, session create/resume, live control RPCs, approval/clarify responses, subagent controls, protocol recovery, finalization, and broader tests. |

---

## 2026-05-23 TUI Steer/Interrupt Busy Controls Stage [PARTIAL SLICE]

This stage adds local terminal TUI semantics for Hermes-style busy input
control modes. Zaion can now keep `queue` as the default terminal behavior,
switch to `steer` for control injections into the active turn, and use
`interrupt` to request cancellation while putting the replacement prompt at the
front of the next-turn queue. This is still a local TUI slice, not the full
Hermes JSON-RPC gateway implementation.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Control semantics evidence: `ui-tui/src/app/useSubmission.ts`,
  `ui-tui/src/app/turnController.ts`,
  `ui-tui/src/app/slash/commands/core.ts`,
  `ui-tui/src/app/slash/commands/session.ts`, and
  `tui_gateway/server.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/tui/app.rs` now tracks
  `BusyInputMode::{Queue, Steer, Interrupt}`, stores local steer control
  injections separately from queued prompts, exposes `/busy`, `/steer`, and
  `/interrupt`, keeps busy steer text out of the user-turn transcript, and
  requests cancellation before queueing interrupt replacements at the front.

Verification:

- `cargo test -p zaion-cli busy_steer_mode_routes_busy_input_to_control_channel_not_fifo -- --nocapture`: passed.
- `cargo test -p zaion-cli slash_steer_without_active_turn_falls_back_to_next_turn_queue -- --nocapture`: passed.
- `cargo test -p zaion-cli busy_interrupt_mode_cancels_active_turn_and_queues_replacement_front -- --nocapture`: passed.
- `cargo test -p zaion-cli busy_ -- --nocapture`: 6 busy-filtered unit tests passed.
- `cargo test -p zaion-cli queue -- --nocapture`: 13 queue-filtered unit tests passed, plus matching filtered integration tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 34 TUI-filtered unit tests passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local TUI steer/interrupt controls | `PARTIAL` | Busy mode, steer fallback, and interrupt replacement behavior are implemented locally and regression-tested. |
| TUI runtime parity | `PARTIAL` | Continue with JSON-RPC/event gateway, live control events, approvals, clarify, subagents, protocol errors, streaming finalization, and broader tests. |

---

## 2026-05-23 TUI Queue Edit/Dequeue UX Stage [PARTIAL SLICE]

This stage adds local terminal TUI controls for reviewing, editing, replacing,
deleting, and cancelling queued prompts while an active model turn is still
streaming. It keeps the stage narrow: Zaion is closer to Hermes queue UX, but
full Hermes TUI runtime parity still requires the event gateway, live control
events, approval/clarify/subagent surfaces, protocol errors, and broader tests.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Queue/edit semantics evidence: `ui-tui/src/hooks/useQueue.ts`,
  `ui-tui/src/components/queuedMessages.tsx`,
  `ui-tui/src/app/useInputHandlers.ts`,
  `ui-tui/src/app/useSubmission.ts`, and
  `ui-tui/src/app/useMainApp.ts`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/tui/app.rs` now tracks a local
  queued-prompt edit index, lets Up/Down select queued items before history
  recall, saves queued edits with Enter, deletes the selected item with
  `Ctrl+X`, cancels queue editing with `Esc`, pauses auto-drain while editing,
  and renders a compact queued prompt preview window in the chat panel.

Verification:

- `cargo test -p zaion-cli queue -- --nocapture`: 11 queue-filtered unit tests
  passed, plus matching filtered integration tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 31 TUI-filtered unit tests
  passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Local TUI queue edit/delete UX | `PARTIAL` | Queue review/edit/delete behavior is implemented in the terminal TUI and regression-tested. |
| TUI runtime parity | `PARTIAL` | Continue with JSON-RPC/event gateway, steer/interrupt, approvals, clarify, subagents, protocol errors, streaming finalization, and broader tests. |

---

## 2026-05-23 Latest Hermes Report Expansion [PARTIAL]

The latest-source comparison report is now expanded enough to serve as the
acceptance contract for full HermesAgent benchmarking before Zaion macro-module
maturity work is claimed complete.

Updated artifact:

- `docs/zaion_vs_hermes.md`

The report now contains:

- source-cited latest Hermes architecture map;
- config-complete-to-first-start sequence;
- workspace/session/profile model;
- CLI/TUI/gateway/tool/memory collaboration model;
- detailed Zaion vs latest Hermes comparison table.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Latest-source report completeness | `PARTIAL` | Comparison structure is complete enough to guide execution, but implementation parity is not complete. |
| TUI runtime parity | `PARTIAL` | Still the next implementation mainline beyond the busy-input queue slice. |
| Overall Hermes surpass | `PARTIAL` | Do not promote to `SURPASSED` until each module has source evidence and local verification. |

---

## 2026-05-23 Source Gate Reconciliation [SURPASSED]

This checkpoint keeps the architecture truth anchors required by `zaion doctor`
in sync with current implementation evidence:

- Phase 8-B Source Truth Reconciliation [SURPASSED]
- Unified Runtime Execution Metrics [SURPASSED]
- BatchRunner Worker Pool Execution [SURPASSED]
- Runtime BatchRunner Execution Chain [SURPASSED]
- Full Architecture Truth Alignment [SURPASSED]
- Stable Runtime Proof Matrix [SURPASSED]
- Operation Stream Source Truth Reconciliation [SURPASSED]

OPD/evolve remains promotable only when the append-only Ed25519 chain verifies
a latest `ConfirmedStable` record. Old Phase 1 command catch-up is not the
Promotion anchor: only when the append-only Ed25519 chain verifies a latest `ConfirmedStable` record.
active mainline; the active mainline is latest-Hermes TUI runtime parity, live
Telegram/channel parity, and tool/MCP/ACP/session/context parity.

---

## 2026-05-23 TUI/TG Visible Reply Lifecycle Isolation Stage [SURPASSED SLICE]

This stage closes the narrow but user-critical failure in which Zaion could
surface internal lifecycle events as Telegram/TUI chat replies. The fix aligns
this slice with Hermes' product boundary: gateway/runtime events may be shown
in observability panels and traces, but ordinary chat replies must contain
assistant output or intentionally visible tool/risk events, not lifecycle
status lines.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Relevant surfaces: `tui_gateway/*`, `ui-tui/src/*`, `gateway/run.py`,
  `gateway/platforms/base.py`, `gateway/platforms/telegram.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/panel_render.rs` now suppresses unknown or
  lifecycle-only operation events for chat-facing rendering.
- `crates/zaion-runtime/src/panel_sink.rs` no longer exposes `TurnCompleted`
  through `TranscriptSink::visible_text()`.
- Regression tests cover `ProviderCalling`, `TurnCompleted`, existing TUI
  lifecycle filtering, explicit TUI error fallback, and final-content streaming
  fallback.

Verification:

- `cargo test -p zaion-cli panel_render -- --nocapture`: 4 passed.
- `cargo test -p zaion-runtime panel_sink -- --nocapture`: 2 passed.
- `cargo test -p zaion-cli lifecycle_operation_events_do_not_render_as_chat_messages -- --nocapture`: passed.
- `cargo test -p zaion-cli completed_turn_without_visible_token_shows_explicit_tui_error -- --nocapture`: passed.
- `cargo test -p zaion-cli streaming_callback_forwards_final_text_when_provider_did_not_emit_token_deltas -- --nocapture`: passed.
- Broader `cargo test -p zaion-cli telegram -- --nocapture` now passes after
  the existing `telegram_channel_commands_share_one_effective_token_source`
  doctor/source-gate blocker was reconciled.
- `cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents -- --nocapture`: passed.
- `cargo test -p zaion-cli global_event_stream_replays_shared_operation_backlog_after_operation_cursor -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| TUI/TG visible reply lifecycle isolation | `SURPASSED` | Keep this boundary as a regression-locked invariant. |
| TUI runtime parity | `PARTIAL` | Continue Hermes-grade gateway/event protocol, queue/interrupt/approval/subagent/protocol-error, and terminal tests. |
| Telegram/live channel parity | `PARTIAL` | Doctor/source-gate blocker is cleared; finish live channel delivery ergonomics. |

---

## 2026-05-23 TUI Busy Input Queue Drain Stage [PARTIAL SLICE]

This stage lands the minimum Hermes queue-mode behavior for Zaion's terminal
TUI. While a model turn is streaming, ordinary user input now enters a local
FIFO queue instead of replacing the active stream or creating a second assistant
placeholder. Local audit slash commands such as `/status` remain immediate and
preserve the active model stream. When the active turn reaches a terminal
state, Zaion drains exactly one queued prompt and starts it as the next user
turn.

Hermes reference:

- Latest main: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Queue semantics evidence: `ui-tui/src/app/useConfigSync.ts`,
  `ui-tui/src/hooks/useQueue.ts`, `ui-tui/src/app/useSubmission.ts`,
  `ui-tui/src/app/useMainApp.ts`, and `tui_gateway/server.py`.

Zaion implementation:

- `crates/zaion-cli/src/commands/process/tui/app.rs` now stores queued prompts
  in a local FIFO, keeps audit slash commands immediate while busy, reconnects
  token deltas to the nearest streaming assistant placeholder, and drains one
  queued prompt after a completed, cancelled, or errored turn settles.

Verification:

- `cargo test -p zaion-cli busy_ -- --nocapture`: 4 passed, 0 failed.
- `cargo test -p zaion-cli queue -- --nocapture`: 9 passed, 0 failed across
  matching unit/integration filters.
- `cargo test -p zaion-cli tui -- --nocapture`: 26 passed, 0 failed.
- `cargo test -p zaion-cli completed_turn_dequeues_next_prompt_and_starts_it_once -- --nocapture`: passed.
- `cargo test -p zaion-cli queued_busy_input_is_transcripted_once_when_drained -- --nocapture`: passed.
- `cargo test -p zaion-cli busy_audit_command_keeps_streaming_placeholder_connected_to_tokens -- --nocapture`: passed.

Plan impact:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| TUI busy input queue drain | `PARTIAL` | Queue-mode minimum semantics are implemented and tested. |
| TUI runtime parity | `PARTIAL` | Continue beyond local queue UX: gateway/event protocol, steer/interrupt, approvals, clarify, subagents, protocol errors, finalization, and broader tests. |

---

## 2026-05-23 Latest Hermes Source Revalidation Stage [PARTIAL]

The latest-source read has advanced enough to update the long-term plan from
old latest-source `OPEN` to `PARTIAL`. This is not a completion claim for
Hermes surpass. Zaion has differentiated strengths, but latest Hermes still sets
the reference bar for TUI runtime, live channels, tool breadth, ACP/MCP,
profile/session/context, and first-start polish.

Reference evidence:

- Latest Hermes mirror: `D:/zaion-reference/hermes-agent-latest`.
- Remote `origin/main`, local `origin/main`, and local `HEAD`: `9c0807070388c4f612a827230f1314ebbf24e857`.
- Latest commit: `2026-05-24 15:57:26 -0700`, `test(cli): update resume usage-hint assertion for numbered selection`.
- Historical zip `D:/zaion-reference/zaion-rust-cleanup-20260501/hermes-agent-2026.4.8.zip` was listed and remains only the old baseline.
- Latest environment/runtime evidence is `tools/environments/*`,
  `batch_runner.py`, and `trajectory_compressor.py`; top-level `environments/*`
  belongs to the old `2026.4.8` zip.

Latest Hermes architecture map:

1. CLI/setup/profile/config: `hermes`, `cli.py`, `hermes_cli/main.py`,
   `hermes_cli/commands.py`, `hermes_cli/setup.py`, `hermes_cli/config.py`,
   `hermes_cli/profiles.py`. Hermes creates a profile-scoped home with config,
   env, sessions, memories, skills, skins, logs, plans, workspace, cron, and
   home boundaries.
2. First-start path: `hermes` or `hermes chat` resolves profile/config/provider,
   tools, memory, context, and session state; TUI starts React/Ink, then
   `GatewayClient` spawns or attaches to Python `tui_gateway`, waits for
   `gateway.ready`, returns a quick `session.create` skeleton, then defers real
   `AIAgent` construction.
3. TUI runtime: `ui-tui` + `tui_gateway` use JSON-RPC/NDJSON/WebSocket for
   `session.create`, `prompt.submit`, streaming deltas, slash exec, queue,
   steer, interrupt, dequeue, approvals, clarify, background turns, subagents,
   usage, and protocol errors.
4. Gateway/channels: `BasePlatformAdapter` normalizes channel events,
   active-session guards, pending merge, typing, retries, media extraction, and
   processing callbacks. Telegram adds MarkdownV2, UTF-16 4096 splitting,
   BotCommand, mention/allowlist, guest mode, group observation, media
   batching/cache, reactions, topic/reply fallback.
5. Memory/context/session: `memory_manager` orchestrates builtin and external
   providers, prefetch, turn sync, non-fatal provider failures, session switch,
   pre-compression, write/delegation/shutdown hooks. Prompt assembly uses SOUL
   identity, project context priority, memory/tool/environment/skills/context,
   and gateway overlays. Compression keeps child-session lineage.
6. Tools/MCP/ACP: `tools/registry.py`, `toolsets.py`, and
   `toolset_distributions.py` provide broad built-ins and toolset routing. MCP
   supports stdio/HTTP client servers, filtering, dynamic refresh, runtime
   `mcp-<server>` toolsets, sampling, and `hermes mcp serve` with 10 bridge
   tools. ACP is JSON-RPC stdio with new/load/resume/fork, permissions, tool
   progress, MCP expansion, and event replay.

Current plan labels:

| Workstream | Label | Plan implication |
| --- | --- | --- |
| Entry/help/launcher/WebUI relationship | `SURPASSED` | Preserve current Zaion command contract. |
| Neural topology observability direction | `SURPASSED` | Keep this as Zaion's unique product lead. |
| TUI runtime parity | `PARTIAL` | Queue-mode minimum plus local queue edit/delete UX are landed; next phase is gateway/event protocol, steer/interrupt, approvals, clarify, subagents, protocol errors, finalization, tests. |
| Telegram/live channel parity | `PARTIAL` | Next implementation phase: live adapter proof beyond `tg simulate`, MarkdownV2/split, batching, media, mention gates, reactions, reply/topic fallback. |
| Tool/MCP/ACP parity | `PARTIAL` | Next implementation phase: broader callable tools, MCP dynamic discovery/sampling/toolsets, ACP load/resume/fork/permission bridge. |
| Profile/session/context/memory parity | `PARTIAL` | Next implementation phase: profile-scoped workspace, prompt assembly, memory provider lifecycle, compression hygiene, lineage verification. |
| OPD/evolution/batch parity | `PARTIAL` | Continue signed promotion work, but compare latest Hermes against `tools/environments/*`, `batch_runner.py`, and `trajectory_compressor.py`. |

Next execution order:

1. TUI runtime parity beyond the local queue minimum and tests.
2. Live Telegram/channel parity implementation and tests.
3. Tool/MCP/ACP/profile/session parity implementation and tests.
4. Re-run latest Hermes comparison and update all four ledgers after each stage.

---

## 0. Document Role And Priority

This file is not the implemented-facts ledger. It is the long-horizon execution
plan for surpassing latest Hermes main. The current Hermes baseline is
`9c0807070388c4f612a827230f1314ebbf24e857`; Hermes `2026.4.8` is historical
archive evidence only.

When documents conflict, decide current truth in this order:

1. `plans/openclaw_latest_gap_report.md`
2. This file, `plans/hermes_surpass_master_plan.md`
3. `MASTER_PLAN.md`
4. `docs/zaion_vs_hermes.md`
5. Other blueprints or drafts

Rules:

- This file defines long-term goals, phase order, execution loop, and acceptance framework.
- `plans/openclaw_latest_gap_report.md` defines current factual status.
- After implementation lands, update the gap ledger first, then this file,
  `MASTER_PLAN.md`, and `docs/AGENTS.md`.
- Do not rewrite planning goals as implemented facts.

---

## 1. Hermes Baseline Ruling - 2026-05-23 Latest Main

The current ruling is based on `D:/zaion-reference/hermes-agent-latest` at
`9c0807070388c4f612a827230f1314ebbf24e857`. The old `2026.4.8` zip remains an
archive baseline, but it no longer represents current Hermes.

This latest-main source pass covered CLI/setup/profile, React/Ink TUI plus
`tui_gateway`, gateway/channel runtime, memory/context/session, ACP/MCP/tools,
and batch/trajectory/environment evidence. The current result is `PARTIAL`:
Zaion leads on product entry and neural topology observability direction, while
TUI runtime, live channels, tool/MCP/ACP, profile/session/context, and
batch/evolution still need parity work before they can be marked `SURPASSED`.

### 1.1 总体能力??

> 2026-05-23 calibration note: the capability clusters below preserve the first Hermes `2026.4.8` structural reading. For latest-main execution, use the revalidation block at the top of this file and the gap ledger. In particular, OPD/environment evidence should use `tools/environments/*`, `batch_runner.py`, and `trajectory_compressor.py`, not old top-level `environments/*`.
Hermes 主要由以下能力簇组成??
1. **CLI / TUI / 配置治理??*
   - 主入口：`hermes_cli/main.py`、`cli.py`
   - 配置治理：`hermes_cli/config.py`
   - profile 治理：`hermes_cli/profiles.py`
   - memory 控制面：`hermes_cli/memory_setup.py`
   - webhook 控制面：`hermes_cli/webhook.py`
   - mcp 控制面：`hermes_cli/mcp_config.py`

2. **Agent runtime 主循??*
   - `environments/agent_loop.py`
   - `agent/*`
   - 支持 model routing、tool calling、context compression、memory orchestration

3. **Session / Memory 双层持久??*
   - `gateway/session.py`
   - `agent/memory_manager.py`
   - `agent/builtin_memory_provider.py`
   - `plugins/memory/*`

4. **Gateway / 多平台消息总线**
   - `gateway/run.py`
   - `gateway/config.py`
   - `gateway/platforms/*`
   - `gateway/channel_directory.py` / `delivery.py` / `pairing.py` / `hooks.py`

5. **ACP / MCP / 外部协议??*
   - `acp_adapter/server.py`
   - `acp_adapter/session.py`
   - `mcp_serve.py`
   - `hermes_cli/mcp_config.py`

6. **Skills / Plugins / 生态扩展面**
   - `skills/**`
   - `optional-skills/**`
   - `agent/skill_commands.py`
   - `agent/skill_utils.py`

7. **训练 / 评测 / 自进化基础设施**??*Zaion 当前最大差??*??   - `batch_runner.py` ??多进程并行轨迹生成、断点续传、ShareGPT 格式、HuggingFace 集成
   - `environments/agentic_opd_env.py` ??**token-level 密集训练信号**、VLLM prompt_logprobs、per-token advantages 计算
   - `environments/agent_loop.py` ??完整工具调用循环、轨迹记录、工具统计提??   - `environments/benchmarks/*` ??TBLite、TerminalBench 2 标准评测框架
   - **理论支撑**：Princeton OpenClaw-RL 论文（arXiv:2603.10165??
8. **安全与工具治??*
   - `tools/osv_check.py`
   - `tools/patch_parser.py`

### 1.2 结构规模信号

当前基线分析确认??- zip 总文件数??489
- tests??65
- gateway 相关测试??13
- CLI 相关测试??08
- tools 相关测试??06
- ACP 测试??
- builtin skills??02 文件
- optional-skills??30 文件
- 平台适配器：18 ??Python 文件

这说??Hermes 的护城河不只来自功能点数量，还来自：
- 产品化治理面
- 协议面完整度
- 会话/记忆持久化闭??- 多平台总线
- 大规模技能生??- 广覆盖测试基??
---

## 2. Zaion 当前状态阶段快照（摘录??gap ledger，非独立真相源）

以下内容仅是**基于 `plans/openclaw_latest_gap_report.md` ??2026-04-12 的阶段快照摘??*，便于后续规划排序；如与账本发生差异，一律以 gap ledger 为准??
截至 2026-04-12，Zaion 已确认具备或收口的能力包括：

### 2.1 已完??/ 收口
- pricing / usage cost
- prompt cache
- secret redaction
- prompt injection scan
- tool call parser 扩展
- smart router
- checkpoint manager
- @引用语法基础
- MoA 基础
- Telegram 增强基础
- 多平台网关基础
- 批处理训练系统基础
- session 基础能力
- slash 结构??- session reset policy

### 2.2 部分完成
- ContextCompressor 产品化集??- execute_code / 程序化工具调??- sessions 高级能力
- slash 产品级行??- platform adapter 深化
- memory setup/status/off 治理??
### 2.3 明确缺口
- webhook subscribe/list/remove/test
- mcp serve/add/remove/list/test/configure
- ACP stdio service
- gateway install/uninstall/setup
- profile list/use/create/delete/export/import
- import-from-openclaw 迁移向导
- honcho / cross-session memory federation
- On-policy distillation / AgenticOPDEnv
- OSV 集成
- V4A patch format
- 正式版对标报??
---

## 3. 总体战略：不是追平，而是三层超越

Zaion ??Hermes 的战略不应停留在“命令数量追平”，而应分三层推进：

### 3.1 第一层：对等补齐
??Hermes 已成熟的治理面、协议面、迁移面、系统面补齐，消除明??GAP??
### 3.2 第二层：主链收口
??Zaion 已存在但未完全接入主回路的能力，接进真实产品路径，避免“有 crate / 有结??/ 无产品闭环”??
### 3.3 第三层：质变超越
在以下维度形成系统级优势??- Ed25519 principal identity
- signed append-only ledger
- A2A federation
- 7-layer memory
- cross-device migration
- Rust 原生性能与部署面
- ACI / AST 级代码修??- 可验证治??/ receipt / provenance

---

## 4. 长期执行路线??
---

## Phase 0 · 自我进化引擎回炉重造（**2026-04-15 新增最高优先级**??
### 目标
??Zaion zaion-evolve ??静态扫??+ LLM 补丁"升级??在线学习 + token-level 优化"的范式突破版本??
### 背景
2026-04-15 范式突破评估结论??- **当前状??*：zaion-evolve 仅有静态扫??+ LLM 补丁生成 + Trinity 评审，无训练闭环
- **Hermes 能力**：AgenticOPDEnv 提供 token-level 密集训练信号、完整数据→训练→评测→迭代闭环
- **核心差距**：学习范式落后（静??vs 在线）、信号密度不足（二元 vs token-level）、闭环不完整（缺训练环节??- **独有优势未融??*：Ed25519/Ouroboros/ACI/ZK-Rollup 等核心优势均未融入自我进化引??
### 工作：Zaion Agentic OPD Engine?? 个子阶段??
#### Phase 0.1 · 核心 OPD 引擎（对??Hermes??**目标**：实现与 Hermes AgenticOPDEnv 同等能力
- [ ] 实现 `AgenticOpdEnv` Rust 版本（基??Hermes agentic_opd_env.py??- [ ] 集成 VLLM backend 支持 prompt_logprobs
- [ ] 实现 token-level advantages 计算（A_t = teacher_logprob - student_logprob??- [ ] 实现工具交互学习闭环（从工具结果中提取学习信号）
- [ ] 实现 ShareGPT 格式轨迹输出（trajectories.jsonl??- [ ] 实现多进程并行轨迹生成（对标 batch_runner.py??- [ ] 实现断点续传机制（checkpoint.json??- [ ] 实现工具集随机采样分??- [ ] 实现 HuggingFace 集成（Parquet/Arrow 格式??
#### Phase 0.2 · 签名轨迹与可验证性（Zaion 独有超越??**目标**：实??Hermes 不具备的可验证训练能??- [ ] 集成 Ed25519 principal identity（每条轨迹签名）
- [ ] 实现 signed trajectory ledger（append-only??- [ ] 实现 provenance tracking（训练信号溯源）
- [ ] 实现训练信号可验证性（token-level advantages 附带 provenance 证明??- [ ] 实现轨迹溯源与审计接??
#### Phase 0.3 · AST 级优化（Zaion 独有超越??**目标**：从文本补丁升级??AST 级代码变??- [ ] 集成 ACI 2.0 AST surgical interface
- [ ] 实现 AST-level transformation（不仅生成文本补丁，还生??AST 变换??- [ ] 实现 syntax-aware optimization（训练信号精确到 AST 节点??- [ ] 实现多语言 AST 支持（Rust/Python/TypeScript/JavaScript??- [ ] 实现 AST diff ??merge

#### Phase 0.4 · 自愈训练闭环（Zaion 独有超越??**目标**：实现训练进程容错与自动恢复
- [ ] 集成 Ouroboros auto-recovery（训练进程崩溃自动恢复）
- [ ] 实现 signed checkpoint management（checkpoint 自动签名与验证）
- [ ] 实现分布式训练容??- [ ] 实现训练进程监控
- [ ] 实现自动重启与恢??
#### Phase 0.5 · 可验证压缩（Zaion 独有超越??**目标**：实现轨迹压缩同时保证可验证??- [ ] 实现 ZK-Rollup trajectory compression
- [ ] 实现 SHA-256 commitment ??- [ ] 实现 compression proof generation
- [ ] 实现压缩轨迹验证
- [ ] 实现存储优化

#### Phase 0.6 · 标准化评测与迭代
**目标**：建立自动化评测框架
- [ ] 实现 TBLite 对标 benchmark
- [ ] 实现 TerminalBench 2 对标 benchmark
- [ ] 实现自动化评测框??- [ ] 实现持续迭代闭环
- [ ] 发布范式突破评估报告 v2.0

### 验收
- **功能对标**：具??Hermes AgenticOPDEnv 同等能力、token-level 密集信号、完整训练闭环、多进程并行、断点续??- **范式突破**：签名轨迹与可验证训练、AST 级优化、自愈训练闭环、可验证轨迹压缩（Hermes 均不具备??- **质量标准**：测试覆盖率 ??80%、性能不低??Hermes、文档完整、代码质量无 clippy 警告
- **理论创新**：形成论文产出，基于 Princeton OpenClaw-RL + Zaion 独有创新

### 预期时间
- Phase 0.1-0.2?? 周（核心对标 + 签名轨迹??- Phase 0.3-0.5?? 周（AST + 自愈 + 压缩??- Phase 0.6?? 周（评测与报告）
- **总计**?? 周达到范式突??
### 当前状??- 2026-04-15：完成范式突破评估，确认需回炉重??- 下一步：立即启动 Phase 0.1（核??OPD 引擎）与 Phase 0.2（签名轨迹）并行实现

---

## Phase 0-legacy · 作战账本与入口统一（已完成??
### 目标
把“分析结果”变成仓库内权威执行入口，防止后续漂移??
### 工作
1. 建立本文件，作为 Hermes 超越长期主计划??2. 回写 `MASTER_PLAN.md`，使其成为作战导航页??3. 强化 `plans/openclaw_latest_gap_report.md` 的执行索引能力??
### 验收
- 仓库内存在单一长期入口文档??- `MASTER_PLAN.md` 不再误导当前阶段判断??- gap ledger 继续保持真相源地位??
---

## Phase 1 · 命令治理面补齐（P1 主攻??
### 目标
补齐??Hermes 产品完整度影响最大的治理命令族??
### 1. webhook 子系??对标 Hermes??- `hermes_cli/webhook.py`
- `gateway/platforms/webhook.py`

Zaion 当前状态：PARTIAL
- 已有 `zaion webhook subscribe/list/remove/test`
- 已有 TOML 持久化、HMAC/secret、基础 SSRF 防护、请??timeout、核心输入校??- 已有基础单测覆盖 subscribe/remove/sign/URL/event 校验
- 本轮新增更严格的公网域名/IP 校验、非 2xx webhook test 失败判定、响应摘要输出，以及本地响应解析测试
- 仍缺真实运行时自动投递闭环、DNS rebinding 级别防护、`cmd_webhook_test` 端到端运行时集成测试

Zaion 下一步目标：
- gateway 动态热加载
- 运行时投递闭??- 更强网络目标校验
- CLI 与运行时彻底打??
### 2. MCP 命令??对标 Hermes??- `hermes_cli/mcp_config.py`
- `mcp_serve.py`

Zaion 目标??- `zaion mcp serve/add/remove/list/test/configure`
- MCP server config 存储
- discovery / test / connect probe
- 至少支持 stdio ??HTTP/SSE 其中一个完整路??
### 3. Profile 命令??对标 Hermes??- `hermes_cli/profiles.py`

Zaion 目标??- `zaion profile list/use/create/delete/export/import`
- profile 隔离 config / env / sessions / memory / skills
- active profile 机制
- clone / export / import 路径

### 4. import-from-openclaw
对标 Hermes??- `docs/migration/openclaw.md`

Zaion 目标??- `zaion import-from-openclaw`
- dry-run
- preset
- overwrite / conflict policy
- migration report

### Phase 1 验收
- 命令存在且具备帮助、测试、持久化落点
- 对应 gap ledger 条目可从 `GAP` 转为 `PARTIAL` ??`DONE`

---

## Phase 2 · 协议面收口（ACP / MCP / 外部消费面）

### 目标
??Zaion 成为可被 IDE、外??agent、自动化系统消费的真实协议节点??
### 1. ACP stdio / JSON-RPC service
当前 Zaion 基础??- `crates/zaion-a2a/src/acp.rs`
- `crates/zaion-cli/src/commands/network.rs`

当前不足??- ??REST run store，不??Hermes 同级 stdio agent server
- 缺少 session lifecycle
- 缺少 tool progress / thinking / permission bridge

Zaion 目标??- `zaion acp serve`
- stdio JSON-RPC 协议服务
- session create/load/resume/fork
- tool progress / permission 回调
- ??A2A / ledger 打??
### 2. MCP serve 主面
Zaion 目标??- conversation / session / permission / process / receipt 可观察桥
- 不只对等 Hermes conversation bridge，还要接??Zaion ledger / principal / audit ??
### Phase 2 验收
- 至少一个真??stdio 服务可被外部 client 调用
- 协议测试存在
- 会话与权限链路可工作

---

## Phase 3 · 主运行时收口

### 目标
??Zaion 已存在的核心能力进入真实主链??
### 1. ContextCompressor 产品??当前已有??- `crates/zaion-runtime/src/compressor.rs`
- slash `/compress` 已可调用

仍缺??- `cmd_wake` / `cmd_bot` 主链路集??- parent_session_id 分裂??- ??session metadata / billing 联动

### 2. execute_code 真执行链
当前已有??- `crates/zaion-runtime/src/execute_code.rs`

仍缺??- Python subprocess + RPC bridge
- Node subprocess + RPC bridge
- timeout / allowed_tools / trace record / output capture

### 3. slash 产品行为深化
仍缺??- branch/fork
- background 真调??- display/reasoning/statusbar 持久??- approval / checkpoint / session 联动

### Phase 3 验收
- 不再只是结构层存在，而是进入实际主循??- gap ledger 中对??PARTIAL 收口??DONE 或更细粒??PARTIAL

---

## Phase 4 · Gateway 与平台联邦强??
### 目标
从“多平台基础适配”升级为“多平台总线 + 治理??+ 服务化”??
### 工作
1. 深化 base adapter 能力
   - richer edit
   - typing
   - processing start/complete callback
   - interrupt model
   - media cache 分层

2. 收口 gateway install/uninstall/setup
3. 优先??webhook，再评估 WhatsApp 等平??4. 统一跨平??thread / reply / topic / edit 语义

### Phase 4 验收
- gateway 具备产品??setup ??service 生命周期
- 平台行为不再只停留在基础发送接??
---

## Phase 5 · Memory 治理??federation 质变

### 目标
??Zaion memory 从局部能力升级为系统优势??
### 工作
1. 收口 memory setup/status/off 产品??2. ??memory 控制面接入运行时自动消费闭环
3. 建立 principal-centered cross-session federation
4. 构建记忆来源、签名、压缩、回放、治理策??5. 规划 honcho-equivalent，但不做低水平复??
### Zaion 超越方向
- signed memory receipts
- principal / workspace / device / agent 四维索引
- A2A memory federation
- provenance-aware memory compaction

---

## Phase 6 · 安全??patch/tooling 面超??
### 目标
??Hermes 的点状安全能力升级为 Zaion 的系统治理能力??
### 工作
1. OSV 集成
   - ??hub / mcp / install / runtime extension 做检??2. V4A patch support
   - 文本 patch 兼容
   - ??ACI / AST patch 融合
3. tool / patch / install provenance 进入 ledger

### 验收
- OSV ??patch 不再是蓝图，而是有真??CLI / runtime 落点

---

## Phase 7 · 训练 / 评测 / 自进化闭??
### 目标
形成 Zaion 的长期自进化基础设施??
### 工作
1. batch runner 收口
2. AgenticOPDEnv / on-policy distillation 环境落地
3. 建立 parity benchmark suite
4. 建立长稳性回归与多阶段指标面??
### 验收
- 不再只靠功能开发推进，而能通过数据??轨迹/评测驱动迭代

---

## Phase 8 · 生态与技能层压制

### 目标
??skills / hub / plugin 生态层??Hermes 形成结构性压制??
### 工作
1. 扩展技能分类：github / research / software-development / productivity / mcp / migration
2. 完整??hub publish/search/install/audit
3. 形成 Rust-native skills + bridge runtime 双栈
4. 对齐 agentskills/open standard，同时加入签名与 provenance

---

## 5. 固定执行闭环

后续执行必须遵循以下循环??
### Loop A · 研究校正
1. 先查 gap ledger
2. 再查 Hermes 证据文件
3. 再查当前 Zaion crate 落点
4. 输出 P2 计划

### Loop B · 小步实现
1. 仅在 `D:/zaion-rust` 下工??2. 优先修改现有 crate
3. 单次改动小而可验证

### Loop C · 独立验收
1. 运行测试 / 构建 /验证命令
2. 发起独立 review
3. 处理阻断问题

### Loop D · 账本回写
1. 先更??`plans/openclaw_latest_gap_report.md`
2. 再更新本文件
3. 再更??`MASTER_PLAN.md`
4. 必要时更??`docs/zaion_vs_hermes.md`

### Loop E · 继续下一??- 未完??Hermes 全量超越前，不把阶段性成果误报为“最终完成??
---

## 6. 当前优先级排序（2026-04-12 裁定??
### P0：治理入口统一
1. 建立 `plans/hermes_surpass_master_plan.md`
2. 回写 `MASTER_PLAN.md`
3. 强化 gap ledger 执行索引

### P1：命令治理面主攻
4. webhook
5. mcp 命令??6. profile 命令??7. import-from-openclaw

### P2：协议与主链收口
8. ACP stdio service
9. ContextCompressor 主链集成
10. execute_code 真执行链
11. slash 产品行为深化

### P3：平台与记忆联邦
12. gateway install/uninstall/setup
13. honcho / cross-session memory federation
14. memory runtime 自动消费闭环

### P4：高级超??15. OSV
16. V4A patch
17. AgenticOPDEnv / distillation
18. 正式版对标报??
---

## 7. 当前裁定

截至 2026-04-12??
- Zaion 已完成多??Hermes P0/P1 基础能力，但仍未形成全面产品闭环??- 真正的下一阶段主攻不应再停留在已完成的 sessions / slash 结构??/ reset policy??- 后续应优先转向：**webhook、mcp、profile、import-from-openclaw、ACP stdio、主链收??*??- 本文件自此作为后续长期执行的主计划入口；但所有真实状态判断仍??`plans/openclaw_latest_gap_report.md` 为准??
