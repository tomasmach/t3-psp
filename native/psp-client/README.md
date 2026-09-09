# T3 PSP (experimental)

Native Rust remote client for PSP-3000 homebrew. T3, providers, and speech recognition run on the desktop. The PSP displays recent threads and messages, records its microphone, reviews transcripts, sends prompts, and requests interruption. No Linux or JavaScript runtime runs on the PSP.

Builds with `psp 0.3.13`, `cargo-psp 0.2.9`, and the pinned nightly toolchain:

```sh
cargo install cargo-psp --version 0.2.9 --locked
cd native/psp-client
cargo psp --release --locked
```

Copy `target/mipsel-sony-psp/release/EBOOT.PBP` to `PSP/GAME/T3PSP/EBOOT.PBP` on the memory card. Copy `gateway.cfg.example` alongside it as `gateway.cfg`, then set the desktop LAN address and the gateway's separate token. Never put T3 credentials or Wi-Fi passwords in the EBOOT or commit them.

The EBOOT includes the T3 PSP icon and embedded Czech fonts. No extra asset files need to be copied to the PSP.

Run the [desktop gateway](https://github.com/tomasmach/t3-psp/blob/psp/apps/psp-gateway/README.md) and allow its port from your local network. Set up a saved infrastructure Wi-Fi connection in the PSP's Network Settings first. On startup, select that connection from the saved-profile list. `profile=1` only sets the initial highlighted profile; it does not force the connection. Switch WLAN on, leave USB mode, and launch **Game → Memory Stick → T3 PSP**. PSP requires a compatible 2.4 GHz 802.11b access point; WPA2 additionally requires compatible ARK firmware with WPA2 support enabled. This program does not change firmware or overwrite Wi-Fi profiles.

## Controls

| Screen                   | Control             | Action                                                            |
| ------------------------ | ------------------- | ----------------------------------------------------------------- |
| Lists / thread / draft   | Up / Down or analog | Move one item or line; hold to keep moving                        |
| Lists / thread / draft   | Left / Right        | Move back / forward by a page (four items in lists)               |
| Threads                  | X                   | Open selected thread                                              |
| Thread                   | L / R               | Messages / activity; each keeps its scroll position               |
| Thread                   | Triangle            | Load and follow the latest content                                |
| Thread                   | X                   | Write a prompt, or review this thread's saved draft               |
| Thread / draft review    | Square              | Record a voice prompt                                             |
| Working / waiting thread | Start, then X       | Confirm an interruption request; Circle cancels                   |
| Thread                   | Circle              | Open the thread sidebar                                           |
| Thread sidebar           | X / Circle          | Open selected thread / return without losing the reading position |
| Draft review             | X                   | Edit with the keyboard                                            |
| Draft review             | Start / Circle      | Send reviewed draft / return with draft kept                      |
| Keyboard                 | D-pad / X           | Select character / type; directions wrap within the row or column |
| Keyboard                 | Square / Triangle   | Backspace (hold to keep deleting) / switch letter case            |
| Keyboard                 | Analog or L / R     | Scroll draft text                                                 |
| Keyboard                 | Circle              | Return to draft review; Start does not send from the keyboard     |
| Recording                | Square / Circle     | Stop and transcribe / cancel                                      |
| Thread list or detail    | Select              | Open Wi-Fi profiles; Circle returns without reconnecting          |
| Wi-Fi profiles           | X                   | Connect to the selected saved profile                             |
| Dialog                   | Circle / X          | Back / acknowledge                                                |
| Any screen               | Home                | Exit                                                              |

The MVP polls every two seconds and shows up to 32 recent unarchived threads, the latest eight messages and five activity summaries. Circle opens the thread sidebar; closing it or reopening the same thread keeps your reading position. Reading history freezes that view; Triangle loads the newest content. Use the desktop for older history, approvals, and structured questions. Drafts stay with their thread until sent or the app exits. Requests block input while waiting; errors have timeouts, and a failed send retains the draft without automatic retry. Check the desktop thread before manually resending an ambiguous request.

The analog stick has a center dead zone and scrolls faster when pushed farther. D-pad navigation and keyboard backspace repeat after a short hold; other actions never repeatedly type, record, send, or stop the agent. Returning from a dialog or recording requires a fresh action press.

Recording continues until Square is pressed again; Circle discards it. Audio uploads in small blocks while recording, with a bounded queue on the PSP and temporary storage on the desktop. There is no fixed recording-duration limit. Desktop disk space and transcription resources remain finite, and transcripts/prompts are limited to 60,000 UTF-8 bytes. A full upload queue or connection failure cancels the recording explicitly; audio is never silently dropped. Stopping includes the remainder of the current microphone block (at most about 0.4 seconds). Transcripts always require a separate Start press before sending.

## Verification

The EBOOT cross-build, formatting check and host tests can run without the device (requires the stable toolchain too):

```sh
bash tools/check.sh
```

Generate screen previews separately:

```sh
rustc +stable --edition 2024 tools/preview.rs -o target/gui-preview
target/gui-preview
```

The preview writes `/tmp/t3-psp-gui-*.ppm` using the actual renderer, fonts, and screen layouts. It does not emulate PSP hardware. Font/icon regeneration uses `tools/generate_assets.py` with Pillow, CairoSVG, and the Fedora DejaVu Sans font paths; the generated assets and font license are checked in.

Desktop HTTP tests are in the gateway package. Physical PSP verification must cover boot, Wi-Fi association, thread refresh, microphone capture, transcript review, prompt submission and interruption. A successful EBOOT build does not verify those hardware paths.

For redistribution, run `bash tools/package.sh` after building. It prints a new package directory containing the EBOOT, example configuration and required [third-party notices](THIRD_PARTY_NOTICES.md). Keep the license files with the distributed binary. This needs `jq` and the pinned toolchain's `rust-docs` component (`rustup component add rust-docs`). Never package your own `gateway.cfg`.
