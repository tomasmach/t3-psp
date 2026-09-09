# T3 PSP

An unofficial, experimental PSP-3000 client for T3 Code, maintained by Tomáš Mach. T3 Code and all agents run on a desktop. The PSP runs a native Rust EBOOT and connects through a small LAN gateway. This is not a Sony or T3 Tools product.

## Start

1. Keep your existing T3 Code desktop/server running. The gateway connects to it; it never opens its database or replaces the desktop installation.
2. Clone the `psp` branch: `git clone --branch psp https://github.com/tomasmach/t3-psp.git`. Install the Node version specified in `package.json`, [Vite+](https://viteplus.dev/), then run `vp i` at the root.
3. Follow the [gateway setup](apps/psp-gateway/README.md) to pair with your T3 environment and optionally configure local whisper.cpp transcription. Use a multilingual model; `small` is the tested starting point for Czech.
4. Build and install the [native client](native/psp-client/README.md), or download the `t3-psp` artifact from a successful **PSP** GitHub Actions run. Keep its `licenses` directory with the EBOOT when redistributing it. Set your own desktop address and gateway token in `gateway.cfg`.
5. Select a compatible saved Wi-Fi profile on the PSP and open a thread. Square starts/stops recording, Circle cancels it, and Start sends the reviewed transcript.

Use a trusted LAN. The PSP link is unencrypted HTTP and its bearer token can control the paired environment. Never expose the gateway port to the internet. Do not put tokens, pairing links, real gateway configuration or recordings into Git. Pairing tokens are single-use and access tokens expire; see the gateway guide before restarting it.

## What is verified

Development testing on one PSP-3000 with ARK-5 covered boot, Wi-Fi selection, real thread browsing, voice prompt submission, and recording cancellation followed by another recording. Automated checks exercise the gateway through HTTP against fake T3 data, the native queue/model/renderer, and the EBOOT cross-build. They do not replace a device test.

Lower microphone gain and the `small` transcription model are included. The model improved one Czech sample, but the lower gain still needs a new hardware recording to verify clipping. Interrupt is covered by gateway tests; do not treat that as confirmation of every provider on hardware. History is bounded, approvals and structured questions remain desktop tasks, and Wi-Fi recovery can briefly block controls. There is no claim of compatibility with every future T3 release.

Custom PSP firmware, CXMB, Windows XP themes, speech models and recordings are not distributed by this fork. Theme troubleshooting is separate from T3 PSP.

## Update from T3 Code

Keep `main` identical to upstream. Develop and release the PSP changes from `psp` (the default branch). This preserves normal Git history and makes conflicts visible. Both branches are protected against deletion and force-pushes in this fork. Do not use GitHub's **discard changes** option on `psp` or force-push an upstream branch over it.

In a fresh clone, add the original repository once:

```sh
git remote add upstream https://github.com/pingdotgg/t3code.git
```

Use a clean worktree with no uncommitted changes. Prepare an update on a separate branch:

```sh
git fetch upstream main
git fetch origin psp
git switch -c chore/update-t3-YYYY-MM-DD origin/psp
git merge --no-edit upstream/main
vp i
bash native/psp-client/tools/check.sh
node --test apps/psp-gateway/src/*.test.ts
vp exec tsc --noEmit -p apps/psp-gateway/tsconfig.json
```

Resolve conflicts explicitly; never accept an entire side blindly. If the merge cannot be completed, `git merge --abort` returns to the pre-merge state. The PSP-specific changes are confined to `native/psp-client`, `apps/psp-gateway`, this documentation, the README introduction, the workspace lockfile, and the PSP workflow. Pay particular attention to changes in `packages/contracts` and T3's authenticated HTTP endpoints.

Push the update branch to **your fork**, review its diff against `psp`, and wait for the PSP workflow (pushes to `chore/update-t3-*` run it automatically). Test list → open thread → record → cancel → record → review/send against a disposable T3 environment, then on hardware before adopting it. Only then merge the update branch into `psp`. Advancing the fork's clean `main` can be done separately with GitHub's **Sync fork** on `main`; it does not update `psp` or your installed applications.

This development worktree keeps `origin` pointing to `pingdotgg/t3code` and uses `psp-fork` for publication. Do not run the fresh-clone commands against that worktree without substituting the correct remotes. Its shared Git configuration is intentionally not renamed.

## License and attribution

T3 Code remains under its original [MIT license](LICENSE), including copyright © 2026 T3 Tools Inc. The PSP additions are also MIT-licensed. Existing upstream notices remain in place. [PSP third-party notices](native/psp-client/THIRD_PARTY_NOTICES.md) describe the fonts and linked runtime dependencies; packaged builds include their license texts. This fork does not claim ownership of T3 Code, PSP trademarks or third-party themes.

Inherited workflows are disabled in this fork's GitHub Actions settings; their source files stay unchanged to reduce merge conflicts. After an upstream update, check for newly added workflows and keep upstream deployment/release jobs disabled. Only the **PSP** workflow is enabled here. It builds a downloadable artifact and never deploys to a desktop or device. A passing build is not a hardware compatibility guarantee.
