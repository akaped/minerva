# Superhuman Learning

Turn passages, videos and recordings into useful notes. Review the result, keep the
parts that help, and retain a path back to the source. This optional plugin is
independently discovered from `MINERVA_PLUGINS/server/superhuman-learning` and never
compiled into the server.

## Available workflows

| Command | Input | Result | Requirements |
| --- | --- | --- | --- |
| `/summary` | Selection, block, or note | Concise, editable knowledge sections | Python 3 + an AI profile |
| `/wisdom` | Selection, block, or note | Insights, concepts, questions, quotes and ideas | Python 3 + an AI profile |
| `/youtube` | One HTTPS YouTube video URL | Metadata, timestamped captions, optional knowledge extraction | Python 3 + yt-dlp Python package |
| `/transcribe` | Uploaded or existing audio attachment | Timestamped transcript, optional knowledge extraction | Python 3 + configured transcription service |

In an empty block, type `/` and choose a command. While editing a populated block,
use **Ctrl+Shift+P** (Cmd+Shift+P on macOS), optionally selecting text first.

Choose an AI profile for knowledge extraction, or **Transcript only** for media.
Transcript-only YouTube caption import makes no language-model calls. Audio
transcription still uses the configured transcription provider and may incur charges.

The result preview shows captured source text and lets you edit or deselect generated
sections. Transcript insertion is optional. Video references link to original seconds;
audio references identify elapsed time in the original recording. Original source text
and generated results are persisted separately. Notes are ordinary Markdown blocks
and remain readable if the plugin is removed. Insertion uses normal collaboration and undo.

## Install only what you need

### Text workflows

Install Python 3 and configure an existing profile in Minerva’s **AI providers**.
No Python packages, media tools, or Fabric installation are required for text.
`MINERVA_PLUGIN_PYTHON` selects the interpreter; it defaults to `python3`.

### YouTube

Install the `yt-dlp` Python package into that interpreter, explicitly as an
administrator. For example, create a dedicated environment and point Minerva at it:

```sh
python3 -m venv /opt/minerva-media
/opt/minerva-media/bin/pip install yt-dlp
export MINERVA_PLUGIN_PYTHON=/opt/minerva-media/bin/python
```

Use a path your server account can read and execute. Installation is never performed
by plugin initialization. For repeatable deployments, pin and maintain a tested
yt-dlp version. The optional [Dockerfile.example](Dockerfile.example) illustrates
an image with media dependencies; the standard Minerva image stays minimal.

The importer prefers human captions, then automatic captions in the requested
language or a regional variant. It does not silently translate or choose an unrelated
language. It supports `watch`, short links, Shorts, and finished live recordings.
Playlists, live broadcasts, non-YouTube URLs, credentials, and explicit ports are
rejected. Subtitle connections use HTTPS, an exact host allowlist and public-IP
validation with IP-pinned connections, including redirects.

YouTube access can fail because of regional restrictions, rate limits, missing
captions, or platform changes. Restricted/login-only material is not bypassed.
The importer disables external JavaScript runtimes and remote components; videos
that require those facilities may be unavailable. It does not install or fetch code
on demand. No real YouTube download is required by the automated tests.

### Transcription

Add an **OpenAI** or **OpenAI-compatible** profile through existing AI provider
settings. Credentials remain encrypted in Minerva; they are not passed to workers.
Then configure the server environment:

```sh
export MINERVA_ASR_PROVIDER='<existing provider profile ID>'
export MINERVA_ASR_MODEL='<model supporting verbose_json and segment timestamps>'
```

Find profile IDs in the authenticated `GET /api/v1/ai/providers` response. Select a
model/service that implements `POST /audio/transcriptions` with multipart uploads,
`response_format=verbose_json`, and segment timestamps. Not every transcription
model supports that format. See the [transcription API contract](https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create).

Uploaded files use Minerva’s existing attachment storage and permission checks.
Supported headers are WAV, MP3, FLAC, and M4A, with a 10 MiB attachment limit. Original
uploads remain in the note’s Files; deleting a job does not delete an attachment.
Malformed/unsupported audio is rejected locally or by the configured service. The
service must return timed segments; plain text is insufficient. File byte limits
apply before transmission; transcript timing/duration is validated after the response.

### Optional YouTube audio fallback

Copy `config.example.json` to `config.json` inside this directory and explicitly set
`allow_audio_fallback` to `true`. A transcription service must also be configured.
The user must separately check that they have permission to process the audio.
Fallback only runs when no caption track in the requested language is available.

This version retrieves an existing HTTPS M4A audio-only stream up to 20 MiB. It does
not download video, transcode formats, concatenate segments, or remove silence.
FFmpeg and VAD are unnecessary for this path; sources without a supported audio
stream fail with an actionable error. Playback timing is preserved.

Temporary audio is kept in a private per-job directory and removed when the operation
finishes or is cancelled normally. A process kill or machine crash can leave orphan
`minerva-media-*` directories in the OS temporary directory; administrators should
include these in their normal temporary-file cleanup policy. Media is never cached
across users. The configured provider may retain submitted audio according to its policy.

## Configuration and diagnostics

`config.json` accepts exactly these fields:

```json
{
  "preferred_language": "en",
  "max_duration_minutes": 120,
  "allow_audio_fallback": false
}
```

Language codes such as `en`, `it`, and `pt-BR` are accepted. Video duration may be
configured from 1 to 240 minutes. Configuration errors disable media capabilities
and appear in diagnostics; text workflows stay usable.

Set `enabled: false` in `manifest.json` to disable the plugin, or remove its folder.
Commands disappear the next time the command menu opens. Active jobs check plugin
availability/version between stages. An in-flight HTTP request can finish before
that check; disabling is not an immediate provider-side billing cancellation.

`GET /api/v1/server-plugins/capabilities` returns authenticated capability and dependency
information. Missing Python, yt-dlp, AI profiles or ASR configuration disables only
commands needing them. This is a configuration check, not a live provider health check.

## Long sources, progress and retry

Sources may contain up to 192,000 UTF-8 bytes. Text notes support up to 1,000 blocks;
transcripts up to 6,000 segments. Sources are split on character boundaries into
bounded batches, up to **24 model calls**, with a 4,096-token output cap per call.
The output is section-by-section extraction, with exact duplicate items removed;
it is not a hierarchical whole-document synthesis. Very fragmented sources can
reach the 24-batch limit before the byte limit. Nothing is silently truncated.

Each completed model batch and acquired transcript is checkpointed. **Retry unfinished
stages** reuses that job’s authorized source snapshot and successful batches; it
requires the same command, provider profile, plugin version and destination note.
Changing the configured model behind the same profile is an administrator action;
start a new job after such changes to avoid mixing results. **Generate/Import**
always starts fresh. There are no automatic paid retries or cross-user result caches.

One job per user and two jobs per server may run concurrently, with 20 submissions
per user per hour. Worker stages time out after 10 seconds (media acquisition: 180);
model HTTP requests after 120 seconds; complete jobs after 30 minutes. Cancelling
stops local requests and workers; a provider may already have charged for work.
Closing the dialog leaves processing running. Reopening the same command in the same
browser tab restores its most recent job; the job ID is kept in session storage.

A server restart marks unfinished jobs `interrupted`. Failed extraction can still
expose the acquired transcript for insertion. Terminal jobs older than seven days
are purged on the next submission (opportunistic cleanup, not a timed guarantee).
`DELETE /api/v1/server-plugins/jobs/{id}` removes source/result payloads and stops work;
a small rate-limit tombstone remains until cleanup. Backups may retain older data.
Jobs are server-local and are not replicated with notes.

## Integrity and security

Structured items include `kind`, `text`, `source_ids`, and `grounding`. Every extraction
must cite an existing segment; quotes must match a cited segment exactly. Interpretations
are labelled as analysis. These checks establish provenance, not factual accuracy.
Edits in the preview are user-authored and are not revalidated as exact quotations.

Workers are trusted administrator-installed code running as the server account,
**not an operating-system sandbox**. Protect plugin directories from member writes.
No shell command or executable path is accepted from editor users. The host keeps
credentials, checks note access and job ownership, and bounds worker inputs, outputs,
concurrency and time. Media/AI content is not logged by default. Prompts treat imported
content as data and expose no model tools; review remains necessary.

Original prompts are inspired by the idea of reusable patterns, not copied Fabric
files. No Fabric runtime dependency is introduced.

## Developer protocol and tests

Protocol 1 uses one JSON request on stdin and one JSON response on stdout per worker
process, with Python isolated mode (`-I`). `prepare` builds an AI request; `validate`
checks structured output. Media-enabled manifests additionally support `diagnostics`
and `ingest`; command input types are `text`, `youtube`, or `audio`. API keys are never
part of this protocol. Bump the manifest version when changing pattern behavior.

```sh
python3 -m unittest discover -s plugins/server/superhuman-learning -p 'test_*.py'
cargo test -p minerva-server --lib server_plugins
cargo test -p minerva-server --test learning --test ai_stream
npm test
npm run build
```

Tests use local mock model/ASR providers and mocked YouTube metadata/captions. Live
platform compatibility and provider-specific audio acceptance still depend on the
administrator’s installed versions, configuration, and permitted test sources.

Next directions: hierarchical synthesis, editable custom patterns, VAD/format
conversion, study notes, flashcards, and spaced repetition.
