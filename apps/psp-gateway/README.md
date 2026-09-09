# PSP gateway

Runs on the desktop and connects the native PSP client to one T3 environment. It reads current thread snapshots, sends prompts, interrupts turns, and optionally transcribes microphone audio locally. It does not launch a T3 server or open a database.

## Start

Install workspace dependencies with `vp i`, then run from the repository root with the project's Node 24 runtime:

```sh
export T3_PSP_SERVER_URL='http://127.0.0.1:3773'
export T3_PSP_T3_TOKEN='your-T3-bearer-access-token'
export T3_PSP_TOKEN='a-separate-random-gateway-token'
export T3_PSP_HOST='0.0.0.0'
node apps/psp-gateway/src/main.ts
```

Use your environment's actual T3 URL. Alternatively, omit `T3_PSP_T3_TOKEN` and set `T3_PSP_PAIRING_TOKEN` to an unused T3 pairing credential (the token from a pairing link). The gateway exchanges it once on startup and keeps the resulting access token in memory. Restart with a fresh pairing credential when it expires. Pairing credentials are single use; they are not bearer access tokens.

`T3_PSP_HOST` defaults to `127.0.0.1`, `T3_PSP_PORT` to `8787`. Binding the host to a LAN address lets the PSP connect. Set the PSP gateway URL to `http://<desktop-LAN-IP>:8787` and use the same `T3_PSP_TOKEN`. The PSP connection uses plain HTTP: use a trusted local network and do not expose this port to the internet. The token grants access to this environment's threads, prompts, and interrupts.

For microphone transcription, install [whisper.cpp](https://github.com/ggml-org/whisper.cpp) separately. Start with its multilingual `small` model (`ggml-small.bin`); `.en` models only support English. See the [model download instructions](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md).

```sh
export T3_PSP_WHISPER_BIN='/path/to/whisper-cli'
export T3_PSP_WHISPER_MODEL='/path/to/ggml-small.bin'
export T3_PSP_WHISPER_LANGUAGE='auto'
```

For Czech-only dictation, set `T3_PSP_WHISPER_LANGUAGE=cs`; leave `auto` for language detection. Restart the gateway after changing these variables.

The binary defaults to `whisper-cli`. Without a model, transcription returns HTTP 503. Audio stays on the desktop and streams to a private temporary file. The gateway resamples it into a mono 16 kHz WAV using fixed-size windows, then Whisper transcribes the complete recording. Gateway audio memory stays bounded; Whisper's memory usage depends on recording length. Recording has no elapsed-time limit; available disk space and the WAV format's 4 GiB size limit still apply. Up to four recording sessions can exist, with one transcription worker shared by streamed and legacy requests. The worker timeout scales with audio duration (20 times its length, with a 60-second minimum). Audio is removed after transcription, failure, cancellation, or gateway shutdown. Transcription only returns editable text. Sending the prompt is a separate request.

## PSP protocol v1

All requests require `Authorization: Bearer <T3_PSP_TOKEN>`. Responses are UTF-8 tab-separated records ending in LF. Inside fields, escape backslash as `\\`, tab as `\t`, CR as `\r`, and LF as `\n`. Decode escapes after splitting records and fields. Responses never exceed 65535 bytes. Thread snapshots may omit whole trailing records; oversized transcripts return an error and are never truncated.

| Request                                     | Response                                                                                    |
| ------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `GET /v1/threads`                           | Up to 32 `THREAD\tid\tstatus\ttitle` records, newest first, archived threads excluded       |
| `GET /v1/threads/:id`                       | One `THREAD` record, up to 8 `MSG\trole\ttext` records, then up to 5 `ACT\tsummary` records |
| `POST /v1/threads/:id/prompt`               | UTF-8 plain text request body; `OK\tsequence` on acceptance                                 |
| `POST /v1/threads/:id/interrupt`            | Empty request body; `OK\tsequence` on acceptance                                            |
| `POST /v1/transcriptions`                   | Raw signed little-endian PCM16, mono, 11025 Hz; `TEXT\ttranscript` response                 |
| `POST /v1/recordings`                       | Empty body; `RECORDING\tid`                                                                 |
| `POST /v1/recordings/:id/chunks?sequence=N` | Raw PCM16 mono 11025 Hz, 4096 samples per chunk; `OK\tnextSequence`                         |
| `POST /v1/recordings/:id/finish`            | Empty body; `PENDING`, starts transcription asynchronously                                  |
| `GET /v1/recordings/:id`                    | `PENDING`, `TEXT\ttranscript`, or non-2xx `ERROR\tmessage`                                  |
| `POST /v1/recordings/:id/cancel`            | Empty body; `OK`, including already removed recordings                                      |

Recording chunk sequences start at zero and must arrive in order. Only the most recently accepted chunk can be retried, with exactly the same bytes. A final chunk may contain fewer than 4096 samples; every chunk must contain a positive, even number of bytes and at most 8192 bytes. Repeating finish never starts another worker. Poll while transcription is pending; cancel remains available throughout. Unfinished recordings expire after five minutes without an accepted chunk, and completed results remain available for five minutes. The older `/v1/transcriptions` endpoint still accepts a single body of up to 30 seconds of audio.

Thread IDs in paths must be URL-encoded. Poll the list or selected detail every 2 seconds, with only one poll in flight. Status is `running`, `idle`, `waiting` (approval or user input pending), or `error`. Approvals, tools, and structured questions must be handled in a full T3 client. Thread titles are capped at 160 characters, message tails at 3000 characters each, and activities at 200. Transcripts exceeding 60000 UTF-8 bytes or the escaped response ceiling return an error. Detail reads request the most recent four user turns from T3. Provider selection and runtime/interaction modes are taken from the existing thread when a prompt is submitted.

POST prompt and interrupt accept optional `X-Request-Id` (1–100 ASCII letters, digits, underscores, or hyphens). Repeated IDs on the same thread/action reuse the result; a different body returns 409. The last 256 results are retained in memory and command IDs are deterministic for upstream deduplication. Do not blindly retry an ambiguous POST without the same request ID. `OK` means T3 accepted the command; poll to observe execution.

Failures use a non-2xx HTTP status and `ERROR\tmessage`. Authentication returns 401, malformed input 400, missing routes/threads/recordings 404, conflicting chunks 409, oversized bodies 413, failed asynchronous transcription 422, busy transcription or full recording capacity 429, and unavailable transcription 503. Upstream failures return 502 (404 is preserved). Prompts are limited to 60000 bytes.

Add `?projectNames=1` to either thread GET endpoint to append the project name as a fifth `THREAD` field. Without it, responses keep the original four-field format.

## Verify

```sh
pnpm --filter @t3tools/psp-gateway test
pnpm exec tsc --noEmit -p apps/psp-gateway/tsconfig.json
```

Tests run a real HTTP gateway against a fake T3 HTTP server. They verify authentication, snapshots, command payloads, deduplication, interrupt, transcription response framing, and WAV conversion. They do not send prompts to real threads or verify a physical PSP, microphone, or installed whisper model.
