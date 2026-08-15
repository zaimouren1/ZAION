# Zaion Legacy Master Plan Evidence

> Status: historical evidence ledger, frozen for routine implementation work.
> Current measured facts live in `docs/PROJECT_STATUS.md`; active priorities and
> acceptance gates live in `ROADMAP.md`; repository ownership and navigation
> live in `docs/PROJECT_MAP.md`. Do not append routine stage checkpoints here.
> Update this file only when explicitly reconstructing or changing its own
> historical/general evidence scope from source-backed material. This notice
> supersedes older maintenance and truth-source rules below.

## 2026-07-13 Whole-Project Organization Baseline [PARTIAL]

This stage establishes a truthful navigation and repository-health baseline
for the whole Zaion workspace. It does not promote the latest-Hermes verdict;
overall comparison remains `PARTIAL`.

Source and repository evidence:

- `cargo metadata --locked --offline` reports 36 workspace crates, 66 targets,
  and roughly 593 resolved packages.
- Crate `src/` directories contain 195,899 Rust lines; crate `tests/`
  directories contain 20,928 lines; 38 Rust files are at least 1,000 lines.
- `zaion-cli` is the composition hub with 30 internal dependencies and 86,553
  source lines.
- Interactive `zaion` currently enters the inline `read_line` chat path in
  `crates/zaion-cli/src/commands/process/tui/mod.rs`; the richer
  `app.rs::run_tui_app` observability path has no production caller.
- The local Hermes mirror is
  `9c0807070388c4f612a827230f1314ebbf24e857` from 2026-05-24.

Implemented organization slice:

- Added `docs/PROJECT_MAP.md`, `docs/PROJECT_STATUS.md`, `docs/README.md`, and
  `plans/README.md` as stable navigation and dated truth surfaces.
- Added read-only `scripts/project-audit.ps1` for worktree, crate, large-file,
  documentation-integrity, local-mirror, and optional disk-usage evidence.
- Corrected public entry documentation to distinguish active inline chat from
  the non-interactive neural snapshot and the currently unreachable full
  ratatui app.
- Updated CI branch matching to `**`, added Cargo `--locked` gates, and made
  stateful workspace tests explicitly use `--test-threads=1`.
- Raised the Docker builder from Rust 1.78 to 1.93 because current
  Wasmtime/Cranelift dependencies require Rust 1.92 or newer; Docker Cargo
  builds now use `--locked`.
- Unified Docker, systemd, and Homebrew services on the foreground full-runtime
  entry `zaion _daemon_run` instead of experimental Singularity startup.
- Added the Apache-2.0 `LICENSE`, a scoped `CONTRIBUTING.md`, and missing
  workspace license inheritance for `zaion-shadow` and `zaion-telemetry`.
- Removed six accidentally tracked `crates/zaion-mcp/target/mcp-tests/`
  runtime artifacts and ignored future machine-local Claude settings.

Verification:

- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/project-audit.ps1`:
  passed and reported the known repository warnings.
- `cargo metadata --format-version 1 --locked --offline`: passed.
- `cargo test -p zaion-types -p zaion-paths --locked --offline -j1`: passed,
  31 tests.
- `cargo clippy -p zaion-types -p zaion-paths --all-targets --locked --offline -- -D warnings`:
  passed.
- `.claude/settings.json` JSON parsing and
  `bash scripts/check-release-assets.sh`: passed after intentional website and
  hook retirement.
- `git diff --check`: passed.
- `cargo fmt --all -- --check`: failed on 73 pre-existing Rust files; a bulk
  formatter rewrite was intentionally not mixed into this stage.
- Main-binary `cargo check` exceeded three minutes without a compiler error;
  full workspace check/test/clippy remains unverified.

Still open:

- Select one authoritative TUI, one turn kernel, and one gateway/WebUI server.
- Isolate rustfmt cleanup, dependency advisory remediation, giant-file splits,
  and historical ledger encoding recovery into reviewable stages.

Label update:

- Whole-project organization baseline: `PARTIAL`.
- Overall latest-Hermes comparison: still `PARTIAL`.

## 2026-06-03 Telegram Native Bare Local MEDIA Path Extraction Evidence [PARTIAL SLICE]

This checkpoint adds conservative Hermes-style bare local file extraction to
Telegram outbound media delivery. Overall latest-Hermes parity remains
`PARTIAL`: Zaion can now detect existing absolute local media/document paths in
plain response text and route them through the existing native Telegram upload
path, but Hermes still has broader extraction, home-relative expansion,
extension coverage, safety-root policy, and cross-platform dispatch.

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

Implemented and verified slice:

- `TelegramAdapter::send_with_report` now scans non-code plain text lines for
  existing absolute local bare paths with allowlisted media/document
  extensions.
- Matched bare paths are removed from the user-visible text and turned into
  `TelegramOutboundMedia` entries, reusing the existing native media routing
  for photos, videos, audio/voice, documents, albums, and fallback behavior.
- The slice is intentionally conservative: no `~/` expansion, no relative
  paths, no remote URLs, no broad archive/document extension list, no richer
  allowed-root policy, and no cross-platform abstraction claim.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_bare_local_media_path_uploads_and_cleans_text -- --nocapture`: failed first because only the text message id `897` appeared and media id `898` was missing, then passed; fresh rerun passed, 1 test.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 7 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 40 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests, with existing dead-code/unused warnings.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram native bare local media path extraction: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Home-relative paths, paths with spaces, richer extension coverage, remote URL
  delivery, safety-root governance, per-file policy granularity,
  cross-platform outbound media propagation, and broader gateway delivery
  orchestration remain open.
## 2026-06-03 Telegram Native MEDIA Album Fallback Evidence [PARTIAL SLICE]

This checkpoint hardens the Telegram native album path by falling back to
per-photo uploads when Telegram `sendMediaGroup` fails. Overall latest-Hermes
parity remains `PARTIAL`: Zaion now avoids losing local image `MEDIA:` replies
when album delivery is rejected, but broader media retry/orchestration remains
open.

Implemented and verified slice:

- Local multi-image `MEDIA:` replies first try Telegram `sendMediaGroup` as
  before.
- If the media group request fails, the same images are retried as individual
  `sendPhoto` uploads instead of aborting the whole reply.
- The fallback is recorded in `TelegramDeliveryReport.fallbacks` as
  `media_group_fallback_to_photos`, and the fallback photo message ids are
  preserved in `telegram_message_ids`.
- Existing single-image, `[[as_document]]`, audio/voice, video, and non-image
  document routing remains unchanged.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_album_failure_falls_back_to_photos -- --nocapture`: failed first because `sendMediaGroup` `ok=false` aborted delivery, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 7 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 39 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram native `MEDIA:` album fallback: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Mixed-media grouping, remote URL media delivery, bare local path detection,
  richer file safety allow roots, per-file policy granularity, cross-platform
  outbound media propagation, retry/backoff policy, and broader gateway
  delivery orchestration remain open.

## 2026-06-02 Telegram Native MEDIA Album Routing Evidence [PARTIAL SLICE]

This checkpoint adds a narrow Telegram native album path for local image
`MEDIA:` outputs. Overall latest-Hermes parity remains `PARTIAL`: Zaion can now
batch multiple local photos into a Telegram media group, but broader media
group policy, remote media handling, and cross-platform orchestration remain
open.

Implemented and verified slice:

- Two or more local image `MEDIA:` files (`.png/.jpg/.jpeg/.gif/.webp`) in the
  same outbound response now batch into a single Telegram `sendMediaGroup`
  request instead of separate `sendPhoto` calls.
- The first image in the album carries the caption and existing reply/topic
  metadata; remaining images are attached as additional album items.
- Single-image image `MEDIA:` delivery still routes through `sendPhoto`;
  `[[as_document]]`, audio/voice, video, and non-image documents remain on
  their existing paths.
- The slice remains conservative: local absolute files only, 50 MiB max, no
  remote URL delivery, no bare-path auto-detection, no media grouping for
  non-image media, and no cross-platform abstraction claim.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_groups_multiple_images_into_album -- --nocapture`: failed first because the adapter still sent two separate `sendPhoto` requests, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 6 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 38 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram native `MEDIA:` album routing: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Media grouping/albums for mixed media, remote URL media delivery, bare local
  path detection, richer file safety allow roots, per-file policy granularity,
  cross-platform outbound media propagation, and broader gateway delivery
  orchestration remain open.

## 2026-06-02 Telegram Native MEDIA As-Document Policy Evidence [PARTIAL SLICE]

This checkpoint adds a narrow Hermes-style `[[as_document]]` policy to outbound
Telegram `MEDIA:` delivery so large/lossless local images can be sent as
original-byte documents instead of Telegram photo uploads. Overall
latest-Hermes parity remains `PARTIAL`: Zaion now supports one explicit
lossless-image directive for local Telegram media, but broader outbound media
policy and cross-platform orchestration remain open.

Implemented and verified slice:

- Standalone `[[as_document]]` directives are stripped from user-visible
  Telegram reply text.
- Local image `MEDIA:` files (`.png/.jpg/.jpeg/.gif/.webp`) marked by
  `[[as_document]]` route to `sendDocument` with multipart field `document`
  instead of `sendPhoto`.
- Ordinary image `MEDIA:` delivery still routes to `sendPhoto`; video, audio,
  explicit voice, and document routing remain unchanged.
- The slice remains conservative: local absolute files only, 50 MiB max, no
  media grouping, no remote URL delivery, no bare path extraction, and no
  cross-platform abstraction claim.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_as_document -- --nocapture`: failed first because `[[as_document]]` leaked into visible text and the image did not yet route as a document, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 5 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 37 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram native `MEDIA:` as-document image policy: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Media grouping/albums, remote URL media delivery, bare local path detection,
  richer file safety allow roots, per-file policy granularity, cross-platform
  outbound media propagation, and broader gateway delivery orchestration remain
  open.

## 2026-06-02 Telegram Native MEDIA Audio/Voice Routing Evidence [PARTIAL SLICE]

This checkpoint extends the narrow Telegram outbound `MEDIA:` delivery path from
photo/video/document uploads into native audio and explicit voice routing while
preserving cleaned text delivery, reply/topic metadata, and media message-id
reporting. Overall latest-Hermes parity remains `PARTIAL`: Zaion now routes
local audio files through Telegram-native audio/voice endpoints, but broader
outbound media parity remains open.

Implemented and verified slice:

- Standalone `[[audio_as_voice]]` directives are stripped from user-visible
  Telegram reply text and mark outbound `MEDIA:` files in the same response as
  voice-intended.
- Local `.mp3/.wav/.m4a/.flac/.ogg/.opus` `MEDIA:` files now route to
  `sendAudio` with multipart field `audio` by default.
- Local `.ogg/.opus` files marked with `[[audio_as_voice]]` route to
  `sendVoice` with multipart field `voice`, matching Hermes' explicit voice
  directive behavior instead of auto-converting ordinary audio.
- Existing image/video/document `MEDIA:` routing, local absolute-file
  validation, 50 MiB max size, cleaned text send path, and delivery report
  media message ids remain intact.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_routes_audio -- --nocapture`: failed first because `.mp3` still used `sendDocument` and `[[audio_as_voice]]` remained in visible text, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 4 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 36 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram native `MEDIA:` audio/voice routing: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Media grouping/albums, `[[as_document]]` lossless policy, bare local path
  detection, remote URL media delivery, richer file safety allow roots,
  cross-platform outbound media propagation, and broader gateway delivery
  orchestration remain open.

## 2026-06-02 Telegram Native MEDIA Tag Delivery Evidence [PARTIAL SLICE]

This checkpoint adds a narrow Hermes-style outbound media delivery path for
Telegram replies while preserving the existing text delivery, Markdown fallback,
topic/reply metadata, and signed delivery reporting. Overall latest-Hermes
parity remains `PARTIAL`: Zaion can now upload local files referenced by
standalone `MEDIA:<absolute-path>` lines as native Telegram photo/video/document
messages, but broader outbound media parity remains open.

Implemented and verified slice:

- `TelegramAdapter::send_with_report` now extracts standalone `MEDIA:` local
  file tags from outbound text, removes those internal tags from user-visible
  text, and keeps the cleaned text delivery path intact.
- Existing Telegram reply/topic metadata is reused for native media uploads;
  uploaded media message ids are included in the `TelegramDeliveryReport`.
- Local `.png/.jpg/.jpeg/.gif/.webp` files route to `sendPhoto`, local
  `.mp4/.mov/.avi/.mkv/.webm` files route to `sendVideo`, and other accepted
  local files route to `sendDocument`.
- The slice is intentionally conservative: only existing absolute local files
  are uploaded, files over 50 MiB are rejected, and no remote URL download,
  bare-path auto-detection, media grouping, audio voice routing, or cross-
  platform delivery abstraction is claimed.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_uploads_local_image_and_cleans_text -- --nocapture`: failed first because only `sendMessage` ran and no native media upload occurred, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_send_with_media_tag_ -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 34 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram native `MEDIA:` tag delivery: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Native audio/voice routing, media grouping/albums, `[[as_document]]` policy,
  bare local path detection, remote URL media delivery, richer file safety
  allow roots, multi-platform outbound media propagation, and broader gateway
  delivery orchestration remain open.
## 2026-06-02 Telegram Cached Video Vision Context Evidence [PARTIAL SLICE]

This checkpoint extends cached Telegram media analysis from images into opt-in
provider-backed video description while preserving the canonical Telegram
envelope body and source hash. Overall latest-Hermes parity remains `PARTIAL`:
Zaion now can send cached local Telegram `video/*` bytes to an
OpenAI-compatible multimodal `/v1/chat/completions` endpoint, but local video
decoding, frame extraction, OCR, temporal scene parsing, outbound native media,
and broader runtime/channel breadth remain open.

Implemented and verified slice:

- `telegram_wake_request` can now append a `Telegram media vision analysis`
  system context block when `ZAION_TELEGRAM_MEDIA_VISION` is enabled and cached
  Telegram media metadata points at a local `video/*` file.
- The video analysis request reads only the local cached file and sends it as a
  `data:<mime>;base64,...` multimodal `video_url` item to an OpenAI-compatible
  `/v1/chat/completions` endpoint.
- Native Telegram videos and video documents keep their existing cached MIME,
  media type, Telegram `file_id`, and delivery evidence; the generated
  description is extra model context, not a replacement for signed ingress.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving duplicate semantics and provenance.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_video_vision_context_reaches_llm -- --nocapture`: failed first because no media video vision request was sent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 39 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached video vision context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Local video decoding, frame extraction, OCR, richer temporal analysis,
  provider-independent video tooling, outbound native media delivery, broader
  channel propagation, and deeper cancellation/task unwind behavior remain
  open.
## 2026-06-02 Telegram Cached PDF Document Context Evidence [PARTIAL SLICE]

This checkpoint extends cached Telegram document extraction from plain text and
Office formats to bounded PDF literal-text extraction while preserving the
canonical Telegram envelope body and source hash. Overall latest-Hermes parity
remains `PARTIAL`: Zaion now extracts simple uncompressed literal text from
cached `.pdf` documents, but rich PDF parsing, OCR, provider-backed document
analysis, video analysis, outbound native media, and broader runtime/channel
breadth remain open.

Implemented and verified slice:

- `telegram_wake_request` can now append PDF-derived text to the existing
  `Telegram document text` system context block when
  `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled and cached Telegram media metadata
  points at a `.pdf` document.
- PDF extraction reads only local cached files, scans at most 1 MiB, requires a
  `%PDF` header near the start, decodes common PDF literal-string escapes, and
  collects basic uncompressed `Tj` / `TJ` text operands before clipping previews
  to the existing 16 KiB budget.
- Existing text, DOCX, PPTX, and XLSX document extraction remain intact;
  compressed PDF streams, complex encodings, OCR, and rich document semantics
  remain intentionally unsupported in this narrow slice.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_pdf_document_context_reaches_llm -- --nocapture`: failed first because no PDF-derived document text context reached the first LLM request, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 38 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached PDF document context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Rich PDF parsing/OCR, provider-backed document extraction, richer Office
  parsing, video analysis, outbound native media delivery, broader channel
  propagation, and deeper cancellation/task unwind behavior remain open.
## 2026-06-02 Telegram Cached XLSX Document Context Evidence [PARTIAL SLICE]

This checkpoint extends cached Telegram document extraction from plain text,
DOCX, and PPTX to bounded XLSX worksheet-text extraction while preserving the
canonical Telegram envelope body and source hash. Overall latest-Hermes parity
remains `PARTIAL`: Zaion now extracts shared-string and worksheet cell text from
cached `.xlsx` documents, but PDF extraction, richer Office semantics, video
analysis, outbound native media, and broader runtime/channel breadth remain
open.

Implemented and verified slice:

- `telegram_wake_request` can now append XLSX-derived text to the existing
  `Telegram document text` system context block when
  `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled and cached Telegram media metadata
  points at a `.xlsx` document.
- XLSX extraction reads only local cached files, parses the ZIP central
  directory, supports store/deflate entries, rejects ZIP64 and oversized XML
  entries, reads `xl/sharedStrings.xml` when present, scans
  `xl/worksheets/sheet*.xml` in path order, decodes common XML entities, strips
  NUL bytes, and clips previews to the existing 16 KiB budget.
- Existing text, DOCX, and PPTX document extraction remain intact; cached PDFs
  continue to preserve signed metadata and cached-path evidence without
  accidental text injection.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_xlsx_document_context_reaches_llm -- --nocapture`: failed first because no XLSX-derived document text context reached the first LLM request, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 37 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached XLSX document context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- PDF extraction, richer Office parsing, video analysis, outbound native media
  delivery, broader channel propagation, and deeper cancellation/task unwind
  behavior remain open.
## 2026-06-02 Telegram Cached PPTX Document Context Evidence [PARTIAL SLICE]

This checkpoint extends cached Telegram document extraction from plain text and
DOCX to bounded PPTX slide-text extraction while preserving the canonical
Telegram envelope body and source hash. Overall latest-Hermes parity remains
`PARTIAL`: Zaion now extracts `ppt/slides/slide*.xml` text from cached `.pptx`
documents, but PDF, XLSX, richer Office parsing, video analysis, outbound
native media, and broader runtime/channel breadth remain open.

Implemented and verified slice:

- `telegram_wake_request` can now append PPTX-derived text to the existing
  `Telegram document text` system context block when
  `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled and cached Telegram media metadata
  points at a `.pptx` document.
- PPTX extraction reads only local cached files, parses the ZIP central
  directory, supports store/deflate entries, rejects ZIP64 and oversized XML
  entries, scans `ppt/slides/slide*.xml` in path order, decodes common XML
  entities, strips NUL bytes, and clips previews to the existing 16 KiB budget.
- Existing text and DOCX document extraction remain intact; cached non-text
  documents such as PDFs continue to preserve signed metadata and cached-path
  evidence without accidental text injection.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_pptx_document_context_reaches_llm -- --nocapture`: failed first because no PPTX-derived document text context reached the first LLM request, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 36 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached PPTX document context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- PDF extraction, XLSX extraction, richer Office parsing, video analysis,
  outbound native media delivery, broader channel propagation, and deeper
  cancellation/task unwind behavior remain open.
## 2026-06-02 Telegram Cached DOCX Document Context Evidence [PARTIAL SLICE]

This checkpoint extends the cached Telegram document text path from plain
text-like files to bounded DOCX paragraph extraction while preserving the
canonical Telegram envelope body and source hash. Overall latest-Hermes parity
remains `PARTIAL`: Zaion now extracts `word/document.xml` text from cached
`.docx` documents, but PDF, XLSX, PPTX, richer Office parsing, video analysis,
outbound native media, and broader gateway/runtime breadth remain open.

Implemented and verified slice:

- `telegram_wake_request` can now append DOCX-derived text to the existing
  `Telegram document text` system context block when
  `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled and cached Telegram media metadata
  points at a `.docx` document.
- DOCX extraction reads only local cached files, parses the ZIP central
  directory, supports store/deflate entries, rejects ZIP64 and oversized XML
  entries, decodes common XML entities, strips NUL bytes, and clips previews to
  the existing 16 KiB budget.
- Cached non-text documents such as PDFs continue to preserve signed metadata
  and cached-path evidence without accidental text injection.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_docx_document_context_reaches_llm -- --nocapture`: failed first because no DOCX-derived document text context reached the first LLM request, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 35 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached DOCX document context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- PDF extraction, XLSX/PPTX extraction, richer Office parsing, video analysis,
  outbound native media delivery, broader channel propagation, and deeper
  cancellation/task unwind behavior remain open.

## 2026-06-02 Telegram Cached Text Document Context Evidence [PARTIAL SLICE]

This checkpoint turns cached text-like Telegram documents into opt-in
model-visible context while preserving the canonical Telegram envelope body and
source hash. Overall latest-Hermes parity remains `PARTIAL`: Zaion now can
extract clipped previews from cached text documents, but PDF/Office extraction,
video analysis, outbound native media, and deeper gateway/runtime breadth
remain open.

Implemented and verified slice:

- `telegram_wake_request` now optionally appends a `Telegram document text`
  system context block when `ZAION_TELEGRAM_DOCUMENT_TEXT` is enabled and
  cached media metadata points at a text-like Telegram `document`.
- The extraction path reads only local cached files, accepts text MIME types and
  common text extensions, strips NUL bytes, and clips previews to 16 KiB.
- Cached non-text documents such as PDFs continue to preserve signed metadata
  and cached-path evidence without accidental text injection.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_text_document_context_reaches_llm -- --nocapture`: failed first because no document text context reached the first LLM request, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_generic_document_dispatches_and_records_media_metadata -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_transcription_context_reaches_llm -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 34 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached text document context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Rich PDF/Office document extraction, video analysis, outbound native media
  delivery, broader channel propagation, and deeper cancellation/task unwind
  behavior remain open.

# Zaion Rust ??主计划书（续??

## 2026-05-30 Telegram Cached Audio Transcription Context Evidence [PARTIAL SLICE]

This checkpoint turns cached Telegram voice/audio bytes into opt-in
model-visible transcription context while preserving the canonical Telegram
envelope body and source hash. Overall latest-Hermes parity remains `PARTIAL`:
Zaion now can transcribe cached Telegram audio through an OpenAI-compatible
`/audio/transcriptions` endpoint, but document extraction, video analysis,
outbound native media, and deeper gateway/runtime breadth remain open.

Implemented and verified slice:

- Added an OpenAI-compatible audio transcription client for cached Telegram
  audio files.
- `telegram_wake_request` now optionally appends a `Telegram audio
  transcription` system context block when `ZAION_TELEGRAM_AUDIO_TRANSCRIPTION`
  is enabled and cached media metadata points at `audio/*` voice/audio files.
- The transcription request posts cached bytes as multipart form data to
  `/v1/audio/transcriptions`, using explicit audio-transcription env overrides
  for base URL, model, and API key.
- The injected transcript includes media type, MIME type, Telegram `file_id`,
  and the generated transcript; cached media reference context remains
  separate.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_transcription_context_reaches_llm -- --nocapture`: failed first because no audio transcription request was sent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_dispatches_and_records_media_metadata -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_wake_request -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_vision_context_reaches_llm -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 33 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached audio transcription context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Audio transcription is currently opt-in via env vars and scoped to cached
  `audio/*` files; document text extraction, image-document policy, video
  analysis, outbound native media delivery, broader channel propagation, and
  deeper cancellation/task unwind behavior remain open.
## 2026-05-30 Telegram Cached Photo Vision Context Evidence [PARTIAL SLICE]

This checkpoint turns cached Telegram photo bytes into opt-in model-visible
vision analysis while preserving the canonical Telegram envelope body and
source hash. Overall latest-Hermes parity remains `PARTIAL`: Zaion now can
analyze cached non-sticker Telegram images through an OpenAI-compatible vision
endpoint, but audio transcription, document extraction, outbound native media,
and deeper gateway/runtime breadth remain open.

Implemented and verified slice:

- Added a reusable OpenAI-compatible image vision client and refactored static
  sticker vision to share it.
- `telegram_wake_request` now optionally appends a `Telegram media vision
  analysis` system context block when `ZAION_TELEGRAM_MEDIA_VISION` is enabled
  and cached media metadata points at `image/*` non-sticker files.
- The image analysis request sends cached bytes as a
  `data:<mime>;base64,...` multimodal image URL to `/v1/chat/completions`.
- The injected analysis includes media type, MIME type, Telegram `file_id`,
  and the generated one-sentence description; cached media reference context
  remains separate.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound caption/text, preserving signed ingress and duplicate semantics.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_vision_context_reaches_llm -- --nocapture`: failed first because no media vision request was sent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_sticker_vision_describer_reaches_llm_delivery_and_cache -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_wake_request -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 32 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached photo vision context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Vision analysis is currently opt-in via media-vision env vars and scoped to
  cached image files; audio transcription, document text extraction, video
  analysis, outbound native media delivery, broader channel propagation, and
  deeper cancellation/task unwind behavior remain open.
## 2026-05-30 Telegram Cached Media Model Context Evidence [PARTIAL SLICE]

This checkpoint makes cached Telegram media references model-visible in live
wake turns without changing the canonical Telegram envelope body or source hash.
Overall latest-Hermes parity remains `PARTIAL`: Zaion now exposes cached
photo/document/media paths, MIME types, media types, and Telegram file ids as a
small extra model context block, while actual media-byte analysis/transcription,
native outbound media breadth, and gateway/runtime depth remain open.

Implemented and verified slice:

- `WakeRequest` now supports `extra_model_context`, inserted as system context
  before the user message and after history/context preparation.
- Live Telegram wake requests add a `Telegram cached media` context block when
  the canonical envelope carries `telegram_media_cached_paths`.
- The context block includes cached local path, media type, MIME type,
  `file_id`, and `file_unique_id` when present, without embedding media bytes.
- Canonical Telegram envelopes and `source_hash` remain bound to the original
  inbound text/fallback text, preserving existing signed ledger semantics.
- Captioned-photo live polling now proves the first LLM request sees cached
  media context while delivery/envelope metadata still preserve signed cached
  media evidence.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: failed first because the first LLM request lacked `Telegram cached media`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 31 tests.
- `cargo test -j 1 -p zaion-cli telegram_wake_request -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram cached media model-visible context: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Direct media-byte vision for non-sticker images, audio transcription,
  document text extraction, outbound native media delivery, broader channel
  propagation, and deeper cancellation/task unwind behavior remain open.

## 2026-05-30 Telegram Sticker Production Vision Evidence [PARTIAL SLICE]

This checkpoint wires the Hermes-style sticker description seam to an explicit
OpenAI-compatible production vision provider. Overall latest-Hermes parity
remains `PARTIAL`: Zaion now has production static-sticker vision analysis
behind an opt-in gate, while broader model-visible media consumption, animated
or video sticker policy, outbound native media, and gateway/runtime breadth
remain open.

Implemented and verified slice:

- `telegram_adapter_for_runtime` can attach an `OpenAiStickerDescriber` when
  `ZAION_TELEGRAM_STICKER_VISION` is enabled.
- The describer reads the cached static sticker image, sends it as a
  `data:<mime>;base64,...` image URL to an OpenAI-compatible
  `/v1/chat/completions` endpoint, and parses the assistant content as the
  sticker description.
- The vision request includes sticker emoji/set context plus a concise sticker
  description prompt and remains fully mockable in tests.
- API calls are opt-in and use explicit sticker-vision env overrides first,
  then OpenAI config/provider maps; Authorization is only sent when a key is
  configured.
- Live Telegram dispatch carries the vision-generated description into the
  LLM request, signed `telegram.delivery` evidence, canonical envelope
  metadata, and persisted `sticker_descriptions.json` cache.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_sticker_vision_describer_reaches_llm_delivery_and_cache -- --nocapture`: failed first because no production vision request was sent, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 15 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 31 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram static sticker production vision provider wiring: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Animated/video sticker policy, transcription and model-visible media
  consumption beyond sticker descriptions, outbound native media delivery,
  broader channel propagation, and deeper cancellation/task unwind behavior
  remain open.

## 2026-05-30 Telegram Sticker Description Generation Evidence [PARTIAL SLICE]

This checkpoint extends the Hermes-style sticker description path from cache
reads to deterministic generation and write-back for newly seen static
stickers. Overall latest-Hermes parity remains `PARTIAL` because Zaion now has
the local provider seam and cache write-back behavior, but the production
vision provider integration is still a follow-up.

Implemented and verified slice:

- `TelegramAdapter::receive()` now caches static sticker bytes before deriving
  sticker prompt text, so cache misses can use the newly cached image path.
- A narrow `TelegramStickerDescriber` seam can describe static sticker images
  from cached path, MIME type, emoji, set name, and Telegram `file_unique_id`.
- Generated descriptions are written to `sticker_descriptions.json` keyed by
  Telegram `file_unique_id`, including emoji, set name, and cache timestamp.
- Generated descriptions replace the sticker fallback with model-visible text
  such as `[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]`.
- Inbound metadata records `telegram_sticker_description` and
  `telegram_sticker_description_source: "generated"` while preserving cached
  sticker binary path/MIME metadata.
- Live Telegram dispatch carries generated descriptions into the LLM request,
  signed `telegram.delivery` evidence, canonical envelope metadata, and the
  persisted sticker description cache.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_generates_and_caches_static_sticker_description -- --nocapture`: failed first because the sticker text stayed at the old fallback, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_generated_sticker_description_reaches_llm_delivery_and_cache -- --nocapture`: failed first because the LLM request lacked the generated description, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 15 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 30 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram static sticker description generation/write-back evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Production vision provider wiring for static stickers, animated/video sticker
  policy, transcription / model-visible media consumption, outbound native media
  delivery, broader channel propagation, and deeper cancellation/task unwind
  behavior remain open.

## 2026-05-29 Telegram Sticker Description Cache Evidence [PARTIAL SLICE]

This checkpoint adds the first Hermes-style sticker description cache read path.
Overall latest-Hermes parity remains `PARTIAL` because Zaion now consumes
pre-existing cached descriptions but still lacks automatic vision analysis and
write-back generation for newly seen static stickers.

Implemented and verified slice:

- `TelegramAdapter::receive()` reads `sticker_descriptions.json` from the
  configured Telegram media cache root and looks up entries by Telegram
  `file_unique_id`.
- Cache hits replace the metadata-only sticker fallback with model-visible
  description text such as
  `[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]`.
- Inbound metadata records `telegram_sticker_description` and
  `telegram_sticker_description_source: "cache"` while preserving cached
  sticker binary path/MIME metadata.
- Live Telegram dispatch carries the cached description into the LLM request,
  signed `telegram.delivery` evidence, and canonical envelope metadata.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_injects_cached_sticker_description -- --nocapture`: failed first because the sticker text stayed at the old fallback, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_cached_sticker_description_reaches_llm_and_delivery -- --nocapture`: failed first because the LLM request/delivery path lacked cached description metadata, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 14 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 29 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed after formatting.

Label update:

- Telegram cached sticker description prompt injection evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Automatic sticker vision analysis, cache write-back for new stickers,
  animated/video sticker policy, transcription / model-visible media
  consumption, outbound native media delivery, broader channel propagation,
  and deeper cancellation/task unwind behavior remain open.

## 2026-05-29 Telegram Static Sticker Cache Evidence [PARTIAL SLICE]

This checkpoint extends the Telegram sticker path from metadata-only handling
to safe static-sticker binary caching. Overall latest-Hermes parity remains
`PARTIAL` because Hermes still adds vision description, file_unique_id
description caching, and richer prompt injection.

Implemented and verified slice:

- Static, non-animated, non-video Telegram stickers now call Bot API `getFile`
  when a media cache root is configured.
- Returned Telegram `file_path` values still use the existing safe relative
  path validation before download.
- Sticker image bytes are cached through the image cache tier, preserving
  `.webp` and common fallback image extensions.
- Inbound metadata records `telegram_media_cached_paths` and
  `telegram_media_cached_mime_types` alongside existing sticker fields.
- Live Telegram dispatch preserves static-sticker cached-path evidence in
  signed `telegram.delivery` events and canonical wake envelopes.
- Animated/video sticker fallback remains metadata-only and does not attempt
  binary download.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_static_sticker -- --nocapture`: failed first because no cached sticker path existed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_static_sticker_dispatches_and_records_cached_media_metadata -- --nocapture`: failed first because live dispatch made no sticker `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 13 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 28 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram static sticker download/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Sticker vision description injection, description cache keyed by
  `file_unique_id`, animated/video sticker policy, transcription /
  model-visible media consumption, outbound native media delivery, broader
  channel propagation, and deeper cancellation/task unwind behavior remain
  open.

## 2026-05-29 Telegram Sticker Metadata Evidence [PARTIAL SLICE]

This checkpoint adds source-preserving Telegram sticker evidence to the live
Telegram receive and dispatch path. Overall latest-Hermes parity remains
`PARTIAL` because Hermes still goes further with sticker image download, vision
description, and cached prompt injection.

Implemented and verified slice:

- `TelegramAdapter::receive()` now gives sticker-only messages a stable
  model-visible fallback text such as `[Telegram sticker: ok from zaion_pack]`
  instead of dropping them as empty input.
- Inbound sticker metadata records Telegram sticker type, dimensions, emoji,
  set name, animation/video flags, file size, and custom emoji id when present.
- Live Telegram dispatch now preserves sticker-specific metadata in signed
  `telegram.delivery` events and canonical wake envelopes.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_preserves_sticker_media_metadata -- --nocapture`: failed first because sticker-only text was empty and sticker-specific metadata was absent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_sticker_dispatches_and_records_media_metadata -- --nocapture`: failed first because sticker-only messages did not reach LLM/sendMessage delivery, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 12 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 27 tests.

Label update:

- Telegram sticker metadata and signed delivery evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Sticker binary cache/vision description injection, transcription / model-visible
  media consumption, outbound native media delivery, broader channel
  propagation, and deeper cancellation/task unwind behavior remain open.

## 2026-05-29 Telegram Generic Document Cache Evidence [PARTIAL SLICE]

This checkpoint extends the Telegram media cache path from photos, image
 documents, voice/audio, native video, and video documents to inbound generic
Telegram documents such as PDFs. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter::receive()` now downloads non-image, non-video Telegram
  `message.document` files through Bot API `getFile` when a media cache root
  is configured.
- Returned Telegram `file_path` values still use the existing safe relative
  path validation before file download.
- Generic documents use an allowlisted extension policy for common document
  types, preserve Telegram MIME metadata when present, and default unknown
  files to `.bin` / `application/octet-stream`.
- Downloaded bytes are cached under the document cache root, and inbound
  metadata records `telegram_media_cached_paths` plus
  `telegram_media_cached_mime_types`.
- Live Telegram dispatch preserves generic-document cached-path evidence in
  signed `telegram.delivery` events and canonical wake envelopes through the
  existing generic media metadata propagation path.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_generic_document -- --nocapture`: failed first because generic documents had no cached media path, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_generic_document_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no generic-document `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 11 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 26 tests.
- `cargo test -j 1 -p zaion-adapters media_cache_ -- --nocapture`: passed, 3 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram generic document download/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Stickers, transcription / model-visible media consumption, outbound native
  media delivery, broader channel propagation, and deeper cancellation/task
  unwind behavior remain open.

## 2026-05-29 Telegram Video Cache Evidence [PARTIAL SLICE]

This checkpoint extends the Telegram media cache path from photos, image
documents, and voice/audio to inbound Telegram native video messages and
video documents. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `MediaCacheManager` now has a dedicated `videos` cache tier plus byte/URL
  video cache helpers and cleanup coverage.
- `TelegramAdapter::receive()` now downloads Telegram `message.video` files
  and Telegram `message.document` files with `video/*` MIME types through Bot
  API `getFile` when a media cache root is configured.
- Returned Telegram `file_path` values still use the existing safe relative
  path validation before file download.
- Video messages and video documents infer common video extensions, default to
  `.mp4` / `video/mp4`, and preserve Telegram `video/*` MIME types when
  supplied.
- Downloaded bytes are cached under the video cache root, and inbound metadata
  records `telegram_media_cached_paths` plus
  `telegram_media_cached_mime_types`.
- Live Telegram dispatch preserves native video and video-document cached-path
  evidence in signed `telegram.delivery` events and canonical wake envelopes
  through the existing generic media metadata propagation path.

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

Label update:

- Telegram native video and video-document download/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Generic document cache policy, stickers, transcription / model-visible media
  consumption, outbound native media delivery, and broader channel propagation
  remain open.

## 2026-05-29 Telegram Voice/Audio Cache Evidence [PARTIAL SLICE]

This checkpoint extends the Telegram media cache path from photos and image
documents to inbound Telegram voice/audio media. Overall latest-Hermes parity
remains `PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter::receive()` now downloads Telegram `message.voice` and
  `message.audio` files through Bot API `getFile` when a media cache root is
  configured.
- Returned Telegram `file_path` values still use the existing safe relative
  path validation before file download.
- Voice notes default to `.ogg` / `audio/ogg`; audio messages infer common
  audio extensions and preserve Telegram `audio/*` MIME types when supplied.
- Downloaded bytes are cached through `MediaCacheManager`'s audio cache, and
  inbound metadata records `telegram_media_cached_paths` plus
  `telegram_media_cached_mime_types`.
- Live Telegram dispatch preserves voice cached-path evidence in signed
  `telegram.delivery` events and canonical wake envelopes through the existing
  generic media metadata propagation path.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_voice_message -- --nocapture`: failed first because no cached voice path existed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_voice_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no voice `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 8 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 23 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram voice/audio download/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Video/generic document cache policy, sticker analysis, outbound native media
  delivery, voice transcription/model-visible audio consumption, and broader
  channel propagation remain open.

## 2026-05-29 Telegram Image-Document Cache Evidence [PARTIAL SLICE]

This checkpoint extends the Telegram media cache path from native photos to
image documents sent as Bot API `message.document`. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter::receive()` now classifies documents with `mime_type`
  starting `image/` as `document_image` instead of generic `document`.
- Image documents call Telegram `getFile`, validate the returned `file_path`,
  download bytes from `/file/bot<TOKEN>/<file_path>`, and cache them through
  `MediaCacheManager`'s image cache.
- Inbound metadata records `telegram_document_file_name`,
  `telegram_document_mime_type`, cached media paths, and cached MIME types.
- Live Telegram dispatch preserves image-document metadata in signed
  `telegram.delivery` events and canonical wake envelopes.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_image_document -- --nocapture`: failed first because image documents were treated as generic documents and no cache path existed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_image_document_dispatches_and_records_media_metadata -- --nocapture`: failed first because live dispatch made no image-document `getFile`/download request, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 7 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 22 tests.

Label update:

- Telegram image-document download/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Voice/audio/video/generic document cache policy, sticker analysis, outbound
  native media delivery, model/tool-visible media consumption, and broader
  channel propagation remain open.

## 2026-05-29 Telegram Cross-Poll Album Debounce Evidence [PARTIAL SLICE]

This checkpoint debounces Telegram photo albums that arrive across adjacent
`getUpdates` polls before live wake dispatch. Overall latest-Hermes parity
remains `PARTIAL`.

Implemented and verified slice:

- Live Telegram runtime now keeps a `TelegramAlbumDebounceBuffer` across polls,
  keyed by chat, topic, and `telegram_media_group_id`.
- Single-photo album fragments are held briefly, merged with later adjacent
  poll fragments, and flushed as one wake turn after a bounded quiet window.
- The merged cross-poll album preserves first caption/trigger text,
  `telegram_album_message_ids`, `telegram_album_update_ids`, media ids,
  cached paths, MIME types, and summed photo counts.
- Pending album state lowers Telegram `getUpdates.timeout` to one second so
  flushes are not hidden behind the default long-poll timeout.
- Already merged same-batch albums still dispatch immediately.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_debounces_photo_album_across_polls_before_dispatch -- --nocapture`: failed first because the first poll dispatched immediately, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_uses_configured_get_updates_timeout -- --nocapture`: failed first because receive timeout was fixed, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_merges_photo_album_before_dispatch -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 6 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 21 tests.

Label update:

- Telegram cross-poll photo album debounce/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Image-document/voice/video/document cache policy, sticker analysis, outbound
  native media delivery, model/tool-visible media consumption, and broader
  channel propagation remain open.

## 2026-05-29 Telegram Photo Album Merge Evidence [PARTIAL SLICE]

This checkpoint merges same-batch Telegram photo albums before live wake
dispatch. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter::receive()` groups same-batch photo messages by chat,
  topic, and `telegram_media_group_id`.
- The merged album keeps the first caption/trigger as the prompt and records
  `telegram_album_message_ids` plus `telegram_album_update_ids`.
- Album metadata appends media types, file ids, unique ids, cached paths, and
  MIME types; photo counts are summed.
- Live Telegram dispatch now produces one wake turn, one outbound reply, and
  one signed `telegram.delivery` for a same-batch album, preserving multiple
  cached media paths.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_merges_photo_album_metadata_and_cached_paths -- --nocapture`: failed first because the adapter emitted two messages for one album, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_merges_photo_album_before_dispatch -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 5 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 20 tests.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram same-batch photo album merge/cache evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Cross-poll album debounce/flush windows, image-document/voice/video/document
  cache policy, sticker analysis, outbound native media delivery, and broader
  channel propagation remain open.

## 2026-05-28 Telegram Photo Download Cache Evidence [PARTIAL SLICE]

This checkpoint adds the first Hermes-style local media cache path for
Telegram photos. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter` can be configured with a media cache root and now calls
  Bot API `getFile` for the largest incoming Telegram photo.
- Returned Telegram `file_path` values are accepted only as safe relative
  paths before downloading `/file/bot<TOKEN>/<file_path>`.
- Downloaded photo bytes are cached through `MediaCacheManager`, producing
  `telegram_media_cached_paths` and `telegram_media_cached_mime_types`.
- Live Telegram runtime stores media under `data_dir()/cache/telegram`, and
  signed `telegram.delivery` plus canonical wake envelopes preserve the cached
  path and MIME metadata.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_downloads_and_caches_largest_photo -- --nocapture`: failed first because the adapter had no cache root/download behavior, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: failed first because no `getFile`/download request or cached-path delivery evidence existed, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 4 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 19 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 21 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 23 tests.

Label update:

- Telegram photo download/cache: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- `media_group_id` album debounce/merge, image-document/voice/video/document
  cache policy, sticker analysis, outbound native media delivery, and broader
  channel propagation remain open.

## 2026-05-28 Telegram Caption Photo Metadata Evidence [PARTIAL SLICE]

This checkpoint makes captioned Telegram photo messages visible to live wake
dispatch and signed delivery evidence. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter::receive()` now uses `message.caption` when `message.text`
  is absent.
- Caption entities are parsed for Telegram mention gating, so direct
  `@zaion_bot` mentions in photo captions can dispatch in allowed groups.
- Incoming photo metadata records `telegram_caption`,
  `telegram_media_group_id`, `telegram_media_types`,
  `telegram_media_file_ids`, `telegram_media_file_unique_ids`, and
  `telegram_photo_count`.
- Signed `telegram.delivery` events and canonical wake envelopes now preserve
  those media fields.

Verification:

- `cargo test -j 1 -p zaion-adapters telegram_receive_preserves_caption_photo_media_metadata -- --nocapture`: failed first because caption/media metadata was absent, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_caption_photo_dispatches_and_records_media_metadata -- --nocapture`: failed first because the captioned photo update did not reach signed media evidence, then passed.
- `cargo test -j 1 -p zaion-adapters telegram_receive_ -- --nocapture`: passed, 3 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ -- --nocapture`: passed, 19 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 20 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 23 tests.
- `git diff --check -- crates/zaion-adapters/src/telegram_adapter.rs crates/zaion-cli/src/commands/network/telegram.rs`: passed.

Label update:

- Telegram caption/photo media metadata: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Hermes-style media download/cache, `media_group_id` album debounce/merge,
  model-visible cached media paths, voice transcription, sticker analysis,
  video/document processing, and broader channel propagation remain open.

## 2026-05-28 Telegram Stop Guard Release Evidence [PARTIAL SLICE]

This checkpoint prevents stopped Telegram background wake turns from leaving a
thread stuck busy. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `TelegramTaskRunner` now tracks active background/held task owner metadata
  by Telegram thread/message.
- `/stop` sends its command response first, then requests cancellation and
  synthesizes signed `telegram.delivery` completions with `status:
  "cancelled"` for unfinished runner-owned tasks.
- Synthetic cancelled completions release the busy guard and return the latest
  queued follow-up once.
- Late completions from already-cancelled task owners are dropped to avoid
  duplicate delivery events or duplicate queue drains.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_stop_synthesizes_cancelled_completion_for_unfinished_task_and_releases_pending -- --nocapture`: failed first because `/stop` did not release the queued follow-up, then passed.
- `cargo test -j 1 -p zaion-cli telegram_task_runner_accepts_stop_while_active_turn_is_in_flight -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_stop_command -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-cli telegram_cancelled_turn_completion_suppresses_reply_and_records_cancelled_delivery -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 22 tests.
- `cargo fmt -p zaion-cli --check`: passed.
- `git diff --check -- crates/zaion-cli/src/commands/network/telegram.rs`: passed.

Label update:

- Telegram stop guard release and stale completion dedupe: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Hermes-style owned async task cancellation with timeout-bounded join/unwind,
  plus propagation across delegated/remote runtime paths and other platform
  adapters, remains open.

## 2026-05-28 Telegram Cancelled Completion Evidence [PARTIAL SLICE]

This checkpoint turns cooperative Telegram wake cancellation into an explicit
completion outcome. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `collect_wake_reply(...)` now records `StreamEvent::Cancelled`.
- Telegram wake task completion checks both the cancellation event and the
  shared `StreamCallback` cancel flag after wake returns.
- Cancelled Telegram turns now skip outbound `sendMessage`, complete with
  `status: "cancelled"`, clear the in-progress reaction, and append signed
  `telegram.delivery` with cancellation reaction evidence.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_cancelled_turn_completion_suppresses_reply_and_records_cancelled_delivery -- --nocapture`: failed first because the completion status was still `sent`, then passed.

Label update:

- Telegram cancelled completion outcome: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Hermes-style owned async task cancellation, bounded join/unwind, and
  propagation across delegated/remote runtime paths and other platform
  adapters remain open.

## 2026-05-28 Telegram Interruptible Wake Runner Evidence [PARTIAL SLICE]

This checkpoint moves live Telegram wake execution off the receive loop so
control messages can be handled while a turn is active. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- Live `run_telegram_loop` now uses a `TelegramTaskRunner` for background wake
  execution and a receive-loop completion drain.
- Active Telegram wake setup registers a shared `StreamCallback` cancel handle
  before the background runner starts.
- `/stop` can now be processed while a test-held active turn remains in
  flight, and it sets the registered cancel handle to `true`.
- Background completions reuse the existing signed `telegram.delivery` audit
  path, unregister active processing markers, and release queued follow-up
  messages.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_task_runner_accepts_stop_while_active_turn_is_in_flight -- --nocapture`: failed first because the runner API did not exist, then passed.
- `cargo test -j 1 -p zaion-cli telegram_stop_command -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-cli telegram_processing_completion_unregisters_active_turn_when_reactions_disabled -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 22 tests.

Label update:

- Telegram interruptible wake runner: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Production cancellation completion semantics still need bounded task
  join/unwind, explicit cancelled outcome, and response-before-cancel ordering
  matching latest Hermes.
- Media batching/cache, retry behavior, delegated/remote propagation, and
  multi-platform equivalents remain open.

## 2026-05-28 Telegram Stop Active Wake Cancel Evidence [PARTIAL SLICE]

This checkpoint connects Telegram `/stop` to Zaion's existing wake cancel
flag. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Telegram processing registry entries can now carry an active wake
  `StreamCallback` cancel handle.
- Live Telegram wake setup registers the active turn's cancel handle before
  `cmd_wake_with_request`.
- `/stop` sets registered active wake cancel flags to `true` and records
  `telegram_reactions: ["cancel_requested"]` on the signed command delivery
  audit for cancel-handle-only entries.
- Existing reaction cleanup still clears marker-only in-progress reactions
  with `setMessageReaction(..., None)`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_stop_command_requests_active_wake_cancellation -- --nocapture`: failed first because `register_active_turn` did not exist, then passed.
- `cargo test -j 1 -p zaion-cli telegram_stop_command_clears_registered_in_progress_reactions -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_processing_reaction_completion_clears_on_cancelled_when_enabled -- --nocapture`: passed.

Label update:

- Telegram `/stop` active wake cancel hook: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- The live Telegram polling loop is still synchronous, so `/stop` cannot yet
  arrive over the same polling lane while a wake/model/tool call blocks that
  lane.
- True Hermes-style active task cancellation, media batching/cache, retry
  behavior, delegated/remote propagation, and multi-platform equivalents.

## 2026-05-28 Telegram Stop Command Reaction Cleanup Evidence [PARTIAL SLICE]

This checkpoint wires the prior Telegram cancellation reaction primitive into
the live command graph for `/stop`. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- Live Telegram processing reactions now maintain a local in-progress reaction
  registry for messages that successfully receive the eyes marker.
- `/stop` is now a stable Telegram command-graph command that returns a safe
  non-turn receipt instead of falling through to wake/model execution.
- When `/stop` is received, Zaion clears all registered in-progress Telegram
  reaction markers with `setMessageReaction(..., None)`.
- The command delivery audit records `telegram_reactions: ["cleared"]` and is
  parented to its signed `telegram.command.stop` receipt.
- Success/failure reaction completion still unregisters the active marker, and
  the existing topic-preserving command reply path remains green.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_stop_command_clears_registered_in_progress_reactions -- --nocapture`: failed first because the registry and `/stop` clear hook did not exist, then passed.
- `cargo test -j 1 -p zaion-cli telegram_processing_reaction_completion_clears_on_cancelled_when_enabled -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_command_reply_preserves_topic_metadata_for_send -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.

Label update:

- Telegram `/stop` command reaction cleanup hook: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- True Hermes-style async live Telegram task cancellation and interrupt
  propagation while wake/model/tool execution is mid-flight.
- Media batching/cache, retry behavior, delegated/remote propagation, and
  multi-platform equivalents.

## 2026-05-28 Telegram Cancellation Reaction Clear Evidence [PARTIAL SLICE]

This checkpoint adds the Hermes-style cancellation reaction cleanup primitive.
Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Telegram processing reactions now use shared start/complete helpers and an
  explicit `TelegramProcessingOutcome`.
- `TelegramProcessingOutcome::Cancelled` clears the in-progress reaction by
  calling `set_message_reaction(..., None)`, matching Hermes' clear-on-cancel
  lifecycle behavior.
- The cancellation helper records a `cleared` reaction audit label.
- Existing live success/default-disabled reaction behavior remains green.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_processing_reaction_completion_clears_on_cancelled_when_enabled -- --nocapture`: failed first because the helper/outcome did not exist, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.

Label update:

- Telegram cancellation reaction clear primitive: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- True live Telegram mid-flight wake/model/tool cancellation after the `/stop`
  command-state cleanup hook.
- Media batching/cache, retry behavior, delegated/remote propagation, and
  multi-platform equivalents.

## 2026-05-28 Telegram Processing Reactions Evidence [PARTIAL SLICE]

This checkpoint adds latest-Hermes-aligned Telegram processing lifecycle
reactions. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `TelegramAdapter` now supports Bot API `setMessageReaction` with emoji
  reaction objects.
- Live Telegram wake processing remains reaction-free by default, matching the
  opt-in nature of Hermes' `TELEGRAM_REACTIONS` gate.
- When `TELEGRAM_REACTIONS=true`, live polling sets an in-progress reaction
  before model/tool processing and swaps it to success or failure after reply
  delivery.
- Signed `telegram.delivery` payloads now include `telegram_reactions` audit
  labels, such as `["eyes", "thumbs_up"]`, while disabled/default runs record
  an empty list.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_mark_processing_lifecycle_when_enabled -- --nocapture`: failed first because no `setMessageReaction` calls were made, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_reactions_ -- --nocapture`: passed, 2 tests.
- `cargo test -j 1 -p zaion-adapters telegram_set_message_reaction_posts_bot_api_payload -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 22 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 19 tests.

Label update:

- Telegram processing lifecycle reactions: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Cancellation/interrupt reaction clearing, media batching/cache, retry
  behavior, delegated/remote propagation, and multi-platform equivalents.

## 2026-05-28 Telegram Observation-Only Group Memory Evidence [PARTIAL SLICE]

This checkpoint adds latest-Hermes-aligned Telegram observation-only group
memory behavior. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `ChannelProfile` now persists optional Telegram
  `observe_unmentioned_group_messages` in `channels.toml` with serde defaults
  for existing channel stores.
- `zaion tg setup --token ... --observe-unmentioned-group-messages true`
  writes the durable observation policy; `--ingest-unmentioned-group-messages`
  is accepted as a legacy alias.
- `TelegramAccessPolicy::from_store` reads durable observation policy,
  `ZAION_TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES`, and legacy
  `ZAION_TELEGRAM_INGEST_UNMENTIONED_GROUP_MESSAGES`.
- `zaion tg doctor` and JSON status expose the effective observe flag.
- Plain unmentioned group/supergroup text is observation-only only after hard
  gates and dispatch triggers, and only when the group chat is explicitly
  allowlisted.
- Live polling writes signed `telegram.observed` with source hash, shared group
  thread id, attributed content, and Telegram metadata, while sending no
  typing/reply and no `telegram.denied` or `telegram.delivery`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_observe_unmentioned_groups_from_env -- --nocapture`: failed first because policy did not read observe env, then passed.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 18 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 20 tests after adding mention-pattern live dispatch evidence.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram observation-only group memory: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group/channel breadth: media batching, reactions,
  retry behavior, delegated/remote propagation, and multi-platform equivalents.
- Equivalent observation diagnostics through wider gateway/channel adapters.

## 2026-05-28 Telegram Mention Patterns Evidence [PARTIAL SLICE]

This checkpoint adds latest-Hermes-aligned Telegram `mention_patterns`
behavior. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `ChannelProfile` now persists optional Telegram `mention_patterns` in
  `channels.toml` with serde defaults for existing channel stores.
- `zaion tg setup --token ... --mention-patterns ...` writes the durable regex
  wake pattern policy alongside allowed chats/topics, ignored threads,
  `guest_mode`, and `free_response_chats`.
- `TelegramAccessPolicy::from_store` reads durable mention patterns and merges
  them with `ZAION_TELEGRAM_MENTION_PATTERNS`, deduping the effective list.
- `zaion tg doctor` and JSON status expose the effective mention pattern list.
- Plain group/supergroup text that matches a configured case-insensitive regex
  can dispatch without `@zaion_bot`, preserving the prompt text.
- Mention-pattern dispatch still respects Hermes-style hard gates: disallowed
  group chats, disallowed topics, ignored topics, and explicit other-bot
  mentions deny before regex dispatch.
- A live fake-API poll now proves regex-matched plain group text performs
  `getUpdates`, sends typing and reply requests, writes signed
  `telegram.delivery` with real chat/topic metadata, and avoids
  `telegram.denied`.

Verification:

- `cargo test -j 1 -p zaion-cli mention_pattern -- --nocapture`: failed first because `TelegramAccessPolicy` had no `mention_patterns` field and `ChannelStore::upsert_telegram_profile_with_policy` lacked the extra argument, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_mention_pattern_dispatches_plain_group_text -- --nocapture`: passed, adding live fake-API evidence over the existing production path.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 16 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 20 tests.
- `cargo fmt -p zaion-cli --check`: passed after formatting.

Label update:

- Telegram mention-pattern regex wake policy: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group policy breadth: observation-only group
  memory, media batching, reactions, and multi-platform equivalents.
- Equivalent mention-pattern diagnostics through delegated execution, remote
  sandbox paths, and wider gateway/channel adapters.

## 2026-05-28 Telegram Free-Response Chats Live Poll Evidence [PARTIAL SLICE]

This checkpoint adds latest-Hermes-aligned Telegram `free_response_chats`
behavior. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `ChannelProfile` now persists optional Telegram `free_response_chats` in
  `channels.toml` with serde defaults for existing channel stores.
- `zaion tg setup --token ... --free-response-chats ...` writes the durable
  free-response policy alongside allowed chats/topics, ignored threads, and
  `guest_mode`.
- `TelegramAccessPolicy::from_store` reads durable free-response chats and
  merges them with `ZAION_TELEGRAM_FREE_RESPONSE_CHATS`, deduping the effective
  policy.
- `zaion tg doctor` and JSON status expose the effective free-response chat
  list.
- Plain group/supergroup text in an approved free-response chat dispatches
  without a direct `@zaion_bot` mention and keeps the prompt unchanged.
- Free-response still respects Hermes-style hard gates: disallowed group chats,
  disallowed topics, and ignored topics deny before free-response dispatch.
- A live fake-API poll proves plain supergroup text in a durable
  free-response chat performs `getUpdates`, model/tool execution,
  `sendChatAction`, `sendMessage`, and signed `telegram.delivery` with real
  chat/topic metadata, without `telegram.denied`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_group_free_response_chat_dispatches_plain_text_without_mention -- --nocapture`: failed first because `TelegramAccessPolicy` had no `free_response_chats` field, then passed.
- `cargo test -j 1 -p zaion-cli telegram_group_free_response_chat_still_respects_hard_group_gates -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_free_response_chat_dispatches_plain_group_text -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_free_response_chats_from_channel_profile -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.

Label update:

- Telegram free-response chat policy: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group policy breadth: configurable mention
  patterns, observation-only group memory, media batching, reactions, and
  multi-platform equivalents.
- Equivalent free-response diagnostics through delegated execution, remote
  sandbox paths, and wider gateway/channel adapters.

## 2026-05-28 Telegram Ignored Threads Live Poll Evidence [PARTIAL SLICE]

This checkpoint adds latest-Hermes-aligned Telegram `ignored_threads`
behavior. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `ChannelProfile` now persists optional Telegram `ignored_threads` in
  `channels.toml` with serde defaults for existing channel stores.
- `zaion tg setup --token ... --ignored-threads ...` writes the durable
  ignored thread/topic policy alongside allowed chats/topics and `guest_mode`.
- `TelegramAccessPolicy::from_store` reads durable ignored threads and merges
  them with `ZAION_TELEGRAM_IGNORED_THREADS`, deduping the effective policy.
- `zaion tg doctor` and JSON status expose the effective ignored thread list.
- Group/supergroup messages in ignored Telegram `message_thread_id` topics are
  silently denied as `telegram_thread_ignored`, even when the message directly
  mentions the configured bot with `@zaion_bot`.
- A live fake-API poll proves ignored-thread direct mentions only perform
  `getUpdates`, append signed `telegram.denied` with real chat/topic metadata,
  send no typing/reply request, and do not append `telegram.delivery`.

Verification:

- `cargo test -j 1 -p zaion-cli telegram_group_ignored_thread_is_denied_even_with_direct_mention -- --nocapture`: failed first because `TelegramDispatchReason::GroupThreadIgnored` did not exist, then passed after adding the policy gate.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: failed first because setup/doctor did not persist or print `ignored_threads`, then passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_ignored_thread_denies_direct_mention_silently -- --nocapture`: passed.

Label update:

- Telegram ignored thread/topic deny policy: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group policy breadth: `free_response_chats`,
  configurable mention patterns, observation-only group memory, media
  batching, reactions, and multi-platform equivalents.
- Equivalent ignored-thread diagnostics through delegated execution, remote
  sandbox paths, and wider gateway/channel adapters.

## 2026-05-28 Telegram Guest-Mode Live Poll Evidence [PARTIAL SLICE]

This checkpoint adds live fake-API evidence for the latest-Hermes Telegram
`guest_mode` slice. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- A real one-poll Telegram adapter path now proves a non-allowlisted
  supergroup message can dispatch when durable `guest_mode=true` and the text
  directly mentions the configured bot with `@zaion_bot`.
- The live proof exercises `getUpdates`, model/tool execution,
  `sendChatAction`, `sendMessage`, prompt mention stripping, and signed
  `telegram.delivery`.
- `telegram.delivery` payloads now copy the same real Telegram chat/topic/
  update/message/reply metadata already preserved by `telegram.denied`, so
  successful deliveries remain auditable by concrete channel context.
- A companion live poll proves ordinary group replies outside the allowlist
  still deny silently as `telegram_group_not_allowed`, send no typing/reply
  request, and do not append `telegram.delivery`.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_guest_mode_allows_direct_mention_outside_group_allowlist -- --nocapture`: failed first because `telegram.delivery.telegram_chat_id` was `Null`, then passed after delivery events copied Telegram metadata.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_guest_mode_denies_group_reply_outside_allowlist -- --nocapture`: passed.

Label update:

- Telegram guest-mode live polling evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group policy breadth: `free_response_chats`,
  `ignored_threads`, configurable mention patterns, observation-only group
  memory, media batching, reactions, and multi-platform equivalents.
- Equivalent guest-mode and delivery metadata propagation through delegated
  execution, remote sandbox paths, and wider gateway/channel adapters.

## 2026-05-28 Telegram Guest-Mode Direct Mention Bypass Evidence [PARTIAL SLICE]

This checkpoint adds the narrow latest-Hermes Telegram `guest_mode` behavior:
non-allowlisted group chats may dispatch only when the current bot is directly
addressed with an explicit `@bot` mention. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- `ChannelProfile` now persists optional Telegram `guest_mode` in
  `channels.toml` with serde defaults for existing channel stores.
- `zaion tg setup --token ... --guest-mode true` writes the durable guest-mode
  policy field alongside allowed chats/topics.
- `TelegramAccessPolicy::from_store` reads durable `guest_mode` and exposes it
  through `zaion tg doctor`.
- Group/supergroup messages outside the allowed chat gate can dispatch when
  `guest_mode` is true and the text directly mentions the configured bot with
  `@zaion_bot`.
- The same guest-mode policy does not let ordinary group replies bypass the
  group allowlist.

Zaion changed files:

- `crates/zaion-cli/src/config.rs`
- `crates/zaion-cli/src/commands/hub.rs`
- `crates/zaion-cli/src/commands/onboard.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/tests/beginner_golden_path.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_guest_mode_allows_direct_bot_mention_outside_group_allowlist -- --nocapture`: failed first because `TelegramAccessPolicy` had no `guest_mode` field, then passed.
- `cargo test -j 1 -p zaion-cli telegram_guest_mode_does_not_allow_group_reply_outside_allowlist -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_guest_mode_from_channel_profile -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli setup_gateway_collects_telegram_owner_allowlist_and_home_channel -- --nocapture`: passed.

Label update:

- Telegram guest-mode direct mention bypass: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group policy breadth: separate
  `group_allowed_chats`, `free_response_chats`, `ignored_threads`,
  configurable mention patterns, observation-only group memory, media
  batching, reactions, and multi-platform equivalents.
- Broader live Telegram proof that guest-mode denials/deliveries carry the
  same metadata through fake-API polling, delegated execution, remote sandbox
  paths, and wider gateway/channel adapters.

## 2026-05-28 Telegram Durable Chat/Topic Policy Config Evidence [PARTIAL SLICE]

This checkpoint productizes the prior env-only Telegram group policy gate into
durable channel configuration and a CLI setup surface. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- `ChannelProfile` now persists optional Telegram `allowed_chats` and
  `allowed_topics` fields in `channels.toml` with serde defaults for existing
  channel files.
- `zaion tg setup --token ... --allowed-chats ... --allowed-topics ...` writes
  the durable group chat/topic policy fields through the same Telegram profile
  path used by `zaion tg set-token`.
- `TelegramAccessPolicy::from_store` now reads durable channel policy values
  and merges them with `ZAION_TELEGRAM_ALLOWED_CHATS` /
  `ZAION_TELEGRAM_ALLOWED_TOPICS`, deduping the combined allowlists.
- `zaion tg doctor` surfaces the effective allowed chat/topic policy so
  operators can inspect the live gate without reading TOML or env vars.
- Existing env-based live group/topic denial behavior remains green.

Zaion changed files:

- `crates/zaion-cli/src/config.rs`
- `crates/zaion-cli/src/commands/hub.rs`
- `crates/zaion-cli/src/commands/onboard.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/tests/beginner_golden_path.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_access_policy_reads_group_gates_from_channel_profile -- --nocapture`: failed first because `upsert_telegram_profile` and `ChannelProfile` had no durable group policy fields, then passed.
- `cargo test -j 1 -p zaion-cli tg_setup_persists_group_allowed_chats_and_topics -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 11 tests.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 14 tests.
- `cargo fmt -p zaion-cli --check`: passed after formatting.

Label update:

- Telegram durable allowed chat/topic config surface: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Hermes-style Telegram group policy breadth: separate
  `group_allowed_chats`, observation-only group memory, guest-mode mention
  bypass, configurable mention patterns, ignored threads, richer
  free-response semantics, media batching, reactions, and multi-platform
  equivalents.
- Equivalent group/topic policy diagnostics across delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

## 2026-05-28 Telegram Allowed Chat/Topic Gate Evidence [PARTIAL SLICE]

This checkpoint adds a latest-Hermes-aligned live Telegram group policy slice
without promoting the whole comparison. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- Live Telegram group/supergroup dispatch now supports opt-in chat and topic
  gates through `ZAION_TELEGRAM_ALLOWED_CHATS` and
  `ZAION_TELEGRAM_ALLOWED_TOPICS`, matching Hermes' latest
  `gateway/platforms/telegram.py` `allowed_chats` / `allowed_topics` shape for
  this narrow runtime gate.
- When a group chat is outside the allowed chat set, dispatch is silently
  denied with `reason = "telegram_group_not_allowed"`.
- When a group message is outside the allowed topic set, dispatch is silently
  denied with `reason = "telegram_topic_not_allowed"`.
- Telegram forum messages without an explicit topic id are treated as General
  topic `1` for topic matching, preserving the Hermes-compatible default.
- A fake-API live poll regression proves an explicit bot mention in an
  allowlisted group but disallowed topic is denied silently, writes
  `telegram.denied`, preserves real chat/topic metadata, sends no typing/reply
  request, and does not append `telegram.delivery`.
- Existing group mention/slash/other-bot and previous live Telegram fallback
  evidence remain green.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_group_allowed_chat_and_topic_can_dispatch_mention -- --nocapture`: failed first because `TelegramAccessPolicy` had no group/topic gate fields, then passed after adding the policy gates.
- `cargo test -j 1 -p zaion-cli telegram_group_disallowed_topic_is_denied_even_with_mention -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_group_disallowed_chat_is_denied_even_with_mention -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_poll_group_allowed_topic_gate_denies_other_topics_silently -- --nocapture`: passed.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 14 tests.
- `cargo test -j 1 -p zaion-cli telegram_group_ -- --nocapture`: passed, 11 tests.
- `cargo fmt -p zaion-cli --check`: passed after formatting.

Label update:

- Telegram allowed chat/topic gate evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Telegram group policy parity: config-file exposure, `group_allowed_chats`
  and observation-only group memory, guest-mode mention bypass, configurable
  mention patterns, ignored threads, richer free-response semantics, media
  batching, reactions, and multi-platform equivalents.
- Equivalent group/topic policy diagnostics across delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Denied Metadata Audit Evidence [PARTIAL SLICE]

This checkpoint extends live Telegram denial audit events with real update
metadata. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `telegram.denied` events now copy real Telegram metadata from the inbound
  update when available.
- Denial payloads can now expose `telegram_chat_id`, `telegram_chat_type`,
  `telegram_message_id`, `telegram_update_id`, `message_thread_id`,
  `telegram_message_thread_id`, `telegram_reply_to_message_id`, and
  `telegram_reply_to_text`.
- A fake-API live poll regression proves a supergroup message without a bot
  trigger is denied silently while the signed denial event preserves chat,
  topic, update, message, and reply context for later policy/debug auditing.
- The denial still does not send typing/reply requests, append
  `telegram.delivery`, or fabricate wake `turn.proof`.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_group_noise_is_denied_from_real_update_metadata -- --nocapture`: failed first because `telegram.denied.telegram_chat_id` was `Null`, then passed after denied events copied Telegram metadata.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 13 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram denial metadata audit evidence: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Full Telegram group policy parity: group chat allowlists, allowed topics,
  guest-mode mention bypass, configurable mention patterns, observation-only
  group memory, media batching, reactions, and multi-platform equivalents.
- Equivalent denied/delivery metadata propagation across delegated execution,
  remote sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Access-Gate Markdown Parse Fallback Evidence [PARTIAL SLICE]

This checkpoint extends the live Telegram access-denial reply delivery
contract with Markdown parse-error retry evidence. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- Live Telegram access-gate denial replies now request Telegram `MarkdownV2`
  formatting through the existing adapter delivery path.
- If Telegram rejects the first denial `sendMessage` with a Markdown entity
  parse error, the adapter retries the same denial reply as plain text.
- The fallback retry removes `parse_mode`, restores the original visible
  denial text, and records `fallbacks = ["markdown_v2_plain_text_retry"]`.
- A fake-API live poll regression proves the first denial MarkdownV2 send
  fails, the plain-text retry succeeds, and
  `telegram.denied.delivery_report` preserves
  `parse_mode = "MarkdownV2"`, the fallback label, and successful Telegram
  message id `884`.
- Access-denial replies remain non-turn access-gate events:
  `telegram.denied` keeps
  `reason = "sender_not_in_telegram_allowlist"` and does not append
  `telegram.delivery` or fabricate `turn.proof`.
- Group-noise denials still do not send typing or reply requests.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_access_denial_markdown_parse_error_retries_plain_text_and_reports_fallback -- --nocapture`: failed first because access-denial replies did not request MarkdownV2 and only one send occurred, then passed after enabling MarkdownV2 on the access-gate reply path.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 13 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 18 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram access-gate MarkdownV2 parse-error fallback reporting:
  `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Broader live Telegram/channel behavior: richer mention/allowlist depth,
  batching, media, reactions, retry policy breadth, topic/reply fallback
  combinations, and multi-platform parity.
- Equivalent Markdown/retry diagnostics across delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Command Markdown Parse Fallback Evidence [PARTIAL SLICE]

This checkpoint extends the live Telegram command-graph reply delivery
contract with Markdown parse-error retry evidence. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- Live Telegram slash-command quick replies handled by `TelegramCommandGraph`
  now request Telegram `MarkdownV2` formatting through the existing adapter
  delivery path.
- If Telegram rejects the first command `sendMessage` with a Markdown entity
  parse error, the adapter retries the same command reply as plain text.
- The fallback retry removes `parse_mode`, restores the original visible
  command reply text, and records
  `fallbacks = ["markdown_v2_plain_text_retry"]`.
- A fake-API live poll regression proves the first command MarkdownV2 send
  fails, the plain-text retry succeeds, and
  `telegram.delivery.delivery_report` preserves
  `parse_mode = "MarkdownV2"`, the fallback label, and successful Telegram
  message id `883`.
- Command replies remain non-turn receipts: `telegram.delivery` keeps
  `runtime = "telegram.command_graph"`, `status = "command_sent"`, and a
  direct `parent_event_id` / `command_receipt_event_id` edge to the command
  receipt without fabricating `turn.proof`.
- Access-denial replies remain scoped to their existing plain-text path.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_command_markdown_parse_error_retries_plain_text_and_reports_fallback -- --nocapture`: failed first because command replies did not request MarkdownV2 and only one send occurred, then passed after enabling MarkdownV2 on the command reply path.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 12 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 18 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram command MarkdownV2 parse-error fallback reporting:
  `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Broader live Telegram/channel behavior: richer mention/allowlist depth,
  batching, media, reactions, retry policy breadth, topic/reply fallback
  combinations, access-denial formatting policy, and multi-platform parity.
- Equivalent Markdown/retry diagnostics across delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Wake Markdown Parse Fallback Evidence [PARTIAL SLICE]

This checkpoint extends the live Telegram wake reply delivery contract with
Markdown parse-error retry evidence. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- Normal live Telegram wake replies now request Telegram `MarkdownV2` formatting
  through the existing adapter path.
- If Telegram rejects the first `sendMessage` with a Markdown entity parse
  error, the adapter retries the same reply as plain text.
- The fallback retry removes `parse_mode`, restores the original visible text,
  and records `fallbacks = ["markdown_v2_plain_text_retry"]`.
- A fake-API live poll regression proves the first MarkdownV2 send fails, the
  plain-text retry succeeds, and `telegram.delivery.delivery_report` preserves
  `parse_mode = "MarkdownV2"`, the fallback label, and successful Telegram
  message id `882`.
- Command quick replies and access-denial replies remain scoped to their
  existing plain-text paths.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_wake_markdown_parse_error_retries_plain_text_and_reports_fallback -- --nocapture`: failed first because live wake replies did not retry after Telegram's Markdown parse error, then passed after enabling MarkdownV2 on the wake reply path.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 11 tests.
- `cargo test -j 1 -p zaion-adapters telegram_ -- --nocapture`: passed, 18 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram wake MarkdownV2 parse-error fallback reporting: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Broader live Telegram/channel behavior: richer mention/allowlist depth,
  batching, media, reactions, retry policy breadth, topic/reply fallback
  combinations, and multi-platform parity.
- Equivalent Markdown/retry diagnostics across command replies, delegated
  execution, remote sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Wake Mention Source-Hash and Topic Reply Fallback Evidence [PARTIAL SLICE]

This checkpoint closes the follow-up wake-path Telegram topic reply fallback
slice without promoting the whole Hermes comparison. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- Live Telegram group mentions now recompute `source_hash` after dispatch
  strips the bot mention and settles the actual wake prompt.
- The canonical Telegram wake envelope is built from the same stripped prompt
  and matching `source_hash`, avoiding a raw-message hash mismatch after
  `@zaion_bot` removal.
- Denied/noise paths still keep their original raw-message source hash, so
  group-noise audit events continue to reflect the message that was denied.
- Normal wake replies now have fake-API live poll coverage for stale topic
  reply-anchor retry, not only command quick replies.
- The wake fallback regression uses a supergroup mention with topic id `77`,
  proves the first reply attempt fails with Telegram's stale replied-message
  error, then proves the retry succeeds without `reply_to_message_id` or
  `message_thread_id`.
- The resulting `telegram.delivery` event keeps
  `runtime = "phase8b.unified_wake"`, `status = "sent"`, records
  `fallbacks = ["thread_reply_anchor_retry"]`, and captures successful
  Telegram message id `881`.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -j 1 -p zaion-cli telegram_live_poll_wake_reply_stale_topic_anchor_fallback_is_recorded -- --nocapture`: passed, 1 test.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 10 tests.

Label update:

- Telegram wake mention source-hash canonicalization and wake stale-topic reply
  fallback reporting: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Broader live Telegram/channel behavior: richer mention/allowlist depth,
  batching, media, Markdown/reactions, retry semantics, topic/reply fallback
  beyond the verified command and wake slices, and multi-platform parity.
- Equivalent diagnostics across delegated execution, remote sandbox paths, and
  broader gateway/channel adapters.

## 2026-05-27 Telegram Command-Graph Delivery Fallback Evidence [PARTIAL SLICE]

This checkpoint closes the interrupted live Telegram command-reply diagnostic
slice without promoting the whole Hermes comparison. Overall latest-Hermes
parity remains `PARTIAL`.

Implemented and verified slice:

- Live Telegram slash-command replies handled by `TelegramCommandGraph` now
  write a `telegram.delivery` event in addition to the command receipt.
- Command delivery events are explicitly labelled with
  `runtime = "telegram.command_graph"` and `status = "command_sent"` or
  `command_send_failed`, so they do not masquerade as normal wake turns.
- Command replies still do not fabricate a `turn.proof`; the command receipt
  remains a `safe_non_turn_receipt`, while `telegram.delivery` carries send
  diagnostics and delivery reports.
- Command delivery events now set `parent_event_id` to the command receipt and
  include `command_receipt_event_id` in the payload, giving operators a direct
  receipt-to-delivery audit edge.
- A fake-API live poll regression proves a stale topic reply anchor fallback is
  recorded: the first `sendMessage` with `reply_to_message_id` and
  `message_thread_id` fails, the retry without the stale anchor succeeds, and
  `telegram.delivery.delivery_report` records
  `fallbacks = ["thread_reply_anchor_retry"]` plus the successful Telegram
  message id.
- Normal wake deliveries keep the existing `phase8b.unified_wake` runtime
  label.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -p zaion-cli telegram_live_poll_stale_topic_reply_fallback_is_recorded_in_delivery_report -- --nocapture`: failed first because the command delivery runtime was still `phase8b.unified_wake`, failed again while delivery lacked a parent command receipt edge, then passed after routing command delivery through the explicit runtime helper and binding it to the command receipt.
- `cargo test -j 1 -p zaion-cli telegram_live_ -- --nocapture`: passed, 9 tests.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Telegram command-graph delivery evidence and stale topic reply fallback
  reporting: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Broader live Telegram/channel behavior: richer mention/allowlist depth,
  batching, media, Markdown/reactions, retry semantics, topic/reply fallback
  beyond command quick replies, and multi-platform parity.
- Equivalent command/delivery diagnostics across delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Proof Binding, Real Update Metadata, and Gateway Resolved Addresses [PARTIAL SLICE]

This checkpoint tightens the Telegram live polling proof chain and carries one
gateway delivery diagnostic field forward without promoting the whole Hermes
comparison. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `telegram.delivery` proof traces are now source-bound. The lookup follows the
  candidate `turn.proof.user_event_id` back to the exact `channel.received`
  event and requires its `source_hash` to match the current Telegram message.
- A same-thread wake failure regression proves a failed Telegram turn no longer
  inherits a stale `turn_proof_event_id`, `tool_receipt_ids`, or storage receipt
  count from a previous successful turn.
- `TelegramAdapter.receive(...)` now preserves real Telegram update metadata
  including chat type, Telegram chat/update/message ids, topic/thread id, and
  reply-to id/text.
- A fake-API live poll regression proves a `supergroup` message without a bot
  trigger is denied as group noise from real adapter metadata, writes
  `telegram.denied`, and does not send typing or reply calls.
- API runtime webhook delivery JSON now preserves `resolved_addrs`, making
  DNS/target resolution evidence visible to gateway consumers.

Zaion changed files:

- `crates/zaion-adapters/src/telegram_adapter.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/src/commands/network/routes.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo test -p zaion-cli telegram_live_wake_failure_does_not_inherit_prior_thread_proof -- --nocapture`: failed first on stale proof inheritance, then passed after source-bound lookup.
- `cargo test -p zaion-cli telegram_live_ -- --nocapture`: passed with `CARGO_BUILD_JOBS=1` / `cargo test -j 1`.
- `cargo test -p zaion-adapters telegram_receive_preserves_topic_and_reply_metadata -- --nocapture`: failed first on missing metadata, then passed.
- `cargo test -p zaion-cli telegram_live_poll_group_noise_is_denied_from_real_update_metadata -- --nocapture`: passed.
- `cargo test -p zaion-cli api_runtime_delivery_result_preserves_resolved_addrs -- --nocapture`: passed.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_large_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: passed.
- `cargo fmt -p zaion-adapters -p zaion-cli --check`: passed.

Label update:

- Telegram source-bound live delivery proof trace: `PARTIAL SLICE`.
- Telegram real update metadata and live group-noise denial: `PARTIAL SLICE`.
- Gateway runtime delivery resolved address visibility: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Broader live Telegram/channel behavior: mention/allowlist depth, batching,
  media, Markdown/reactions, topic/reply fallback polish, retry semantics, and
  multi-platform parity.
- Equivalent source-bound proof and storage propagation through delegated
  execution, remote sandbox paths, and broader gateway/channel adapters.

## 2026-05-27 Telegram Live Polling Tool-Result Storage Receipt E2E [PARTIAL SLICE]

This checkpoint closes the prior live Telegram polling large-output storage
receipt follow-up without promoting the whole Hermes comparison. Overall
latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Telegram live polling now has finite one-poll fake Telegram API E2E coverage
  through the real `TelegramAdapter.receive(...)` path.
- The shared live handler path runs
  `process_live_telegram_message_once(...) -> cmd_wake_with_request(...) ->
  native fs_search large output -> persisted storage receipt summary`.
- `telegram.delivery` now has verified coverage for
  `tool_result_storage_receipt_count == 1` on the live polling path, not only
  `zaion tg simulate`.
- The live loop keeps the same forever-polling production shape while the
  extracted one-message handler and test-only one-poll helper make the behavior
  verifiable.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `MASTER_PLAN.md`
- `plans/openclaw_latest_gap_report.md`
- `plans/hermes_surpass_master_plan.md`
- `docs/zaion_vs_hermes.md`

Verification:

- `cargo fmt -p zaion-cli --check`: passed.
- `cargo test -p zaion-cli telegram_live_ -- --nocapture`: passed after a
  broad parallel run first hit rustc OOM/stack-overrun during compilation; the
  same filter passed with `CARGO_BUILD_JOBS=1` / `cargo test -j 1`.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_large_tool_call_exposes_persisted_storage_receipt_summary -- --nocapture`: passed.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: passed.

Label update:

- Telegram live polling storage receipt E2E: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Richer live Telegram behavior: bot mention trigger context, allowlist/group
  nuances, batching, media, Markdown/reactions, retry behavior, and topic/reply
  fallback.
- Equivalent storage/proof propagation through delegated execution, remote
  sandbox paths, and broader gateway/channel adapters.

## 2026-05-26 Service Wake Tool-Result Storage Receipt Summary [PARTIAL SLICE]

This checkpoint extends the verified service/channel wake receipt response
contract with persisted tool-result storage receipt summaries. Overall
latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `receipt_join.rs` now exposes `tool_result_storage_receipts(...)`, a shared
  helper that resolves returned `tool.receipt` ids and summarizes only receipts
  with non-null `tool_result_storage`.
- Summary entries include receipt event id, signed status, tool identity,
  receipt status, `tool_result_storage`, and
  `tool_result_storage_binding`.
- MCP HTTP wake responses, API `/v1/runs` wake responses, ACP stdio wake
  results, webhook synchronous wake `agent_trigger` results, and Telegram
  delivery payloads now expose `tool_result_storage_receipts` and
  `tool_result_storage_receipt_count`.
- No-storage local tool turns expose stable empty arrays/count `0`; `tg
  simulate --no-llm` also writes explicit empty/default storage receipt fields.
- ACP stdio protocol coverage includes a non-empty injected mock storage
  receipt, proving protocol JSON can carry backend/environment binding
  summaries.
- True large-output local wake E2E now covers MCP HTTP wake, API `/v1/runs`,
  webhook synchronous `agent_trigger`, and ACP stdio wake. Each path executes a
  native `fs_search` call large enough to persist tool output, returns a
  non-empty `tool_result_storage_receipts` array/count `1`, and verifies the
  stored output file exists under workspace-visible `.zaion/tool-results`.
- True large-output local wake E2E now also covers `zaion tg simulate`
  delivery, including the visible `tool_storage_count     : 1` trace and the
  persisted storage receipt summary written to the `telegram.delivery` ledger
  event.

Zaion changed files:

- `crates/zaion-a2a/src/stdio_service.rs`
- `crates/zaion-adapters/src/webhook_runtime.rs`
- `crates/zaion-cli/src/commands/mcp.rs`
- `crates/zaion-cli/src/commands/network/routes.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/src/commands/receipt_join.rs`
- `crates/zaion-cli/src/commands/system.rs`
- `crates/zaion-cli/src/commands/webhook/webhook_serve.rs`
- `crates/zaion-cli/tests/beginner_golden_path.rs`
- `crates/zaion-cli/tests/phase8_surface.rs`

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

Label update:

- Service wake tool-result storage receipt summaries: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Richer live Telegram/channel semantics beyond the verified one-poll storage
  receipt E2E.
- Carry equivalent storage receipt summaries through delegated execution,
  remote sandbox paths, and broader gateway/channel adapters.

## 2026-05-26 Explicit Tool-Result Environment Identity [PARTIAL SLICE]

This checkpoint replaces the local-only storage-root identity fallback with an
optional explicit backend identity path for persisted tool results. Overall
latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `ToolResultStorageTarget` can now expose optional `environment_id` and
  `environment_kind` values.
- `ToolResultMetadata` now records those optional environment fields for
  persisted oversized tool outputs.
- `HostToolResultStorageTarget::with_environment(...)` lets structured callers
  bind a real backend identity to a storage root.
- `WakeRequest` now carries optional `tool_result_environment_id` and
  `tool_result_environment_kind` fields.
- Wake builds its host tool-result target through
  `wake_tool_result_storage_target(...)`, carrying explicit identity when
  present.
- Signed wake receipt `tool_result_storage_binding.environment` now prefers the
  explicit identity/kind from storage metadata and still falls back to
  `storage-root:<hash>` / `storage_target` for local/default targets.

Zaion changed files:

- `crates/zaion-runtime/src/tool_result_storage.rs`
- `crates/zaion-cli/src/commands/process/wake.rs`

Verification:

- `cargo fmt -p zaion-runtime -p zaion-cli --check`: passed.
- `cargo test -p zaion-runtime tool_result_metadata_records_explicit_environment_identity_from_target -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_request_tool_result_environment_identity_reaches_host_storage_target -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_receipt_binding_prefers_explicit_environment_identity -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: passed.
- `cargo test -p zaion-runtime tool_result_large_output_can_spill_through_active_environment_storage_target -- --nocapture`: passed.

Label update:

- Explicit tool-result environment identity: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Thread real remote Modal/Docker/Daytona/SSH environment ids into callers once
  those backend selectors exist.
- Carry equivalent environment identity through delegated execution and broader
  gateway/channel adapters.

## 2026-05-26 ACP/Webhook/Telegram Wake Receipt/Proof Propagation [PARTIAL SLICE]

This checkpoint propagates the existing wake receipt/proof join contract through
the local service/channel response set instead of leaving it only in local
ledger/query surfaces. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- MCP HTTP `runtime_route=wake` responses now expose `tool_receipt_ids`,
  `tool_receipt_count`, `tool_receipt_proof_join_event_id`,
  `tool_receipt_proof_join`, `tool_receipt_join_found`, and
  `tool_receipt_proof_hash_verified` when the wake turn executed tools.
- API `/v1/runs` wake responses now expose the same receipt/proof join summary
  for tool-using turns.
- ACP stdio wake JSON-RPC results expose the same receipt ids/count and signed
  proof-join summary.
- Webhook synchronous wake `agent_trigger` results expose the same receipt
  ids/count and signed proof-join summary.
- Telegram live delivery traces and `zaion tg simulate` now expose the same
  receipt/proof summary; `tg simulate --no-llm` writes explicit empty/default
  receipt/proof fields instead of omitting them.
- All populated response paths decode the signed `TurnProof`, locate the signed
  `tool.receipt.proof_join` event by exact `tool_receipt_ids` array membership,
  and report whether the join's proof hash/event id match the returned
  `turn.proof`.
- Direct MCP HTTP tool calls remain intentionally `receipt_only`; they still
  do not fabricate a turn proof.
- Shared receipt/proof extraction is centralized in
  `crates/zaion-cli/src/commands/receipt_join.rs` and reused by ACP, webhook,
  MCP/API, and Telegram callers.
- MCP HTTP and API run response builders now call that shared helper instead of
  keeping private copy-pasted proof-join lookup/summary functions.

Zaion changed files:

- `crates/zaion-a2a/src/stdio_service.rs`
- `crates/zaion-adapters/src/webhook_runtime.rs`
- `crates/zaion-cli/src/commands/mcp.rs`
- `crates/zaion-cli/src/commands/mod.rs`
- `crates/zaion-cli/src/commands/network/routes.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/src/commands/receipt_join.rs`
- `crates/zaion-cli/src/commands/system.rs`
- `crates/zaion-cli/src/commands/webhook/webhook_serve.rs`
- `crates/zaion-cli/tests/beginner_golden_path.rs`
- `crates/zaion-cli/tests/cli_stable_surface.rs`
- `crates/zaion-cli/tests/phase8_surface.rs`

Verification:

- `cargo fmt -p zaion-a2a -p zaion-cli -p zaion-adapters --check`: passed.
- `cargo test -p zaion-a2a acp_stdio_create_run_can_route_through_injected_wake_runtime -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_runtime_route_wake_joins_stable_turn_proof_chain -- --nocapture`: passed.
- `cargo test -p zaion-cli direct_mcp_http_call_executes_builtin_tool_with_signed_receipt -- --nocapture`: passed.
- `cargo test -p zaion-cli api_create_run_wake_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_create_run_executes_wake_runtime_and_returns_turn_proofs -- --nocapture`: passed.
- `cargo test -p zaion-cli --test beginner_golden_path telegram_simulate_tool_call_exposes_receipt_proof_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli --test phase8_surface phase8b_telegram_simulate_proves_local_delivery_chain -- --nocapture`: passed.
- `cargo test -p zaion-cli --test cli_stable_surface doctor_source_gate_locks_shared_receipt_join_helper_for_service_wake_surfaces -- --nocapture`: failed first on private MCP/API helpers, then passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_parser_tool_call_records_permission_receipt -- --nocapture`: passed.

Label update:

- ACP/Webhook/Telegram/MCP/API wake receipt/proof response propagation:
  `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Carry equivalent receipt/proof response propagation through delegated
  execution, remote sandbox paths, and broader gateway/channel adapters beyond
  the currently verified local wake surfaces.
- Replace storage-root-derived local environment ids with real non-local
  backend identities.

## 2026-05-26 Delegation Receipt Trace [PARTIAL SLICE]

This checkpoint makes delegated proof records inspectable without pretending
delegation is a generic `tool.receipt`. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- `zaion agent receipt-trace <pid> <delegation-proof-event-id>` now resolves a
  signed `delegation.proof` event.
- The trace recomputes the deterministic `merge_receipt` from principal,
  delegate, task, scope, input hash, and output hash.
- The trace verifies the A2A delegation message signature against the local
  principal key that created the proof.
- The Phase 8 surface regression now exercises
  `agent proof -> agent receipts -> agent receipt-trace` and requires
  `merge_receipt_verified`, `message_signature_valid`, and
  `runtime_scope : delegation_proof`.

Zaion changed files:

- `crates/zaion-cli/src/commands/network/agent.rs`
- `crates/zaion-cli/tests/phase8_surface.rs`

Verification:

- `cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.

Label update:

- Delegation receipt trace: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Carry equivalent receipt/proof traceability into live delegated execution,
  gateway, ACP, and MCP paths.
- Keep delegation proofs semantically separate from tool receipts unless a
  future runtime produces an actual delegated tool execution receipt.

## 2026-05-25 Tool Receipt Trace Surfaces [PARTIAL SLICE]

This checkpoint exposes the local receipt/proof join through direct CLI,
turn-inspection, and MCP diagnostic surfaces. Overall latest-Hermes parity
remains `PARTIAL`.

Implemented and verified slice:

- `zaion tool receipts <pid>` now prints each local receipt ledger event id as
  `event_id=...`.
- `zaion tool receipt-trace <pid> <receipt-event-id>` resolves a
  `tool.receipt` event, follows the signed `tool.receipt.proof_join` event via
  receipt-id array membership, resolves the linked `turn.proof`, and
  recomputes the turn proof hash from normalized `TurnProof` material.
- The beginner golden path now extracts a receipt event id from
  `zaion tool receipts` and asserts that `receipt-trace` reports
  `join_found`, `proof_found`, and `proof_hash_verified` as `yes`.
- `zaion turn trace <proof-event-id> --pid <pid>` now reports receipt join
  count, join presence, proof linkage, and join/proof hash match for proofs
  that contain `tool_receipt_ids`.
- The native MCP tool registry now includes `tool_receipt_trace`, a compact
  diagnostic tool that traces a local receipt to `tool.receipt.proof_join` and
  verifies the linked turn proof hash without exposing full ledger payloads.

Zaion changed files:

- `crates/zaion-cli/src/commands/tool.rs`
- `crates/zaion-cli/src/commands/turn.rs`
- `crates/zaion-cli/tests/beginner_golden_path.rs`
- `crates/zaion-mcp/src/builtin_tools.rs`

Verification:

- `cargo test -p zaion-cli wake_parser_tool_call_records_permission_receipt -- --nocapture`: passed.
- `cargo test -p zaion-mcp tool_receipt_trace -- --nocapture`: passed.
- `cargo test -p zaion-cli chat_executes_native_tool_call_without_mcp -- --nocapture`: passed.

Label update:

- Tool receipt trace surfaces: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Propagate receipt/proof joins through delegated, remote sandbox, gateway, and
  MCP execution paths.
- Replace storage-root-derived environment ids with real non-local backend
  environment identities.

## 2026-05-25 Ledger Receipt Proof Join Lookup [PARTIAL SLICE]

This checkpoint adds ledger-level lookup ergonomics for the new
receipt/proof join events. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `EventLedger::list_events_by_payload_string_array_contains(...)` now lists
  newest events whose top-level payload array contains an exact string value.
- The helper narrows by namespace and event type in SQLite, parses payload JSON
  in Rust, and avoids relying on SQLite JSON1 availability.
- The new regression proves `tool.receipt.proof_join` events can be looked up
  by `tool_receipt_ids` membership, newest-first, while excluding scalar
  lookalikes and other event types.
- Existing scalar payload lookup behavior remains covered by its adjacent
  regression.

Zaion changed files:

- `crates/zaion-ledger/src/ledger.rs`
- `crates/zaion-ledger/src/tests.rs`

Verification:

- `cargo test -p zaion-ledger test_list_events_by_payload_string_array_contains_returns_latest_exact_matches -- --nocapture`: failed first on the missing helper, then passed.
- `cargo test -p zaion-ledger test_list_events_by_payload_string_returns_latest_exact_matches -- --nocapture`: passed.
- `cargo test -p zaion-ledger -- --nocapture`: 30 passed.
- `cargo check -p zaion-ledger`: passed.
- `cargo fmt -p zaion-ledger -p zaion-types -p zaion-cli --check`: passed.

Label update:

- Ledger receipt/proof join lookup: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Consider dedicated SQL indexes only if measured receipt/proof lookup volume
  outgrows the current namespace/event-type narrowed scan.
- Keep extending the join contract beyond local wake/CLI/MCP diagnostics into
  non-local runtime paths.

## 2026-05-25 Wake Tool Receipt Proof Join [PARTIAL SLICE]

This checkpoint adds an append-only wake proof/receipt join event so consumers
can follow signed tool receipts forward to the later turn-proof event without
mutating already-written receipt events. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- Added `EventType::ToolReceiptProofJoin` with wire string
  `tool.receipt.proof_join`.
- Wake appends a signed `tool.receipt.proof_join` event after `turn.proof`
  whenever signed tool receipt ids exist.
- The join event is parented to the `turn.proof` event and records
  `tool_receipt_ids`, receipt count, `turn_proof_event_id`,
  `turn_proof_hash`, answer/output/user event ids, lineage, and `join_hash`.
- No join event is written for turns without tool receipts.
- The event-type invariant suite now locks `tool.receipt.proof_join` as a
  stable ledger wire string.

Hermes evidence:

- `tools/tool_result_storage.py`
- `tools/tool_output_limits.py`
- `agent/tool_executor.py`
- `tools/environments/base.py`

Zaion changed files:

- `crates/zaion-cli/src/commands/process/wake.rs`
- `crates/zaion-types/src/event.rs`
- `crates/zaion-types/tests/invariants.rs`

Verification:

- `cargo test -p zaion-cli wake_tool_receipt_proof_join_event_links_receipts_to_turn_proof -- --nocapture`: failed first on missing join support, then passed.
- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: passed.
- `cargo test -p zaion-runtime turn_proof_records_tool_receipt_ids_in_lineage -- --nocapture`: passed.
- `cargo test -p zaion-types event -- --nocapture`: passed.

Label update:

- Wake tool receipt proof join: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Thread the same receipt/proof join contract through delegated, remote
  sandbox, gateway, and MCP execution paths.
- Replace storage-root-derived environment ids with real non-local backend
  environment identities.
- Keep local query surfaces aligned as the join contract expands beyond wake.

## 2026-05-25 Wake Tool Receipt Provenance Binding [PARTIAL SLICE]

This checkpoint binds persisted wake tool-output storage metadata to permission
scope, provenance, and turn-proof lineage. Overall latest-Hermes parity
remains `PARTIAL`.

Implemented and verified slice:

- Signed wake `tool.receipt` payloads now include
  `tool_result_storage_binding` when an oversized tool output was persisted.
- The binding records storage-root-derived environment identity, storage path,
  permission scope, permission proof hash, principal/namespace/channel/thread
  provenance, parent output event id, tool identity, argument/output hashes,
  turn material, and a binding hash.
- `append_tool_receipts(...)` now returns signed receipt event ids.
- Wake `RuntimeOutput.tool_receipt_ids` now exposes those receipt ids.
- `TurnProofInput` / `TurnProof` now carry `tool_receipt_ids` and
  `tool_receipt_count`; turn-proof lineage includes receipt event ids.
- Receipt-side `turn_proof_event_id` and `turn_proof_hash` remain `null`
  because receipts are appended before the proof event; the proof links back
  to receipts in append-only lineage.

Hermes evidence:

- `tools/tool_result_storage.py`
- `tools/tool_output_limits.py`
- `agent/tool_executor.py`
- `tools/environments/base.py`

Zaion changed files:

- `crates/zaion-cli/src/commands/process/wake.rs`
- `crates/zaion-runtime/src/turn_proof.rs`
- `crates/zaion-cli/src/commands/process_unified.rs`

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

Label update:

- Wake persisted-output receipt provenance binding: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Thread the same binding through delegated, remote sandbox, gateway, and MCP
  execution paths.
- Replace storage-root-derived environment ids with real non-local backend
  environment identities.

## 2026-05-25 Wake Tool Receipt Storage Metadata [PARTIAL SLICE]

This checkpoint adds persisted-output storage metadata to signed wake tool
receipts. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Wake `ToolExecutionRecord` now carries optional `ToolResultMetadata` from
  per-result tool budgeting and aggregate turn-budget enforcement.
- Successful todo, native, and MCP tool execution paths retain metadata from
  `maybe_store_tool_result_with_target(...)`.
- Signed `tool.receipt` payloads now include a compact `tool_result_storage`
  object for persisted oversized outputs, including schema, tool name, tool
  call id, stored/truncated flags, byte counts, persisted path, and storage
  root.
- The receipt preserves permission proof alongside storage metadata and avoids
  embedding the full preview in the ledger payload.

Hermes evidence:

- `tools/tool_result_storage.py`
- `tools/tool_output_limits.py`
- `agent/tool_executor.py`
- `tools/environments/base.py`

Zaion changed file:

- `crates/zaion-cli/src/commands/process/wake.rs`

Verification:

- `cargo test -p zaion-cli wake_tool_receipt_records_persisted_output_storage_metadata -- --nocapture`: failed first on missing `tool_result_storage`, then passed.
- `cargo test -p zaion-cli wake_tool_context -- --nocapture`: 4 passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed.
- `cargo fmt -p zaion-cli --check`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.

Label update:

- Wake persisted-output receipt metadata: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Bind persisted tool-output receipts to explicit environment identity,
  permission scope, provenance chain, and turn-proof material.
- Thread the same receipt metadata through delegated, remote sandbox, gateway,
  and MCP execution paths.

## 2026-05-25 Structured Wake Caller Tool-Result Root [PARTIAL SLICE]

This checkpoint extends the workspace-visible tool-result spill root across the
local structured wake caller set. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slice:

- Wake now exposes `workspace_tool_result_storage_root()`, matching the local
  live default of `cwd/.zaion/tool-results` while preserving the existing
  `data_dir()/tool-results` fallback when cwd cannot be resolved.
- Structured wake callers now build `WakeRequest` values through the shared
  canonical helper path, attaching the canonical envelope and explicitly
  setting the workspace-visible tool-result root.
- Verified local structured callers include API runs, MCP HTTP wake route,
  webhook agent dispatch, ACP stdio wake route, Telegram live polling, and
  `zaion tg simulate`.
- The doctor architecture source gate now locks the MCP HTTP route to
  `mcp_http_wake_request(pid.clone(), envelope.clone())` and the helper to
  `structured_wake_request(pid, envelope.body.clone(), envelope)`, matching the
  current canonical-envelope helper architecture instead of the old inline
  builder chain.
- The same source gate now locks ACP stdio wake ingress to
  `acp_stdio_wake_request(...)` and
  `structured_wake_request(submitter_principal, message, envelope)`, replacing
  the stale inline `.with_envelope(...)` proof needle.
- This closes the local service-cwd ambiguity slice for structured wake calls.
  Delegated execution, remote sandbox environment selection, and explicit
  environment/provenance/turn-proof binding for persisted-output receipts
  remain open.

Hermes evidence:

- `tools/tool_result_storage.py`
- `tools/tool_output_limits.py`
- `tools/environments/base.py`
- `gateway/platforms/telegram.py`
- `gateway/platforms/webhook.py`
- `gateway/run.py`
- `mcp_serve.py`
- `acp_adapter/server.py`

Zaion changed files:

- `crates/zaion-cli/src/commands/process/wake.rs`
- `crates/zaion-cli/src/commands/process/mod.rs`
- `crates/zaion-cli/src/commands/network/routes.rs`
- `crates/zaion-cli/src/commands/mcp.rs`
- `crates/zaion-cli/src/commands/webhook/webhook_serve.rs`
- `crates/zaion-cli/src/commands/system.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/tests/cli_stable_surface.rs`

Verification:

- `cargo test -p zaion-cli structured_wake_request_from_envelope_defaults_to_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli api_run_structured_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli mcp_http_runtime_route_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli webhook_agent_dispatch_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli acp_stdio_wake_request_uses_workspace_tool_result_root -- --nocapture`: passed.
- `cargo test -p zaion-cli telegram_live_wake_request_uses_workspace_tool_result_root -- --nocapture`: failed in RED with `tool_result_storage_root == None`, then passed.
- `cargo test -p zaion-cli telegram_simulate_wake_request_uses_workspace_tool_result_root -- --nocapture`: failed in RED with `tool_result_storage_root == None`, then passed.
- `cargo test -p zaion-cli telegram_channel_commands_share_one_effective_token_source -- --nocapture`: failed first on the stale doctor source gate, then passed after the gate was updated to the structured helper pattern.
- `cargo test -p zaion-cli telegram -- --nocapture`: 25 matching Telegram-related tests passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed.
- `cargo test -p zaion-cli wake_request_tool_result_storage_root_overrides_default_budget_root -- --nocapture`: passed.
- `cargo test -p zaion-cli doctor_source_gate_locks_acp_canonical_envelope_ingress -- --nocapture`: failed in RED on the stale ACP source gate, then passed.
- `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix -- --nocapture`: passed.
- `cargo fmt -p zaion-cli --check`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.

Label update:

- Local structured wake caller tool-result root: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Thread explicit active-environment storage roots through delegated execution,
  remote sandbox runners, and non-local environment-backed tool paths.
- Bind persisted tool-output receipts to explicit environment identity,
  permission scope, provenance chain, and signed turn proof material.

## 2026-05-23 Active-Environment Tool Result Storage Target [PARTIAL SLICE]

This checkpoint records the active-environment-visible tool-result spill slice.
Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Runtime tool-result spill APIs now accept a `ToolResultStorageTarget`, so
  oversized tool outputs can be written through a caller-supplied storage
  boundary instead of always using the host data directory.
- `HostToolResultStorageTarget` preserves the existing host-backed behavior for
  explicit host callers and fallback paths.
- `maybe_store_tool_result_with_target(...)` and
  `enforce_turn_budget_with_target(...)` now let per-result and aggregate
  budgeting write full output to an active environment path while injecting a
  model-visible persisted-output pointer for later inspection.
- Wake helper tests prove both single-result spill and aggregate turn-budget
  spill can use a fake active-environment target, and that no host fallback
  file is written in those target-backed paths.
- Wake native tool execution helpers now accept the same budget config and
  storage target used by aggregate turn-budget enforcement, so successful
  native/MCP/todo tool results can spill through a caller-supplied target
  before re-entering provider context.
- Default local live wake now resolves its tool-result budget storage root to
  the current workspace's `.zaion/tool-results`, so oversized local tool output
  is visible from the same working directory used by native `fs_*` and
  `shell_exec` tools instead of being hidden under the host data directory.
- Structured wake callers can now set `WakeRequest::tool_result_storage_root`
  via `with_tool_result_storage_root(...)`; live wake uses that explicit root
  before falling back to the local workspace default, giving TUI/gateway/MCP
  integrations a concrete way to avoid service-cwd ambiguity.
- TUI local model-turn requests now capture the TUI startup workspace root in
  `AppState` and pass `workspace_root/.zaion/tool-results` explicitly through
  `WakeRequest`, so TUI worker turns do not depend on a later process cwd guess.

Hermes evidence:

- `tools/tool_result_storage.py`
- `tools/tool_output_limits.py`
- `tools/environments/base.py`
- `agent/tool_executor.py`
- `tools/terminal_tool.py`

Zaion changed files:

- `crates/zaion-runtime/src/tool_result_storage.rs`
- `crates/zaion-runtime/src/lib.rs`
- `crates/zaion-cli/src/commands/process/wake.rs`
- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-runtime tool_result_storage -- --nocapture`: 8 passed.
- `cargo test -p zaion-cli wake_tool_context -- --nocapture`: 4 passed.
- `cargo test -p zaion-cli wake_native_tool_calls_use_active_environment_target_for_per_result_spill -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_live_tool_result_budget_ -- --nocapture`: 2 passed after
  `wake_live_tool_result_budget_defaults_to_workspace_visible_dir` first failed on the
  old `data_dir()/tool-results` default.
- `cargo test -p zaion-cli wake_request_tool_result_storage_root_overrides_default_budget_root -- --nocapture`:
  failed first on the missing structured override API, then passed.
- `cargo test -p zaion-cli tui_model_turn_request_ -- --nocapture`: 2 passed
  after the new TUI request-root coverage first failed on the missing helper and
  startup workspace field.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.
- `cargo fmt -p zaion-runtime -p zaion-cli --check`: passed.

Label update:

- Active-environment-capable tool-result spill target: `PARTIAL SLICE`.
- Default local live wake now uses `cwd/.zaion/tool-results` for workspace-visible
  spill, structured callers can override the root explicitly, and TUI local
  model turns now pass a captured startup workspace root. This closes the local
  host-hidden default and one real structured TUI caller path, but remote
  sandbox, gateway, MCP, and delegated execution paths still need real active
  environment selection.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Supply a real active sandbox/environment target from non-local live tool
  execution environments into wake requests, gateway, and broader tool/MCP
  execution paths.
- Thread caller-supplied `tool_result_storage_root` through gateway, MCP,
  delegated, and other service-launched wake requests whose current directory
  is not the intended workspace.
- Add richer storage receipts tying persisted tool outputs to environment
  identity, provenance, and permission scope.

## 2026-05-23 Wake Todo State Redaction and Size Caps [PARTIAL SLICE]

This checkpoint hardens durable wake todo-state persistence. Overall
latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Wake now sanitizes durable todo state immediately before appending
  `zaion.session_todo.state.v1` to the signed ledger.
- `state_json`, structured `state`, and `state_hash` are all derived from the
  same sanitized JSON string.
- Todo `title` and Hermes-compatible `content` fields are redacted for obvious
  secrets and capped at 512 characters before append-only persistence.
- Todo `notes` fields are redacted for obvious secrets and capped at 2048
  characters before append-only persistence.
- Hydration continues to read `state_json`, so later wake turns restore the
  sanitized durable state rather than secret-bearing oversized state.

Zaion changed file:

- `crates/zaion-cli/src/commands/process/wake.rs`

Verification:

- `cargo test -p zaion-cli wake_todo_state_event_redacts_and_caps_durable_strings_before_ledger_write -- --nocapture`: failed first on the old unsanitized ledger write, then passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 7 passed.

Label update:

- Wake durable todo-state redaction and size caps: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Gateway/channel runtimes still need the same durable todo hydration
  contract.
- Future oversized todo state may need sealed external storage plus a capped
  ledger preview if full content must be retained.

## 2026-05-23 Payload-Queryable Wake Todo State Lookup [PARTIAL SLICE]

This checkpoint removes the bounded over-fetch risk in durable wake todo-state
hydration. Overall latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- `EventLedger` now exposes `list_events_by_payload_string(...)`, which narrows
  by `namespace_key` + `event_type` in SQLite, parses payload JSON through the
  existing ledger event decoder, and returns newest-first exact string matches
  without relying on SQLite JSON1 availability.
- Ledger schema initialization now ensures a composite
  `idx_events_namespace_type_seq` index for newest-first scans inside a
  namespace/event-type slice.
- Wake todo hydration now asks the ledger for the latest
  `zaion.session_todo.state.v1` event whose payload `thread_id` matches the
  current thread, instead of reading a fixed recent window and filtering it
  after the fact.
- Regression coverage now proves that 600 newer todo-state events for other
  threads cannot hide an older matching target-thread state.

Zaion changed files:

- `crates/zaion-ledger/src/ledger.rs`
- `crates/zaion-ledger/src/tests.rs`
- `crates/zaion-cli/src/commands/process/wake.rs`

Verification:

- `cargo test -p zaion-ledger test_list_events_by_payload_string_returns_latest_exact_matches -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_todo_state_hydration_is_not_shadowed_by_newer_other_threads -- --nocapture`: passed after failing first on the old bounded-window implementation.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 6 passed.
- `cargo test -p zaion-ledger -- --nocapture`: 29 passed.
- `cargo fmt -p zaion-cli -p zaion-ledger --check`: passed.
- `cargo check -p zaion-ledger`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.

Label update:

- Wake durable todo-state thread lookup: `PARTIAL SLICE`, now queryable and
  no longer bounded-over-fetch based.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Gateway/channel runtimes still need the same durable todo hydration contract.

## 2026-05-23 Durable Wake Todo State Hydration [PARTIAL SLICE]

This checkpoint records a live wake todo-continuity slice. Overall
latest-Hermes parity remains `PARTIAL`.

Implemented and verified slice:

- Successful wake `todo` tool calls now keep a full-store
  `todo_store.response()` JSON snapshot separate from the model-visible
  response, so filtered `todo list` calls cannot accidentally persist a
  truncated durable state.
- Wake appends a signed `zaion.session_todo.state.v1` event after
  `channel.sent`, parented to the sent event and scoped by channel/thread.
- New wake turns hydrate `TodoStore` from the latest matching durable todo
  event before falling back to synthetic tool-message history.
- Compression session splits snapshot the current todo store into the active
  child namespace, preserving todos across compression even when the current
  turn did not execute a new `todo` tool.

Hermes evidence:

- `tools/todo_tool.py`, `run_agent.py`,
  `agent/conversation_compression.py`, `agent/context_compressor.py`,
  `tests/tools/test_todo_tool.py`, and
  `tests/run_agent/test_compression_boundary.py`.

Zaion changed file:

- `crates/zaion-cli/src/commands/process/wake.rs`

Verification:

- `cargo test -p zaion-cli wake_todo -- --nocapture`: 5 passed.
- `cargo test -p zaion-cli wake_tool_context_batch_enforces_aggregate_turn_budget_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-runtime compression_split_reinjects_active_todos_before_child_branch -- --nocapture`: passed.
- `cargo fmt -p zaion-cli -p zaion-runtime`: passed.
- `cargo check -p zaion-cli`: passed with existing dead-code warnings.
- `cargo check -p zaion-runtime`: passed.

Label update:

- Durable wake todo-state event and hydration: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Gateway/channel runtimes still need the same durable todo hydration contract.

## 2026-05-23 Wake Aggregate Tool Budget and Todo-Aware Compression Split [PARTIAL SLICES]

This checkpoint records two live-runtime Hermes-alignment slices. Overall
latest-Hermes parity remains `PARTIAL`.

Implemented and verified slices:

- Wake now enforces the aggregate tool-result turn budget after a batch of
  native/MCP/todo tool calls returns and before those tool results re-enter the
  provider context. Individual tool results still get per-result spill first;
  the batch pass converts the live `ToolExecutionRecord` set into
  `ToolResultMessage` values, calls `zaion_runtime::enforce_turn_budget`, and
  writes the budgeted content back to the model-visible tool-result messages.
- `CompressionSplitter` now exposes
  `compress_and_split_with_todo_reinjection(...)`, reusing the existing
  compression session split path while calling
  `ContextCompressor::compress_with_todo_reinjection(...)`.
- `wake` now uses the todo-aware compression split path, so an in-memory
  session todo store available during the current turn is protected in the
  compressed child history.

Hermes evidence:

- Tool results: `tools/tool_result_storage.py`,
  `tools/tool_output_limits.py`, `agent/tool_executor.py`, `run_agent.py`,
  `tests/tools/test_tool_result_storage.py`, and
  `tests/tools/test_tool_output_limits.py`.
- Todo/compression: `tools/todo_tool.py`, `toolsets.py`,
  `agent/conversation_compression.py`, `agent/context_compressor.py`,
  `tests/tools/test_todo_tool.py`, `tests/agent/test_context_compressor.py`,
  `tests/run_agent/test_compression_boundary.py`, and
  `tests/run_agent/test_compression_persistence.py`.

Zaion changed files:

- `crates/zaion-cli/src/commands/process/wake.rs`
- `crates/zaion-runtime/src/compression_split.rs`

Verification:

- `cargo test -p zaion-cli wake_tool_context_batch_enforces_aggregate_turn_budget_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_tool_context_output_spills_large_results_before_model_reentry -- --nocapture`: passed.
- `cargo test -p zaion-cli wake_todo -- --nocapture`: 5 passed after the
  durable todo-state slice.
- `cargo test -p zaion-runtime compression_split_reinjects_active_todos_before_child_branch -- --nocapture`: passed.

Label update:

- Wake live aggregate tool-result budget before model re-entry:
  `PARTIAL SLICE`.
- Todo-aware compression split for current-turn active todo state:
  `PARTIAL SLICE`; durable wake todo state is now covered by signed events.
- Overall latest-Hermes comparison: still `PARTIAL`.

Still open:

- Tool-result storage now has an active-environment target abstraction, and
  local live wake writes oversized spills under `cwd/.zaion/tool-results`;
  non-local sandbox, service-cwd, gateway, MCP, and delegated execution paths
  still need real environment target selection.
- Cross-turn todo hydration is now present for wake through signed
  `zaion.session_todo.state.v1` events and queryable thread lookup, but
  gateway/channel hydration remains open; later wake hardening covers durable
  redaction/size caps.
- Compression session persistence/session split remains weaker than Hermes
  around DB flush cursor reset, old-session end reason coverage, and broader
  persistence tests.

## 2026-05-23 ACP Sink, MCP list_changed, Telegram Mention Gate, TUI Close/Resume [PARTIAL SLICES]

This checkpoint records the latest Hermes-alignment slices plus the review
hardening applied immediately after them. Overall latest-Hermes parity remains
`PARTIAL`.

Implemented and verified slices:

- ACP stdio protocol events now have a sink abstraction:
  `AcpProtocolEventSink`, `AcpStdioProtocolEventSink<W: Write>`, and
  `AcpProtocolEventCollector`. `write_protocol_event` routes through the sink,
  and tests cover `text.delta` and `tool.progress` as newline-delimited
  JSON-RPC `protocol/event` notifications with no `id`.
- ACP session lifecycle calls now enforce principal ownership consistently:
  `new_session`, `load_session`, `resume_session`, and `fork_session` reject
  unsafe or cross-principal submitters instead of loading/resuming only by
  `session_id`.
- MCP `refresh_server_tools(server_name)` now preserves the previous server
  tools when rediscovery fails, and only replaces that server's tools after a
  successful refresh. Unknown servers still fail with
  `MCP server '<name>' not found`.
- Telegram group dispatch now requires an explicit bot mention, wake trigger,
  or `/cmd@zaion_bot` target. Bare group slash commands and commands for other
  bots are treated as group noise after access policy is checked.
- Telegram busy guard cleanup now releases the active thread if canonical
  envelope construction fails after `begin_or_hold`, preventing a local
  per-thread deadlock.
- TUI `/gateway-close` now sends `session.close` for an active gateway session
  and detaches local gateway transport state so later prompts do not queue
  forever as "gateway session pending". No-session and usage-error paths remain
  control/status only.

Hermes evidence:

- ACP: `acp_adapter/events.py`, `acp_adapter/server.py`,
  `acp_adapter/session.py`, `tests/acp/test_events.py`.
- MCP: `tools/mcp_tool.py`, `tests/tools/test_mcp_tool.py`.
- Telegram: `gateway/platforms/telegram.py`,
  `tests/gateway/test_telegram_group_gating.py`,
  `tests/gateway/test_telegram_mention_boundaries.py`,
  `tests/gateway/test_telegram_noise_filter.py`.
- TUI lifecycle: `ui-tui/src/app/useSessionLifecycle.ts`,
  `tui_gateway/server.py`.

Zaion changed files:

- `crates/zaion-a2a/src/stdio_service.rs`
- `crates/zaion-runtime/src/mcp_tools.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-cli gateway_close -- --nocapture`: 5 passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 23 matching tests passed
  across unit and integration filters.
- `cargo test -p zaion-runtime mcp -- --nocapture`: 26 passed.
- `cargo test -p zaion-a2a acp -- --nocapture`: 11 passed, 0 failed, 14
  filtered out.

Label update:

- ACP event sink and session-owner hardening: `PARTIAL SLICE`.
- MCP `list_changed` refresh hook and failure-preserving replacement:
  `PARTIAL SLICE`.
- Telegram mention/noise gate and busy-guard cleanup: `PARTIAL SLICE`.
- TUI gateway close lifecycle hardening: `PARTIAL SLICE`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Next high-ROI non-hot slices identified by read-only exploration: tool-result
spill-to-file budgeting, session todo tool with compression reinjection, and
context-compression active-task safety.

## 2026-05-23 Gateway Approval/Clarify, Telegram Topic Routing, ACP Events, Dynamic MCP Toolsets [PARTIAL SLICES]

This checkpoint records the latest verified Hermes-alignment slices without
promoting the whole latest-Hermes comparison. Overall latest-Hermes parity
remains `PARTIAL`.

Implemented and verified slices:

- TUI gateway approval/clarify response controls: `/approve [once|session|always|all]`,
  `/deny [all]`, and `/clarify <answer>` now answer pending gateway
  `approval.request` / `clarify.request` frames through stdio JSON-RPC
  `approval.respond` and `clarify.respond` instead of starting a local wake
  turn. Empty `/clarify` sends an empty answer for cancel semantics.
- Telegram live-loop busy guard groundwork: ordinary messages for an active
  Telegram thread are held in one replaceable pending slot, while separate
  threads remain independent and slash commands still bypass the guard.
- Telegram adapter chunking now measures Telegram's 4096 limit in UTF-16 code
  units, preserving emoji and non-BMP correctness.
- Telegram outbound send bodies now preserve topic/reply metadata: metadata
  `thread_id` or `message_thread_id` maps to Telegram `message_thread_id`,
  General topic `"1"` is omitted, metadata `telegram_reply_to_message_id` can
  supply the reply anchor, and chunked sends keep topic routing while replying
  only from the first chunk.
- ACP protocol event DTOs, capability advertisement, and stdio
  `protocol/event` JSON-RPC notification helpers are present for
  `tool.progress`, `permission.request`, `permission.result`,
  `thinking.delta`, and `text.delta` under `zaion.acp.event.v1`.
- MCP dynamic toolset reporting now exposes configured/discovered servers as
  Hermes-style `mcp-<server>` toolsets with raw server aliases in
  `zaion tools list`, `zaion tools summary`, and
  `zaion capability show --json`.

Hermes evidence:

- TUI gateway approval/clarify: `ui-tui/src/gatewayTypes.ts`,
  `ui-tui/src/app/createGatewayEventHandler.ts`, `tui_gateway/server.py`.
- Telegram channel behavior: `gateway/platforms/telegram.py`,
  `gateway/platforms/base.py`, `gateway/run.py`.
- ACP event model: `acp_adapter/events.py`, `acp_adapter/server.py`,
  `tests/acp/test_events.py`.
- MCP dynamic toolsets: `tools/mcp_tool.py`, `toolsets.py`,
  `tools/registry.py`, `tests/tools/test_mcp_tool.py`,
  `tests/acp/test_session.py`, `tests/acp/test_server.py`.

Zaion changed files:

- `crates/zaion-cli/src/commands/process/tui/app.rs`
- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-adapters/src/telegram_adapter.rs`
- `crates/zaion-a2a/src/acp.rs`
- `crates/zaion-a2a/src/stdio_service.rs`
- `crates/zaion-runtime/src/mcp_tools.rs`
- `crates/zaion-cli/src/commands/tool.rs`
- `crates/zaion-cli/src/commands/capability.rs`

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

Label update:

- TUI gateway approval/clarify response controls: `PARTIAL SLICE`.
- Telegram UTF-16 chunking, topic/reply metadata routing, and live busy guard
  groundwork: `PARTIAL SLICE`.
- ACP protocol event DTO advertisement and stdio notification helper:
  `PARTIAL SLICE`.
- Dynamic MCP `mcp-<server>` toolset reporting and raw alias resolution:
  `PARTIAL SLICE`.
- Overall TUI runtime parity: still `PARTIAL`.
- Overall Telegram/live channel parity: still `PARTIAL`.
- Overall ACP/MCP/tool parity: still `PARTIAL`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Next unresolved gap: ACP live runtime event egress, TUI session lifecycle
depth, WebSocket attach, Telegram mention/allowlist/media/reaction/retry
behavior, and MCP sampling / `list_changed` refresh.

## 2026-05-23 TUI Gateway Stdio JSON-RPC Transport [PARTIAL SLICE]

This slice attaches Zaion's terminal TUI gateway reducer to a Hermes-style
stdio JSON-RPC transport. `zaion tui` and the default neural TUI path now accept
`--gateway-stdio <program>` plus repeated `--gateway-arg <arg>` as structured
argv, spawn the configured process with piped stdin/stdout, send initial
`session.create`, record `result.session_id`, and route ready-session prompts
through `prompt.submit`. Busy steer and interrupt modes now use
`session.steer` and `session.interrupt` when the gateway session is ready.

Hermes evidence: `ui-tui/src/gatewayClient.ts`,
`ui-tui/src/app/useSessionLifecycle.ts`, `ui-tui/src/app/useSubmission.ts`,
`ui-tui/src/app/turnController.ts`, `tui_gateway/entry.py`, and
`tui_gateway/server.py`.

Zaion changed files:

- `crates/zaion-cli/src/commands/process/tui/mod.rs`
- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-cli gateway_transport_without_session_queues_prompt_instead_of_falling_back_to_local_wake -- --nocapture`: 1 passed, 0 failed.
- `cargo test -p zaion-cli gateway -- --nocapture`: 28 passed, 0 failed in the unit filter, plus matching filtered integration/stable tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 46 passed, 0 failed.
- `cargo test -p zaion-cli busy_ -- --nocapture`: 7 passed, 0 failed.
- `cargo test -p zaion-cli queue -- --nocapture`: 16 unit tests plus 3 matching filtered integration/slash tests passed, 0 failed.

Label update:

- Local TUI stdio JSON-RPC transport: `PARTIAL` slice.
- Overall TUI runtime parity: still `PARTIAL`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Next unresolved gap: gateway-backed `approval.respond` and `clarify.respond`,
then subagent controls, protocol recovery, session lifecycle depth, WebSocket
attach parity, and streaming finalization.

## 2026-05-23 TUI Gateway Event Frame Ingress [PARTIAL SLICE]

This slice starts the Hermes TUI gateway/event protocol mainline. Zaion's
terminal TUI now has a local Hermes-style event-frame reducer plus a
`/gateway-event <json>` helper for dogfooding. The reducer handles
`gateway.ready`, `gateway.protocol_error`, `approval.request`,
`clarify.request`, `subagent.*`, `message.delta`, and `message.complete`
without treating protocol frames as user turns or starting model prompts.

Hermes evidence: `ui-tui/src/gatewayTypes.ts`,
`ui-tui/src/gatewayClient.ts`, `ui-tui/src/app/createGatewayEventHandler.ts`,
`tui_gateway/entry.py`, `tui_gateway/server.py`, and `tui_gateway/ws.py`.

Zaion changed file:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-cli gateway_event -- --nocapture`: 2 passed, 0 failed.

Label update:

- Local TUI gateway event reducer: `PARTIAL` slice.
- Overall TUI runtime parity: still `PARTIAL`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Next unresolved gap: attach a real JSON-RPC/WebSocket/stdio gateway transport,
then wire live `session.steer`, `session.interrupt`, `approval.respond`,
`clarify.respond`, subagent controls, protocol recovery, and finalization.

## 2026-05-23 TUI Steer/Interrupt Busy Controls [PARTIAL SLICE]

This slice extends the Hermes TUI runtime work beyond queue-only behavior.
Zaion's terminal TUI now has local busy input modes for `queue`, `steer`, and
`interrupt`; `/busy` changes or reports the mode; `/steer <prompt>` records a
control injection when a turn is active and falls back to the next-turn queue
when no turn is active; `/busy interrupt` requests cancellation and places the
replacement prompt at the front of the queue.

Hermes evidence: `ui-tui/src/app/useSubmission.ts`,
`ui-tui/src/app/turnController.ts`,
`ui-tui/src/app/slash/commands/core.ts`,
`ui-tui/src/app/slash/commands/session.ts`, and `tui_gateway/server.py`.

Zaion changed file:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-cli busy_steer_mode_routes_busy_input_to_control_channel_not_fifo -- --nocapture`: passed.
- `cargo test -p zaion-cli slash_steer_without_active_turn_falls_back_to_next_turn_queue -- --nocapture`: passed.
- `cargo test -p zaion-cli busy_interrupt_mode_cancels_active_turn_and_queues_replacement_front -- --nocapture`: passed.
- `cargo test -p zaion-cli busy_ -- --nocapture`: 6 busy-filtered unit tests passed.
- `cargo test -p zaion-cli queue -- --nocapture`: 13 queue-filtered unit tests passed, plus matching filtered integration tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 34 TUI-filtered unit tests passed.

Label update:

- Local TUI steer/interrupt controls: `PARTIAL` slice.
- Overall TUI runtime parity: still `PARTIAL`.
- Overall latest-Hermes comparison: still `PARTIAL`.

Next unresolved gap: gateway/event protocol parity for these controls, then
approval/clarify/subagent/protocol-error/finalization parity.

## 2026-05-23 Latest Hermes Report Expansion [PARTIAL]

`docs/zaion_vs_hermes.md` has been expanded from a short evidence note into the
current latest-source recalibration report and acceptance contract. It now
covers the source-cited Hermes architecture map, config-complete-to-first-start
sequence, workspace/session/profile model, CLI/TUI/gateway/tool/memory
collaboration model, and a detailed Zaion vs latest Hermes comparison table.

This update does not promote the whole comparison. Overall latest-Hermes parity
remains `PARTIAL`. The next mainline is still TUI runtime parity beyond the
local queue UX, then live Telegram/channel parity, then
tool/MCP/ACP/profile/session/context parity.

## 2026-05-23 TUI Queue Edit/Dequeue UX [PARTIAL SLICE]

This slice extends the Hermes queue-mode work with local terminal TUI
queued-prompt editing. Zaion now shows a queued prompt preview window and lets
the operator select, edit, replace, delete, or cancel queued prompts without
interrupting the active streaming turn.

Hermes evidence: `ui-tui/src/hooks/useQueue.ts`,
`ui-tui/src/components/queuedMessages.tsx`,
`ui-tui/src/app/useInputHandlers.ts`, `ui-tui/src/app/useSubmission.ts`, and
`ui-tui/src/app/useMainApp.ts`.

Zaion changed file:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-cli queue -- --nocapture`: 11 queue-filtered unit tests
  passed, plus matching filtered integration tests passed.
- `cargo test -p zaion-cli tui -- --nocapture`: 31 TUI-filtered unit tests
  passed.

Label update:

- Local TUI queue edit/delete UX: `PARTIAL` slice.
- Overall TUI runtime parity: still `PARTIAL`.

Next unresolved gap: TUI runtime parity beyond local queue UX: JSON-RPC/event
gateway, steer/interrupt, approvals, clarify, subagent events, protocol
errors, streaming finalization, and broader tests.

## 2026-05-23 Source Gate Reconciliation [SURPASSED]

This checkpoint preserves the current architecture truth anchors required by
`zaion doctor` before the Telegram/TUI parity work continues:

- Phase 8-B Source Truth Reconciliation [SURPASSED]
- Unified Runtime Execution Metrics [SURPASSED]
- BatchRunner Worker Pool Execution [SURPASSED]
- Runtime BatchRunner Execution Chain [SURPASSED]
- Full Architecture Truth Alignment [SURPASSED]
- Stable Runtime Proof Matrix [SURPASSED]
- Operation Stream Source Truth Reconciliation [SURPASSED]

OPD/evolve remains chain-gated for production promotion: it is promotable only
when the append-only Ed25519 chain verifies a latest `ConfirmedStable` record.
Promotion anchor: only when the append-only Ed25519 chain verifies a latest `ConfirmedStable` record.
The next mainline is not old Phase 1 command catch-up; it is latest-Hermes TUI
runtime parity, live Telegram/channel parity, and tool/MCP/ACP/session parity.

## 2026-05-23 TUI/TG Visible Reply Lifecycle Isolation [SURPASSED SLICE]

This stage completes the concrete visible-reply isolation slice for the current
TUI/TG mainline. Zaion no longer lets lifecycle-only operation events become
chat reply text. Internal events such as `provider calling` and
`turn completed` remain observability/tracing material, while visible assistant
text and explicit tool/risk events remain eligible for chat surfaces.

Hermes baseline remains latest main
`9c0807070388c4f612a827230f1314ebbf24e857` (`2026-05-24 15:57:26 -0700`,
`test(cli): update resume usage-hint assertion for numbered selection`). This slice is
compared against Hermes' separation of TUI gateway events, channel delivery,
and user-visible chat text in `tui_gateway/*`, `ui-tui/src/*`,
`gateway/run.py`, `gateway/platforms/base.py`, and
`gateway/platforms/telegram.py`.

Changed files:

- `crates/zaion-cli/src/commands/panel_render.rs`
- `crates/zaion-runtime/src/panel_sink.rs`

Verification:

- `cargo test -p zaion-cli panel_render -- --nocapture`: 4 passed, 0 failed.
- `cargo test -p zaion-runtime panel_sink -- --nocapture`: 2 passed, 0 failed.
- `cargo test -p zaion-cli lifecycle_operation_events_do_not_render_as_chat_messages -- --nocapture`: passed.
- `cargo test -p zaion-cli completed_turn_without_visible_token_shows_explicit_tui_error -- --nocapture`: passed.
- `cargo test -p zaion-cli streaming_callback_forwards_final_text_when_provider_did_not_emit_token_deltas -- --nocapture`: passed.
- `cargo test -p zaion-cli telegram -- --nocapture`: 12 passed, 0 failed after
  the `telegram_channel_commands_share_one_effective_token_source`
  doctor/source-gate blocker was reconciled.
- `cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents -- --nocapture`: passed.
- `cargo test -p zaion-cli global_event_stream_replays_shared_operation_backlog_after_operation_cursor -- --nocapture`: passed.

Label update:

- Visible reply lifecycle isolation: `SURPASSED` for this slice.
- Overall TUI runtime maturity: still `PARTIAL`.
- Overall Telegram/live channel parity: still `PARTIAL`.

Next mainline: continue Hermes-grade TUI runtime parity, then live
Telegram/channel parity beyond local simulation.

## 2026-05-23 TUI Busy Input Queue Drain [PARTIAL SLICE]

This slice lands the minimum Hermes queue-mode behavior for Zaion's terminal
TUI. While a model turn is streaming, ordinary user input now enters a local
FIFO queue instead of replacing the active stream or starting a second assistant
placeholder. Local audit slash commands such as `/status` remain immediate.
When the active turn settles, Zaion drains exactly one queued prompt and starts
it as the next user turn.

Hermes evidence: `ui-tui/src/app/useConfigSync.ts`,
`ui-tui/src/hooks/useQueue.ts`, `ui-tui/src/app/useSubmission.ts`,
`ui-tui/src/app/useMainApp.ts`, and `tui_gateway/server.py`.

Zaion changed file:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Verification:

- `cargo test -p zaion-cli busy_ -- --nocapture`: 4 passed, 0 failed.
- `cargo test -p zaion-cli queue -- --nocapture`: 9 passed, 0 failed across
  matching unit/integration filters.
- `cargo test -p zaion-cli tui -- --nocapture`: 26 passed, 0 failed.
- `cargo test -p zaion-cli completed_turn_dequeues_next_prompt_and_starts_it_once -- --nocapture`: passed.
- `cargo test -p zaion-cli queued_busy_input_is_transcripted_once_when_drained -- --nocapture`: passed.
- `cargo test -p zaion-cli busy_audit_command_keeps_streaming_placeholder_connected_to_tokens -- --nocapture`: passed.

Label update:

- TUI busy input queue drain: `PARTIAL` slice.
- Overall TUI runtime parity: still `PARTIAL`.

Next unresolved gap: TUI runtime parity beyond the queue minimum: event gateway
protocol, steer/interrupt, gateway-backed queue controls, approvals, clarify,
subagent events, protocol errors, and finalization.

## 2026-05-23 Latest Hermes Source Revalidation [PARTIAL]

This is the current mainline calibration block. It supersedes older notes that
were written against Hermes `2026.4.8`, while keeping those notes as historical
stage evidence. The latest-source comparison is now `PARTIAL`, not `OPEN` and
not `SURPASSED`: Zaion has real product progress, but latest Hermes still leads
in several runtime/product layers.

Reference evidence:

- Latest Hermes mirror: `D:/zaion-reference/hermes-agent-latest`.
- Hermes upstream: `https://github.com/NousResearch/hermes-agent.git`.
- Remote `origin/main`, local `origin/main`, and local `HEAD` all resolve to
  `9c0807070388c4f612a827230f1314ebbf24e857`.
- Latest mirror commit: `2026-05-24 15:57:26 -0700`, `test(cli): update resume usage-hint assertion for numbered selection`.
- Historical zip `D:/zaion-reference/zaion-rust-cleanup-20260501/hermes-agent-2026.4.8.zip` was listed again and remains only the historical
  baseline.
- Latest Hermes does not have top-level `environments/*`; latest-main
  environment/runtime evidence is `tools/environments/*`, `batch_runner.py`,
  `trajectory_compressor.py`, and current docs/tests.

Hermes source areas covered in this revalidation:

- TUI/runtime bridge: `tui_gateway/server.py`, `tui_gateway/ws.py`,
  `tui_gateway/transport.py`, `ui-tui/src/gatewayClient.ts`,
  `ui-tui/src/app/useSubmission.ts`,
  `ui-tui/src/app/createGatewayEventHandler.ts`,
  `ui-tui/src/components/appLayout.tsx`, `ui-tui/src/__tests__/*`.
- Gateway/channel runtime: `gateway/config.py`, `gateway/session.py`,
  `gateway/run.py`, `gateway/platforms/base.py`,
  `gateway/platforms/telegram.py`, `website/docs/user-guide/messaging/*`.
- Memory/context/session: `agent/memory_manager.py`, `agent/prompt_builder.py`,
  `hermes_state.py`, `website/docs/developer-guide/prompt-assembly.md`,
  `website/docs/developer-guide/context-compression-and-caching.md`.
- ACP/MCP/tools: `acp_adapter/server.py`, `acp_adapter/session.py`,
  `website/docs/developer-guide/acp-internals.md`, `mcp_serve.py`,
  `hermes_cli/mcp_config.py`, `website/docs/user-guide/features/mcp.md`,
  `tools/registry.py`, `toolsets.py`, `toolset_distributions.py`.
- Batch/trajectory/environment: `batch_runner.py`, `trajectory_compressor.py`,
  `tools/environments/*`.

Current Zaion vs latest Hermes labels:

| Area | Label | Current judgment |
| --- | --- | --- |
| Product entry contract | `SURPASSED` | `zaion`, `zaion dashboard`, `zaion start`, and `zaion gateway start` now have clear roles, and `zaion launch-check` verifies them. |
| Neural observability direction | `SURPASSED` | Zaion's signed ledger, provenance, evidence/risk packets, truth labels, and neural topology direction remain a differentiated product lead. |
| TUI runtime maturity | `PARTIAL` | Zaion has chat-first TUI, right context rail, `Ctrl+L`, slash suggestions, overlays, evidence/risk/topology state, visible-reply isolation, busy-input FIFO queue drain, local queue edit/delete UX, and tests; Hermes still leads on React/Ink depth, JSON-RPC gateway, deferred session build, steer/interrupt, approval/subagent/protocol-error handling, and test surface. |
| Telegram/live channels | `PARTIAL` | Zaion fixed final-content fallback and `tg simulate` visible replies; Hermes still leads on live Telegram batching, MarkdownV2, mention/allowlist gates, media, reactions, topic/reply fallback, and delivery ergonomics. |
| Tools/MCP/ACP | `PARTIAL` | Zaion has 8 native MCP built-ins and proof-aware diagnostics; Hermes still leads on tool breadth, toolsets, MCP client/server depth, dynamic discovery, sampling, approval, tool-result storage, and ACP lifecycle. |
| Profile/session/context/memory | `PARTIAL` | Zaion has signed runtime/provenance advantages; Hermes still leads on profile workspace, prompt assembly, memory provider lifecycle, and compression hygiene. |
| OPD/evolution/batch | `PARTIAL` | Zaion has chain-gated OPD/evolve and signed promotion concepts; Hermes latest retains strong batch/trajectory/compression tooling. |

Next mainline:

1. Implement and verify Hermes-grade TUI runtime parity beyond the local queue
   minimum: event gateway protocol, steer/interrupt, approval/clarify/subagent
   events, protocol errors, streaming finalization, and deeper terminal tests.
2. Implement and verify live Telegram parity beyond `tg simulate`: mention
   gates, allowlists, batching, media cache, MarkdownV2/splitting, reactions,
   topic/reply fallback, and visible final replies in a real channel loop.
3. Expand callable tools plus MCP/ACP/profile parity: dynamic MCP discovery,
   runtime toolsets, sampling guardrails, ACP load/resume/fork, permission
   bridge, provenance-bound tool-result storage, profile-scoped workspace,
   prompt/memory/compression hygiene.
4. After each stage, update `MASTER_PLAN.md`,
   `plans/openclaw_latest_gap_report.md`,
   `plans/hermes_surpass_master_plan.md`, and `docs/AGENTS.md` before
   reporting the stage as complete.

## Phase 10 ??全量回归、对标证明、超越包

- parity test suite（对??OpenClaw 每个命令族的行为测试??- performance benchmark（启动时间、内存占用、吞吐量??- long-run stability test??2h bot loop 无崩溃）
- 全量 review（代码、配置、测试、文档、UX??- 发布 "Zaion vs OpenClaw" 对标报告
- 超越清单：列??Zaion 独有能力（治理面、审计面、ClawhHub 原生、Rust 性能??

## 5. ClawhHub 集成规范

ClawhHub ??OpenClaw 的技能市场。Zaion 原生支持意味着??
1. **skill manifest 格式兼容**：`clawhub.toml` ??OpenClaw ??skill manifest 结构对齐
2. **API 对接**：`zaion hub` 命令直接调用 ClawhHub API（search、install、publish??3. **运行时桥??*：ClawhHub JS/TS skill 通过 Node.js runtime bridge 运行；原??Rust skill 直接加载
4. **双向发布**：Zaion 自己的技能可发布??ClawhHub，供 OpenClaw 用户使用

## 6. 超越 OpenClaw 的差异化方向

| 维度 | OpenClaw | Zaion 目标 |
|------|----------|------------|
| 语言 | TypeScript/Node.js | Rust（更低内存、更快启动） |
| 审计??| 基础 audit log | 完整 receipt + replay + approval gate |
| 记忆治理 | RAG + markdown | 四层记忆 + 治理策略 + compaction |
| 技能市??| ClawhHub（JS??| ClawhHub 原生 + Rust native skills |
| 移动??| 有限支持 | Termux 一等公??|
| principal_id | ??| Ed25519 身份，所有事件签??|
| 自愈 | 基础 doctor | doctor + reflector + skill distiller |

## 7. Current Execution Order

The current long-horizon entry remains `plans/hermes_surpass_master_plan.md`,
and the current fact source remains `plans/openclaw_latest_gap_report.md`.
Latest Hermes `main` is now the comparison baseline; historical Hermes
`2026.4.8` notes are archive evidence only.

Current execution order:

1. Read `plans/openclaw_latest_gap_report.md` first and confirm the current
   Hermes comparison label.
2. Read `plans/hermes_surpass_master_plan.md` next and follow the long-term
   phase order without treating old `2026.4.8` conclusions as latest-main facts.
3. Current highest-priority implementation mainline: TUI runtime parity beyond
   the local queue minimum, including event gateway protocol, steer/interrupt,
   approval/clarify/subagent events, protocol errors, streaming finalization,
   and terminal tests.
4. After TUI runtime parity, move to live Telegram/channel parity: real
   Telegram reply loop, MarkdownV2/splitting, mention/allowlist, batching,
   media cache, reactions, topic/reply fallback.
5. Then move to tools/MCP/ACP/profile/session/context parity: dynamic MCP
   discovery, runtime toolsets, sampling, ACP session load/resume/fork,
   permission bridge, profile-scoped workspace, prompt/memory/compression
   hygiene.
6. Do not re-open already completed entry-contract work as the next mainline:
   product launcher/dashboard/start/gateway relationship, chat-first TUI
   concept, TG final-content fallback, `tg simulate` visible reply path, and
   the current 8 native MCP built-ins are bases for the next parity stages.

## 8. 计划维护规则

- 本文件当前只作为导航页与历史归档入口，不再单独承担“真实状态源”职�??- Hermes / OpenClaw 当前真实状态一律以 `plans/openclaw_latest_gap_report.md` 为准??- Hermes 长期阶段规划与执行闭环以 `plans/hermes_surpass_master_plan.md` 为准??- 若旧段落中的历史计划、里程碑或“已完成”描述与 gap ledger 冲突，一律视为历史归档，不构成当前事实判�??- 新实现落地后，先更新 `plans/openclaw_latest_gap_report.md`，再回写本文�??- 本文件路径：`D:/zaion-rust/MASTER_PLAN.md`
- ??Python 版主计划（`D:/zaion/omni-agent/plans/zaion_master_plan.md`）并行，互不覆盖??
### 8.1 治理事件镜像

- **[REVIEW-REPORT-REMEDIATION-COMPLETE] 2026-04-19** ??`review报告/CODE_REVIEW_REPORT.md` 全量 HIGH + MEDIUM 缺陷收口?? CRITICAL ??2026-04-18 批次完成。本次扫尾批次覆??HIGH（H1, H3–H11, H12, H13, H14, H19, H23–H36）与 MEDIUM（M1–M29）全量。`cargo check --workspace` 零错误、零警告??0 ??crate 测试套件零失败；`cargo build --workspace` 96 ??0 warnings。关键结论：
  - **安全??(H1 H3 H4 H5 H6 H7 H8 H9 H10)**：TOCTOU / 0o600 / client_secret body / refresh token / IpAddr SSRF / shell 沙箱 / relay auth / rollback sanitize / config key 全部修复，详情见 commits 16d25d4 / b436274 / c4a9cae / e9013d3 / 5b841d7??  - **并发??(H19 H26 H27 H28)**：EventLedger 二次 ensure、spawn_blocking WASM、共??reqwest client、EncryptedStore Send+Sync ??全部修复，详情见 commits 16d25d4 / 924d45c / c4a9cae / b436274??  - **测试??(H33 H34 H35)**：AciLedger 实现 + test（commit 7e256ec）、zaion-gitledger 0??6 tests（commit c5ddc67）、zaion-codex codegen+diff 0??5 tests（commit 353d693�??  - **代码质量??(H11 H12 H13 H14)**??    - H11 ??`zaion-adapters/src/provider.rs` 1440 LoC 拆分??6 子模块（openai / anthropic / ollama / deepseek / embedding / mod），每文??< 520 LoC，commit 1cc45d1??    - H12 ??`zaion-cli/src/commands/network.rs` 1222 LoC 拆分??7 子模块（daemon / telegram / gateway / routes / console / agent / pair），commit 3ae0d2e??    - H13 ??`zaion-cli/src/commands/process.rs` 882 LoC 拆分??6 子模块（helpers / lifecycle / chat / wake / bot / mod），commit 127ccdc??    - H14 ??消除 `zaion-core ??zaion-runtime` 反向层依赖；删除孤儿 `CoreError::Runtime` ??`ProcessController::wake()→AgentLoop` 构造路径。`zaion-core/Cargo.toml` 现在严格遵守层序 `types ??crypto ??ledger ??secrets ??core`，commit bd45398??  - **警告清理 (M1–M29)**：`cargo fix --workspace --allow-dirty --tests` 批量修复 66 ??auto-suggestions（陈??imports、mut 移除、`_` 命名），另外??18 个合法“保留未来使用”的脚手架文件添??`#![allow(dead_code)]`（含 runtime_integration / zk_compression / execute_code{,_uds,_js} / slash_integration / import_openclaw / evolve / webhook::dispatch_event_webhooks 等）。`cargo build --workspace`??6 ??0 warnings，commit 1a97051??  - **全量 verify**：`cargo check --workspace` 11.31s clean；`cargo test --workspace --exclude zaion-cli` 40/40 test suite 全绿，共 ~1,200 tests 无失败。`zaion-cli` 单线??webhook 套件 16/16 pass；`--test-threads=4` 91/91 pass；两条原??flake（`embedding_api_status_distinguishes_untested_keys` ??`summarize_webhook_test_response_extracts_metadata_and_body_preview`）在 H12/H13 之前已存在，与本批次无关，属 HTTP 网络依赖 flake??  - **残留**：原 review 报告 CRITICAL ??8 项已??2026-04-18 批次清零；HIGH 36 项全部消化；MEDIUM 29 项并??M1–M29 警告清理批次完成。无新发现的 P0/P1??
- **[LEDGER-CALIBRATED-2026-04-18] [P0-CRITICAL-FIXED] 2026-04-18** ??P0 编译修复?? errors ??0?? 8 CRITICAL 安全缺陷全部修复 + 账本真值校准。详情见 `plans/openclaw_latest_gap_report.md` §7 ??`plans/fix_p0_critical_and_ledger_20260418.md`。关键结论：
  - `cargo check --workspace` 零错误绿灯。`cargo test` 7 crates 418 passed / 0 failed??  - CRITICAL #1/#2: shell injection（ShadowTask + execute_terminal）→ `CommandSpec` + allow-list，不??`sh -c`??  - CRITICAL #3: `cargo check` 触发恶意 `build.rs` ??替换??`cargo metadata --no-deps --offline`??  - CRITICAL #4/#5: ??Ed25519 占位????真实 ed25519-dalek v2 签名（`McpProvenance` + `TurnSignature`�??  - CRITICAL #6: 路径遍历防护??`let _ =` 禁用 ??真实 `starts_with` + Windows 卷号检�??  - CRITICAL #7: master key ??zeroize ??`Zeroizing<[u8; 32]>`??  - CRITICAL #8: API key 明文序列????`ApiKeySource` enum + `SecretString`，磁盘仅存别�??  - 账本校准：Phase 1/1.5/1.7/1.8 ??SURPASSED 条目原基于占位符声称达标，现已通过真实密码学实现合法获??SURPASSED 地位。HIGH #15/#16 标记??RESOLVED（已??P2 前独立修复）??  - 已知残留：全局插件 `everything-claude-code` 仍拦??`plans/**.md` 写入（C-0，跨仓不改）；`~/.claude/settings.json` 内明??`ANTHROPIC_AUTH_TOKEN` 建议轮换??
- **[HOOKS-HARDENED] 2026-04-17** ??`.claude/hooks/` 全面硬化完成。详情见 `plans/openclaw_latest_gap_report.md` §7 ??`plans/fix_claude_hooks_20260417.md`。关键结论：
  - PreToolUse 现在真实覆盖 Bash / Write / Edit / NotebookEdit / `mcp__Filesystem__{write_file,edit_file,move_file,create_directory}`；路径做小写盘符 + 正斜杠归一化后比对，`D:/zaion-rust/**` 放行、`D:/zaion/zaion/**` ??`D:/zaion/omni-agent/**` 拦截??  - 危险命令黑名单在 strip heredoc body + 首条逻辑行上匹配，消??文档里提??`rm -rf` 就被误拦"的旧假阳�??  - `inject-context.sh` 改为??`$CLAUDE_SESSION_ID` 每会话只注入一次，终结??prompt 上下文污�??  - `stop-verify.sh` 移除不可??echo 安慰�??  - 自测 33/33 PASS，灰盒矩??4/4 PASS，`.claude/hooks/trace.log` 留痕可审�??  - 已知 residual：全局插件 `everything-claude-code` 仍会拦截 `plans/**.md` 写入（C-0，跨仓不改）；`~/.claude/settings.json` 内明??`ANTHROPIC_AUTH_TOKEN` 建议轮换??

---

## 9. 历史归档（不作为当前事实源）

以下内容保留为历史分析与阶段性作战记录，仅供参考：
- 旧版 OpenClaw 路线与差异化设计说明
- 2026-04-11 形成??Hermes 深挖与阶段作战草??- 已被 gap ledger 纠偏或覆盖的里程??完成态描??
使用规则??- 需要判断“当前是否已完成”时，禁止直接引用下文，必须回到 `plans/openclaw_latest_gap_report.md`??- 需要判断“长期下一步怎么做”时，优先读??`plans/hermes_surpass_master_plan.md`??
---

# 历史归档：HERMES SURPASS PLAN（仅保留，不作为当前事实源）

> 以下内容??2026-04-11 历史草案归档。凡涉及“已完成”“已达成”“里程碑完成”等表述，均不得直接作为当前事实引用；当前真实状态必须回??`plans/openclaw_latest_gap_report.md` 核对??
**版本**: v1.0
**制定日期**: 2026-04-11
**目标**: 全量对标并超??Hermes Agent 2026.4.8??44 Python文件??
**原则**: 每次开发对照本文档，不得漂移，完成一项打勾一�??
---

## 战情评估（Hermes 深度解析??
### Hermes 核心能力 vs Zaion 现状

| ??| Hermes 实现 | Zaion 现状 | 差距等级 |
|----|------------|-----------|---------|
| 成本核算 | CanonicalUsage + Decimal精度 + 15+模型定价快照 | ??| P0 |
| Prompt缓存 | system_and_3策略 4断点 75%降费 | ??| P0 |
| 上下文压??| ContextCompressor结构化摘??中段剪枝+首尾保护 | ??| P0 |
| 密钥脱敏 | 35+前缀模式RedactingFormatter日志安全 | ??| P0 |
| 注入扫描 | 10+模式+隐形Unicode检??| ??| P0 |
| SessionDB | SQLite WAL+FTS5+schema v6+抖动重试 | 基础ledger | P1 |
| 工具调用解析 | 11种格??GLM/Kimi/Qwen/DeepSeek?? | OpenAI+Anthropic | P1 |
| 智能路由 | 关键??长度启发式廉价模型分??| ??| P1 |
| 检查点管理 | shadow git仓库+写前快照+回滚 | gitledger(不同场景) | P1 |
| 程序化工具调??| Python脚本→UDS RPC→工具分??| ??| P2 |
| 混合智能??MoA) | 4并行前沿模型+Claude聚合 | TrinityEngine(类似) | P2 |
| 会话FTS搜索 | FTS5跨会话召??LLM摘要 | SkillStore文本搜索 | P2 |
| @引用语法 | @file/@url/@git上下文注??| ??| P2 |
| 多平台网??| 13平台(Discord/飞书/钉钉/Signal?? | Telegram+Terminal | P2 |
| Telegram增强 | MarkdownV2+媒体批聚??话题支持 | 基础收发 | P2 |
| RL蒸馏 | AgenticOPDEnv逐token训练信号 | ??| P3 |
| OSV漏洞扫描 | tirith_security binary集成 | security scan(自研) | P3 |
| V4A补丁格式 | patch_parser V4A diff解析 | ACI AST补丁 | P3 |

### Zaion 独有超越维度（Hermes 没有??
1. Ed25519密码学身????每事件签名，防篡改账??2. Ouroboros自愈协议 ??进程崩溃自动复活，zaion-watchdog
3. ACI 2.0 AST外科接口 ??语法熔断，多语言AST级别代码修改
4. Trinity三位一体推????Architect+Developer+Tester并发角色仲裁
5. ZK-Rollup记忆折叠 ??SHA-256 commitment链，记忆压缩证明
6. Reality Sync现实锚点 ??SHA-256文件锚点+漂移检??7. TEE硬件飞地 ??zaion-enclave EnclaveIdentity+SealedSecret
8. 神经拓扑TUI ??60FPS ratatui 5面板+TopoPane实时拓扑动画
9. 自进化引??zaion-evolve) ??scanner→proposer→review→apply全管??10. W3C DID ??did:key method，Ed25519VerificationMethod JSON-LD
11. Genesis Protocol ??SkillForge+DreamEngine+Multiverse平行宇宙
12. 559 tests绿灯 ??Hermes无等价测试套??
---

## 实施路线??
### Phase A ??成本感知与安全基础（P0??
**[A1] zaion-pricing crate**

- [x] CanonicalUsage struct (input/output/cache_read/cache_write/reasoning tokens)
- [x] PricingEntry struct + PRICING_TABLE (20+模型: Anthropic/OpenAI/DeepSeek/Google/ZhipuAI/MiniMax/o3-mini)
- [x] estimate_usage_cost(usage, model) -> Option<CostResult>
- [x] normalize_usage(raw: &Value) -> CanonicalUsage (统一3种API shape)
- [x] zaion insights CLI命令 (会话成本�??
- [x] 目标: ?? tests ??(13 tests)

**[A2] Anthropic Prompt Caching**

- [x] apply_cache_control(messages, system) ??system_and_3策略??断点
- [x] 集成??AnthropicProvider::complete() ??complete_stream()
- [x] zaion wake --cache 标志??- [x] 目标: ?? tests ??(4 tests)

**[A3] ContextCompressor**
- [ ] compress(history, max_tokens) -> CompressedContext
- [ ] 结构化摘要模?? Goal/Progress/Decisions/Files/NextSteps
- [ ] 中段剪枝 + 首尾保护 + LLM摘要fallback截断
- [ ] 集成??cmd_wake / cmd_bot history加载
- [ ] 目标: ?? tests

**[A4] Secret Redaction**

- [x] SecretRedactor 35+前缀模式
- [x] Telegram token / DB connection string 检??- [x] redact(text) -> String 全文扫描替换
- [x] 集成??zaion-cli 日志输出
- [x] 目标: ?? tests ??(9 tests)

**[A5] Prompt Injection Scanner**

- [x] PromptInjectionScanner 10+检测模??(6??
- [x] 隐形Unicode检??(U+200B/200C/200D/2060/FEFF)
- [x] scan(text) -> ScanResult { clean, findings }
- [x] 集成??cmd_wake / cmd_bot 入口
- [ ] zaion security scan-input CLI子命??(待完??
- [x] 目标: ?? tests ??(10 tests)

### Phase B ??多模型支持与路由（P1??
**[B1] 工具调用解析器扩??*

- [x] ToolCallParser trait + 11种格式实??(HermesParser/DeepSeekV3/Mistral/Llama3Json/Longcat/Glm45/Glm47/KimiK2/Qwen3Coder/Qwen)
- [x] 自动格式检??try_all_parsers() + --parser CLI选项
- [x] 目标: ??1 tests ??(15 tests)

**[B2] 智能模型路由**

- [x] SmartRouter: 简单请求→廉价模型分派
- [x] 集成??cmd_wake (--smart-route)
- [x] 目标: ?? tests ??(7 tests)

**[B3] SessionDB升级**

- [x] EventLedger WAL + NORMAL synchronous + 64MB cache (已有)
- [x] FTS5虚拟??events_fts + INSERT触发器自动同??- [x] fts_search(pid, query, limit) + fts_search_global(query, limit)
- [x] zaion sessions search <pid> <query> [--limit N] CLI
- [x] 目标: ?? tests ??(10 tests in zaion-ledger)

**[B4] CheckpointManager**

- [x] shadow git仓库 ~/.zaion/checkpoints/{sha256[:16]}/
- [x] 写前自动快照 snapshot(dir, msg) + restore(dir, id)
- [x] zaion checkpoint list/snap/restore/diff CLI
- [x] 目标: ?? tests ??(5 tests)

### Phase C ??战略超越（P2??
**[C1]** @引用语法 ??@file/@url/@git/@mem上下文注??
- [x] `parse_references(text)` ??扫描 @file:/@url:/@git:/@mem: token
- [x] `expand_references(text, base_dir)` ??展开??fenced code block
- [x] @file: 读取本地文件（≤8KB clip），ext→语言映射
- [x] @url: HTTP GET + 4KB clip（reqwest blocking??- [x] @git: git2 recent commits + workdir diff stat
- [x] @mem: placeholder（需运行时记忆集成）
- [x] 集成??cmd_wake ??message 展开后传??LLM
- [x] 目标: ??0 tests ??(13 tests)

**[C2]** Mixture-of-Agents (MoA) ??4并行模型+聚合

- [x] `MoaConfig` ??4个proposer??+ aggregator配置
- [x] `run_moa_sync(query, config, call_llm)` ??顺序proposer+聚合
- [x] `build_aggregator_prompt(query, proposals)` ??标准聚合提示??- [x] `best_fallback_proposal(proposals)` ??聚合器失败时回退
- [x] `format_moa_output(result, verbose)` ??格式化输??- [x] 目标: ?? tests ??(9 tests)

**[C3]** Telegram增强 ??MarkdownV2+媒体批聚??消息分块

- [x] `parse_mode` 字段支持 MarkdownV2
- [x] `chunk_message()` 4096字符限制自动分块
- [x] `escape_markdown_v2()` 特殊字符转义
- [x] `merge_album_photos()` 相册批量合并??0??批）
- [x] 目标: ?? tests ??(8 tests in platform_gateway)

**[C4]** 多平台网????Discord/飞书/钉钉/Email/Slack

- [x] `BasePlatformAdapter` trait 统一接口
- [x] `UnifiedMessageEvent` 跨平台消息结??- [x] `InterruptMode` 三路消息中断模型
- [x] `MediaCacheManager` SSRF保护+指数退避重??- [x] `DiscordAdapter` 实现（connect/send_text/get_chat_info/edit_message??- [x] `FeishuAdapter` 实现（tenant_access_token获取+发送）
- [x] `DingTalkAdapter` 实现（access_token获取+发送）
- [x] `chunk_message_for_platform()` 代码块感知分??- [x] 目标: ?? tests ??(10 tests)

**[C5]** 程序化工具调??(execute_code) ??Python→UDS RPC→工??
- [x] `ExecuteCodeRequest` / `ExecuteCodeResult` 结构定义
- [x] `CodeLanguage` enum (Python/JavaScript)
- [x] `ToolCallRecord` 工具调用记录
- [x] `RpcRequest` / `RpcResponse` UDS协议
- [x] `CodeExecutor` 基础框架（subprocess管理占位??- [ ] Python subprocess + UDS client 实际实现
- [ ] JavaScript/Node.js subprocess + UDS client 实际实现
- [x] 目标: ?? tests ??(8 tests, 基础结构完成)

### Phase D ??RL与高级功能（P3??
**[D1]** On-Policy蒸馏 (AgenticOPDEnv)
**[D2]** OSV漏洞扫描集成
**[D3]** V4A补丁格式

---

## 里程??
| 里程??| 目标 | 测试??|
|--------|------|--------|
| M0 当前基线 | 557 tests green | 557 |
| M1 Phase A完成 | 成本+缓存+压缩+脱敏+扫描 | ~600 |
| M2 Phase B完成 | 多模??路由+SessionDB+检查点 | ~630 |
| M2.5 Phase B/C混合 | FTS5+@引用+MoA | 656 |
| M3 Phase C完成 | @引用+MoA+多平??Telegram增强 | 624 |
| M3.5 Phase C全量 | C1-C5全部基础结构 | 632 |
| **M3.8 关键增强** | **SessionStore+Slash+Batch** | **657** ??|
| M4 Phase D完成 | RL蒸馏+OSV+V4A | ~700 |
| 最终目??| 全量超越Hermes | 700+ |

## 超越判定标准

- [x] Phase A全部5项交????- [x] Phase B全部4项交????- [x] Phase C全部5项交??(C1-C5基础结构完成) ??- [x] 测试套件 **632 tests green** ??(M3.5达成)
- [ ] Phase D全部3项交??(D1/D2/D3 待完??
- [ ] 测试套件??00 tests green (Phase D目标)
- [ ] Zaion独有超越维度维持12个以??- [ ] docs/zaion_vs_hermes.md 对标报告发布

---

## HERMES 深挖补充发现??026-04-11 代理报告??
### 新增 P0 差距——Session管理??
Hermes session架构远超当前认知，补充以下关键差距：

**Session Key算法 (7种组??**
- DM: `{platform}:dm:{chat_id}[:{thread_id}]`
- Group(按user隔离): `{platform}:{type}:{chat_id}:{user_id}[:{thread_id}]`
- Slack线程: DM线程从父DM继承历史

**消息中断模型??路分流）**
- /approve /deny /stop ??bypass active guard，立即dispatch
- PHOTO ??album合并（不中断??- 普通消????interrupt_event + 入pending队列

**Session字段补充**
- `memory_flushed` ??跨重启持久化背景内存flush�??- `was_auto_reset / auto_reset_reason` ??追踪自动重置原因
- `cost_status` ??成本状态（与zaion-pricing集成??- `estimated_cost_usd` ??按session累计成本

### 新增 P0 差距——平台适配器统一接口

**BasePlatformAdapter 必实现方法（Rust trait对等??*
```rust
async fn connect(&self) -> Result<bool>
async fn disconnect(&self) -> Result<()>
async fn send(&self, chat_id, content, reply_to, metadata) -> Result<SendResult>
async fn get_chat_info(&self, chat_id) -> Result<ChatInfo>
// 可选覆盖（有默认impl??
async fn send_image / send_video / send_audio / send_document
async fn send_typing / stop_typing
async fn edit_message
async fn on_processing_start / on_processing_complete
```

**MediaCacheManager（需新增??*
- `cache/images/` / `cache/audio/` / `cache/documents/`
- SSRF保护: is_safe_url() 阻止内网地址
- HTTP重试: 指数退??1.5s × (attempt+1), 处理429/5xx
- 自动从响应内容提??MEDIA:/path 标签路由到对应发送接??
**消息分块（需新增??*
- code-block感知截断（不在代码块中间截断??- chunk indicators ([1/3], [2/3]...)
- Telegram文本批量合并 + photo album合并

### 新增 P1 差距——Cron引擎设计细节

关键 at-most-once 语义:
```
advance_next_run() ??先推进下次时间（防重入）
  ??grace window = min(schedule_interval/2, 120s~2h)
过期超过grace ??fast-forward跳过（防启动burst??```

skip_memory=True: cron执行不污染用户记忆（重要！）
data script + path traversal保护（script字段??
### 新增 P1 差距——ACP Protocol

完整 stdio JSON-RPC agent接口（Zaion完全缺失??
- new_session / load_session / resume_session / fork_session
- ToolCallStart + ToolCallProgress (4类事件回??
- request_permission ??AllowedOutcome (审批协议桥接)
- thinking streaming (update_agent_thought_text)
- MCP server运行时动态注??
---

## 更新后优先级排序（结合代理报告）

### Phase B 调整（P1 ??优先顺序调整??
**B0 [新增P0] 统一平台适配器trait**
- [ ] `ChannelAdapter` trait 扩展: send_image/send_typing/edit_message/get_chat_info
- [ ] `UnifiedMessageEvent` struct (含media_urls/reply_to_text/auto_skill/message_type)
- [ ] `MediaCache` module (image/audio/doc三目?? SSRF保护, 指数退避重??
- [ ] 消息分块 chunk_message(text, max_len) ??code-block感知
- [ ] 目标: ?? tests

**B1 [已有] 工具调用解析器扩??(11种格??**
- [ ] 见原计划

**B2 [调整] SessionStore升级**
- [x] session_key算法 (7种组?? group_per_user/thread_per_user配置)
- [x] SessionEntry扩展字段 (estimated_cost_usd, memory_flushed, was_auto_reset, auto_reset_reason)
- [x] SQLite WAL + FTS5 (已在ledger.rs实现)
- [x] upsert_session / get_by_key / list_by_principal API
- [x] 目标: ?? tests ??(7 tests)

**B3 [已有] CheckpointManager**
- [x] 见原计划 ??
**B4 [新增P1] 三层Session重置策略**
- [x] SessionResetPolicy (daily/idle/both/none)
- [x] reset_by_platform > reset_by_type > default 优先??- [x] reset_triggers (["/new", "/reset"])
- [x] 目标: ?? tests ??(5 tests)

### 新增实现（超??Hermes??
**[E1] Slash命令系统** (8 tests)
- [x] SlashCommand enum (15种命?? retry/undo/compress/rollback/branch/btw/queue/background/stop/approve/deny/verbose/statusbar/skin/reasoning/personality)
- [x] parse_slash_command() 解析??- [x] execute_slash_command() 执行框架（含 retry/queue/background/rollback/compress 结果模型??- [x] 目标: ?? tests ??(8 tests)

**[E2] 批处理训练系??* (4 tests)
- [x] BatchRunner + BatchConfig
- [x] Checkpoint/resume 支持
- [x] ShareGPT格式输出 (trajectories.jsonl)
- [x] ToolsetSample 工具集随机采??- [x] 目标: ?? tests ??(4 tests)

**[E3] Sessions扩展CLI** (1 test)
- [x] sessions browse/stats/export 命令
- [x] sessions delete/prune/rename 命令
- [x] 主路??`zaion sessions` 已切换到 SessionStore 扩展实现
- [x] 目标: ?? test ??(1 test)

---

## HERMES CLI & CONFIG 深挖补充发现??026-04-11 第二份代理报告）

### 新增发现：Config Schema 完整结构（v9??
Hermes config.yaml 包含 25 个顶??Section，关键补充：

**terminal 6种后端（Zaion完全缺失??*
- local / ssh / docker / singularity / modal / daytona
- 每个backend统一参数: container_cpu/memory(MB)/disk(MB)/persistent

**compression（Zaion A3 ContextCompressor 可对标）**
```yaml
compression:
  enabled: true
  threshold: 0.50        # 上下文使用率触发�??  target_ratio: 0.20     # 保留最??0%作为tail
  protect_last_n: 20     # 无论如何保护最后N??  summary_model: "google/gemini-3-flash-preview"
```
压缩触发 parent_session_id 链式分裂，与 SessionDB 集成??
**memory（Zaion对标需求）**
```yaml
memory:
  memory_enabled: true
  memory_char_limit: 2200    # ~800 tokens
  user_char_limit: 1375      # ~500 tokens
  nudge_interval: 10         # 每N轮提醒agent更新记忆
  flush_min_turns: 6         # exit/reset前最少N轮才触发
```

### 新增发现：Slash命令全量??0+??
Hermes 会话??slash 命令远超 Zaion 当前实现，重要补充：

**Session管理??*
- `/retry` ??重发最后消??- `/undo` ??撤销最后一??user/assistant 交换
- `/compress` ??手动触发上下文压缩（对标 A3 ContextCompressor??- `/rollback [checkpoint]` ??文件系统快照回滚（对??B4 CheckpointManager??- `/branch [name]` ??分叉当前会话探索不同路径
- `/btw <question>` ??临时侧边问题（不持久化，无工具）
- `/queue <prompt>` ??队列下一??prompt
- `/background <prompt>` ??后台执行 prompt
- `/stop` ??终止所有后台进??- `/approve` / `/deny` ??危险命令审批

**显示/配置??*
- `/verbose` ??循环切换工具显示级别 off→new→all→verbose
- `/statusbar` ??切换上下??模型状态栏
- `/skin [name]` ??切换显示主题
- `/reasoning [show|hide|effort]` ??管理 reasoning 显示
- `/personality <name>` ??切换14种人格预??
### 新增发现：Skills 系统??0+技能，对标 zaion-evolve??
Hermes 技能系统的关键设计原则??- **渐进式披??(Progressive Disclosure)** ??按需加载减少 token �??- 兼容 agentskills.io 开放标准（Zaion ??zaion-hub 对等??- 技能来?? official / community / local
- 80+个内置技能涵?? GitHub/MLOps/DevOps/SmartHome/Social/Research/Media

**Zaion 需增加的技能分类（高优??*
- [ ] `github` 技能集 (code-review/issues/pr-workflow)
- [ ] `research` 技能集 (arxiv/web-search/polymarket)
- [ ] `software-development` 技能集 (code-review/plan)
- [ ] `productivity` 技能集 (Google Workspace/Notion/Linear)

### 新增发现：SQLite Schema v6 关键字段

补充 sessions 表关键字段（之前遗漏??
```sql
parent_session_id TEXT    -- 压缩分裂/subagent??billing_provider TEXT     -- 计费provider
billing_mode TEXT         -- 计费模式
actual_cost_usd REAL      -- 实际费用（API回报??estimated_cost_usd REAL   -- 估算费用（pricing table??title TEXT UNIQUE         -- 自动编号去重 "title #N"
end_reason TEXT           -- 'compression'|'user'|'idle_timeout'|...
message_count INT, tool_call_count INT
reasoning_tokens INT      -- o-series/claude-thinking
```

### 新增发现：批处理系统（batch_runner.py??
对标 zaion-evolve 的训练数据生成：
- multiprocessing.Pool (4 workers默认)
- 断点续传 (checkpoint.json)
- 输出 ShareGPT 格式 trajectories.jsonl
- 工具集随机采样分??(per-prompt toolset distribution)
- 适配 HuggingFace datasets Parquet/Arrow 格式
- Tinker-Atropos RL 训练框架集成

### 新增发现：OpenClaw 迁移完整覆盖（hermes claw migrate??
迁移范围30+类别，关键路径：
- SOUL.md / MEMORY.md / USER.md 直接导入
- skills/ (4个源目录) ??skills/openclaw-imports/
- exec_approval_patterns.yaml ??config.yaml
- API keys (OpenRouter/OpenAI/Anthropic) ??.env
- cron jobs / plugins / hooks ??归档待手动处??
**??Zaion 的价??*: `zaion import-from-openclaw` 命令需要类似完整度??
---

## 更新后全命令表（Zaion vs Hermes 对等检查）

### Hermes �??Zaion 缺的命令??
| Hermes 命令 | 等价 Zaion 需实现 | 优先??|
|------------|----------------|--------|
| `hermes gateway install/uninstall [--system]` | systemd/launchd服务安装 | P2 |
| `hermes gateway setup` | 交互式gateway配置向导 | P2 |
| `hermes auth list/add/remove/reset` | `zaion auth` (已有) | ??|
| `hermes cron` (全量6子命?? | `zaion cron` (已有) | ??|
| `hermes webhook subscribe/list/remove/test` | webhook子系??| P1 |
| `hermes doctor [--fix]` | `zaion doctor` (已有) | ??|
| `hermes config show/edit/set/path/check/migrate` | `zaion config` (已有部分) | 部分 |
| `hermes pairing list/approve/revoke` | `zaion pair` (已有) | ??|
| `hermes skills browse/search/install/inspect/audit/publish` | `zaion skill` (已有) | ??|
| `hermes honcho setup/status/peers/sessions/map/sync` | Honcho跨会话记??| P3 |
| `hermes memory setup/status/off` | 记忆系统CLI（已有基础控制面，仍需深化??| 部分 |
| `hermes acp` | ACP stdio服务 | P2 |
| `hermes mcp serve/add/remove/list/test/configure` | `zaion mcp add/remove/list/configure/test/serve` 已完成命令族、`McpStore` TOML 持久化、HTTP 健康探针、最??HTTP 服务（`/mcp/v1/health` `/mcp/v1/tools` `/mcp/v1/call`�??8 单测全绿；仍??stdio 子进??JSON-RPC 桥接 | PARTIAL |
| `hermes plugins install/enable/disable/list` | `zaion hub` (类似) | P2 |
| `hermes sessions browse/export/delete/prune/stats/rename` | `zaion sessions` (已完成扩?? | ??|
| `hermes insights [--days] [--source]` | `zaion insights` (已实现A1!) | ??|
| `hermes claw migrate` | openclaw迁移向导 | P2 |
| `hermes profile list/use/create/delete/export/import` | 多profile隔离 | P2 |
| `hermes whatsapp` | WhatsApp适配??| P3 |
| `hermes update` | `zaion update` (已有) | ??|
| `hermes completion bash/zsh` | shell补全生成 | P3 |
| Chat: `/retry /undo /compress /rollback /branch /btw /queue /background` | slash命令扩展（基础结构已完成，行为待深化） | 部分 |
| Chat: `/verbose /statusbar /skin /reasoning /personality` | 显示控制slash命令（基础结构已完成，行为待深化） | 部分 |
