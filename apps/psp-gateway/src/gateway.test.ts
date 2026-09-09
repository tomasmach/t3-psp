import * as NodeAssert from "node:assert/strict";
import * as NodeHttp from "node:http";
import * as NodeTest from "node:test";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeOS from "node:os";
import { createGateway, escapeField } from "./gateway.ts";
import { pcmToWav, pcmFileToWav, whisperFileTranscriber } from "./transcribe.ts";
import { Recordings } from "./recordings.ts";

const date = "2026-09-09T10:00:00.000Z";
const thread = {
  id: "test-thread",
  projectId: "test-project",
  title: "Test\tthread\nTitle",
  modelSelection: { instanceId: "codex", model: "gpt-test" },
  runtimeMode: "approval-required",
  interactionMode: "plan",
  branch: null,
  worktreePath: null,
  latestTurn: null,
  createdAt: date,
  updatedAt: date,
  archivedAt: null,
  settledOverride: null,
  settledAt: null,
  session: null,
  latestUserMessageAt: null,
  hasPendingApprovals: true,
  hasPendingUserInput: false,
  hasActionableProposedPlan: false,
};

async function listen(server: NodeHttp.Server) {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  NodeAssert.ok(address && typeof address !== "string");
  return `http://127.0.0.1:${address.port}`;
}
async function close(server: NodeHttp.Server) {
  server.closeAllConnections();
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}

NodeTest.test(
  "real HTTP gateway against fake T3: auth, bounded snapshots, modes, deduplication and interrupt",
  async () => {
    const commands: Record<string, unknown>[] = [];
    let shellThreads: unknown[] = [thread];
    let shellProjects: unknown[] = [
      {
        id: thread.projectId,
        title: "Na Pivo\tČeské\\projekty\nVývoj",
        workspaceRoot: "/private/workspaces/not-for-client",
        defaultModelSelection: null,
        scripts: [],
        createdAt: date,
        updatedAt: date,
      },
    ];
    const upstream = NodeHttp.createServer(async (request, response) => {
      NodeAssert.equal(request.headers.authorization, "Bearer upstream-secret");
      response.setHeader("content-type", "application/json");
      if (request.url === "/api/orchestration/shell")
        response.end(
          JSON.stringify({
            snapshotSequence: 1,
            projects: shellProjects,
            threads: shellThreads,
            updatedAt: date,
          }),
        );
      else if (request.url === "/api/orchestration/threads/test-thread?turnLimit=4")
        response.end(
          JSON.stringify({
            snapshotSequence: 1,
            thread: {
              ...thread,
              deletedAt: null,
              checkpoints: [],
              messages: Array.from({ length: 12 }, (_, i) => ({
                id: `message-${i}`,
                role: "assistant",
                text: `${i}:` + "x".repeat(4000),
                turnId: null,
                streaming: false,
                createdAt: date,
                updatedAt: date,
              })),
              activities: Array.from({ length: 8 }, (_, i) => ({
                id: `activity-${i}`,
                tone: "info",
                kind: "tool",
                summary: "Working" + "x".repeat(300),
                payload: {},
                turnId: null,
                createdAt: date,
              })),
            },
          }),
        );
      else if (request.url === "/api/orchestration/dispatch") {
        const buffers = [];
        for await (const chunk of request) buffers.push(Buffer.from(chunk));
        commands.push(JSON.parse(Buffer.concat(buffers).toString()));
        response.end(JSON.stringify({ sequence: commands.length }));
      } else {
        response.statusCode = 404;
        response.end("{}");
      }
    });
    const serverUrl = await listen(upstream);
    const gateway = createGateway({
      serverUrl,
      t3Token: "upstream-secret",
      token: "psp-secret",
      transcribe: async (pcm) => {
        NodeAssert.equal(pcm.length, 4);
        return "Ahoj\nsvěte";
      },
    });
    const gatewayUrl = await listen(gateway);
    const headers = { authorization: "Bearer psp-secret" };
    try {
      NodeAssert.equal((await fetch(`${gatewayUrl}/v1/threads`)).status, 401);
      NodeAssert.equal(
        await (await fetch(`${gatewayUrl}/v1/threads`, { headers })).text(),
        "THREAD\ttest-thread\twaiting\tTest\\tthread\\nTitle\n",
      );
      const detail = await (
        await fetch(`${gatewayUrl}/v1/threads/test-thread`, { headers })
      ).text();
      NodeAssert.match(detail, /^THREAD\ttest-thread\twaiting\t/);
      NodeAssert.equal(detail.split("\n").filter((record) => record.startsWith("MSG\t")).length, 8);
      NodeAssert.equal(detail.split("\n").filter((record) => record.startsWith("ACT\t")).length, 5);
      NodeAssert.ok(detail.length < 65536);
      const namedHeader =
        "THREAD\ttest-thread\twaiting\tTest\\tthread\\nTitle\tNa Pivo\\tČeské\\\\projekty\\nVývoj";
      for (const path of ["/v1/threads", "/v1/threads/test-thread"]) {
        const named = await (
          await fetch(`${gatewayUrl}${path}?projectNames=1`, { headers })
        ).text();
        NodeAssert.equal(named.split("\n")[0], namedHeader);
        NodeAssert.equal(named.split("\n")[0]?.split("\t").length, 5);
        NodeAssert.ok(!named.includes("/private/workspaces"));
        const legacy = await (await fetch(`${gatewayUrl}${path}`, { headers })).text();
        NodeAssert.equal(
          legacy.split("\n")[0],
          "THREAD\ttest-thread\twaiting\tTest\\tthread\\nTitle",
        );
      }
      shellProjects = [];
      for (const path of ["/v1/threads", "/v1/threads/test-thread"]) {
        const unnamed = await (
          await fetch(`${gatewayUrl}${path}?projectNames=1`, { headers })
        ).text();
        NodeAssert.equal(
          unnamed.split("\n")[0],
          "THREAD\ttest-thread\twaiting\tTest\\tthread\\nTitle\t",
        );
      }
      const prompt = {
        method: "POST",
        headers: { ...headers, "x-request-id": "send-1" },
        body: "Test prompt",
      };
      NodeAssert.equal(
        await (await fetch(`${gatewayUrl}/v1/threads/test-thread/prompt`, prompt)).text(),
        "OK\t1\n",
      );
      NodeAssert.equal(
        await (await fetch(`${gatewayUrl}/v1/threads/test-thread/prompt`, prompt)).text(),
        "OK\t1\n",
      );
      NodeAssert.equal(commands.length, 1);
      NodeAssert.deepEqual(commands[0]?.modelSelection, thread.modelSelection);
      NodeAssert.equal(commands[0]?.runtimeMode, "approval-required");
      NodeAssert.equal(commands[0]?.interactionMode, "plan");
      NodeAssert.equal(
        (
          await fetch(`${gatewayUrl}/v1/threads/test-thread/prompt`, {
            ...prompt,
            body: "different",
          })
        ).status,
        409,
      );
      NodeAssert.equal(
        (
          await fetch(`${gatewayUrl}/v1/threads/test-thread/prompt`, {
            method: "POST",
            headers,
            body: " ",
          })
        ).status,
        400,
      );
      NodeAssert.equal(
        (
          await fetch(`${gatewayUrl}/v1/threads/missing/prompt`, {
            method: "POST",
            headers,
            body: "test",
          })
        ).status,
        404,
      );
      NodeAssert.equal(
        (await fetch(`${gatewayUrl}/v1/threads/test-thread/interrupt`, { method: "POST", headers }))
          .status,
        200,
      );
      NodeAssert.equal(commands[1]?.type, "thread.turn.interrupt");
      NodeAssert.equal(
        await (
          await fetch(`${gatewayUrl}/v1/transcriptions`, {
            method: "POST",
            headers,
            body: Buffer.alloc(4),
          })
        ).text(),
        "TEXT\tAhoj\\nsvěte\n",
      );
      NodeAssert.equal(
        (
          await fetch(`${gatewayUrl}/v1/transcriptions`, {
            method: "POST",
            headers,
            body: Buffer.alloc(3),
          })
        ).status,
        400,
      );
      for (const sessionStatus of ["starting", "running", "error"] as const) {
        shellThreads = [
          {
            ...thread,
            hasPendingApprovals: false,
            session: {
              threadId: thread.id,
              status: sessionStatus,
              providerName: "codex",
              runtimeMode: thread.runtimeMode,
              activeTurnId: null,
              lastError: sessionStatus === "error" ? "Failed" : null,
              updatedAt: date,
            },
          },
        ];
        const records = await (await fetch(`${gatewayUrl}/v1/threads`, { headers })).text();
        NodeAssert.equal(records.split("\t")[2], sessionStatus === "error" ? "error" : "running");
      }
      shellThreads = [{ ...thread, hasPendingApprovals: false, backgroundLiveness: "working" }];
      NodeAssert.equal(
        (await (await fetch(`${gatewayUrl}/v1/threads`, { headers })).text()).split("\t")[2],
        "running",
      );
    } finally {
      await close(gateway);
      await close(upstream);
    }
  },
);

NodeTest.test("PCM16 is resampled into a valid mono 16 kHz WAV", () => {
  const pcm = Buffer.alloc(11025 * 2);
  for (let i = 0; i < 11025; i++) pcm.writeInt16LE(1234, i * 2);
  const wav = pcmToWav(pcm);
  NodeAssert.equal(wav.toString("ascii", 0, 4), "RIFF");
  NodeAssert.equal(wav.readUInt32LE(24), 16000);
  NodeAssert.equal(wav.readUInt16LE(22), 1);
  NodeAssert.equal(wav.length, 32044);
  NodeAssert.equal(wav.readInt16LE(44), 1234);
  NodeAssert.equal(wav.readInt16LE(wav.length - 2), 1234);
  NodeAssert.throws(() => pcmToWav(Buffer.alloc(1)));
  NodeAssert.equal(escapeField("a\\b\t\n\r"), "a\\\\b\\t\\n\\r");
});

NodeTest.test(
  "streamed file resampling matches PCM conversion across windows and the final sample",
  async () => {
    const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "psp-resample-test-"));
    try {
      const pcm = Buffer.alloc(11025 * 2 * 31 + 14);
      for (let i = 0; i < pcm.length / 2; i++) pcm.writeInt16LE(((i * 37) % 65536) - 32768, i * 2);
      const source = NodePath.join(directory, "audio.pcm");
      const destination = NodePath.join(directory, "audio.wav");
      await NodeFSP.writeFile(source, pcm);
      await pcmFileToWav(source, destination, new AbortController().signal);
      NodeAssert.deepEqual(await NodeFSP.readFile(destination), pcmToWav(pcm));
      const aborted = new AbortController();
      aborted.abort();
      await NodeAssert.rejects(
        pcmFileToWav(source, NodePath.join(directory, "cancelled.wav"), aborted.signal),
        { name: "AbortError" },
      );
    } finally {
      await NodeFSP.rm(directory, { recursive: true, force: true });
    }
  },
);

NodeTest.test(
  "file transcriber passes a recording beyond five minutes intact to the worker",
  async () => {
    const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "psp-whisper-test-"));
    try {
      const binary = NodePath.join(directory, "whisper");
      await NodeFSP.copyFile(new URL("./fixtures/whisper.mjs", import.meta.url), binary);
      await NodeFSP.chmod(binary, 0o700);
      const pcm = Buffer.alloc(11025 * 2 * 301);
      for (let i = 0; i < pcm.length / 2; i++)
        pcm.writeInt16LE(i < 11025 * 300 ? 1234 : -1234, i * 2);
      const source = NodePath.join(directory, "audio.pcm");
      await NodeFSP.writeFile(source, pcm);
      const transcribe = whisperFileTranscriber(binary, "fake-model");
      NodeAssert.equal(
        await transcribe(source, new AbortController().signal),
        "4816000:1234:-1234\n",
      );
    } finally {
      await NodeFSP.rm(directory, { recursive: true, force: true });
    }
  },
);

NodeTest.test(
  "recordings stream over 30 seconds in order, retry safely, finish asynchronously, and never dispatch",
  async () => {
    const result = Promise.withResolvers<string>();
    const started = Promise.withResolvers<void>();
    let calls = 0;
    let pcmPath = "";
    let received = Buffer.alloc(0);
    const gateway = createGateway({
      serverUrl: "http://127.0.0.1:1",
      t3Token: "unused",
      token: "test",
      transcribeFile: async (path) => {
        calls++;
        pcmPath = path;
        received = await NodeFSP.readFile(path);
        started.resolve();
        return result.promise;
      },
    });
    const origin = await listen(gateway);
    const headers = { authorization: "Bearer test" };
    const post = (path: string, body?: Buffer) =>
      fetch(origin + path, {
        method: "POST",
        headers,
        ...(body ? { body: Buffer.from(body) } : {}),
      });
    try {
      NodeAssert.equal((await fetch(origin + "/v1/recordings", { method: "POST" })).status, 401);
      const created = await (await post("/v1/recordings")).text();
      NodeAssert.match(created, /^RECORDING\t[a-zA-Z0-9_-]+\n$/);
      const route = `/v1/recordings/${created.trim().split("\t")[1]}`;
      NodeAssert.equal((await post(route + "/finish")).status, 400);
      const expected = Buffer.alloc(11025 * 2 * 35 + 10);
      for (let i = 0; i < expected.length / 2; i++)
        expected.writeInt16LE(((i * 7) % 65536) - 32768, i * 2);
      let sequence = 0;
      for (let offset = 0; offset < expected.length; offset += 8192) {
        const chunk = expected.subarray(offset, offset + 8192);
        const path = route + `/chunks?sequence=${sequence}`;
        NodeAssert.equal(await (await post(path, chunk)).text(), `OK\t${sequence + 1}\n`);
        if (sequence === 0) {
          NodeAssert.equal(await (await post(path, chunk)).text(), "OK\t1\n");
          NodeAssert.equal((await post(path, Buffer.alloc(8192))).status, 409);
          NodeAssert.equal((await post(route + "/chunks?sequence=2", chunk)).status, 409);
          NodeAssert.equal((await post(route + "/chunks?sequence=1", Buffer.alloc(3))).status, 400);
          NodeAssert.equal(
            (await post(route + "/chunks?sequence=1", Buffer.alloc(8194))).status,
            413,
          );
        }
        sequence++;
      }
      NodeAssert.equal(
        (await post(route + `/chunks?sequence=${sequence}`, Buffer.alloc(2))).status,
        409,
      );
      NodeAssert.equal(await (await post(route + "/finish")).text(), "PENDING\n");
      await started.promise;
      NodeAssert.deepEqual(received, expected);
      NodeAssert.equal(await (await post(route + "/finish")).text(), "PENDING\n");
      NodeAssert.equal(await (await fetch(origin + route, { headers })).text(), "PENDING\n");
      NodeAssert.equal(calls, 1);
      result.resolve(" Ahoj\nsvěte\t\\ ");
      NodeAssert.equal(
        await (await fetch(origin + route, { headers })).text(),
        "TEXT\tAhoj\\nsvěte\\t\\\\\n",
      );
      await NodeAssert.rejects(NodeFSP.stat(NodePath.dirname(pcmPath)), { code: "ENOENT" });
      NodeAssert.equal(await (await post(route + "/cancel")).text(), "OK\n");
      NodeAssert.equal(await (await post(route + "/cancel")).text(), "OK\n");
      NodeAssert.equal((await fetch(origin + route, { headers })).status, 404);
    } finally {
      result.resolve("");
      await close(gateway);
    }
  },
);

NodeTest.test(
  "recording cancellation aborts transcription, bounds sessions and cleans audio",
  async () => {
    const started = Promise.withResolvers<void>();
    const aborted = Promise.withResolvers<void>();
    let pcmPath = "";
    const gateway = createGateway({
      serverUrl: "http://127.0.0.1:1",
      t3Token: "unused",
      token: "test",
      transcribe: async () => "legacy",
      transcribeFile: async (path, signal) => {
        pcmPath = path;
        started.resolve();
        return new Promise<string>((_resolve, reject) =>
          signal.addEventListener(
            "abort",
            () => {
              aborted.resolve();
              reject(new Error("cancelled"));
            },
            { once: true },
          ),
        );
      },
    });
    const origin = await listen(gateway);
    const headers = { authorization: "Bearer test" };
    const post = (path: string, body?: Buffer) =>
      fetch(origin + path, {
        method: "POST",
        headers,
        ...(body ? { body: Buffer.from(body) } : {}),
      });
    try {
      const routes: string[] = [];
      for (let i = 0; i < 4; i++)
        routes.push(
          "/v1/recordings/" + (await (await post("/v1/recordings")).text()).trim().split("\t")[1],
        );
      NodeAssert.equal((await post("/v1/recordings")).status, 429);
      const route = routes[0]!;
      await post(route + "/chunks?sequence=0", Buffer.alloc(2));
      await post(route + "/finish");
      await started.promise;
      NodeAssert.equal((await post("/v1/transcriptions", Buffer.alloc(2))).status, 429);
      await post(routes[1] + "/chunks?sequence=0", Buffer.alloc(2));
      NodeAssert.equal((await post(routes[1] + "/finish")).status, 429);
      NodeAssert.equal(await (await post(route + "/cancel")).text(), "OK\n");
      await aborted.promise;
      await NodeAssert.rejects(NodeFSP.stat(NodePath.dirname(pcmPath)), { code: "ENOENT" });
      NodeAssert.equal(
        await (await post("/v1/transcriptions", Buffer.alloc(2))).text(),
        "TEXT\tlegacy\n",
      );
    } finally {
      await close(gateway);
    }
  },
);

NodeTest.test("oversized or failed transcripts return errors instead of partial text", async () => {
  for (const text of ["é".repeat(30_001), "\\".repeat(40_000), null]) {
    const gateway = createGateway({
      serverUrl: "http://127.0.0.1:1",
      t3Token: "unused",
      token: "test",
      transcribeFile: async () => {
        if (text === null) throw new Error("failed");
        return text;
      },
    });
    const origin = await listen(gateway);
    const headers = { authorization: "Bearer test" };
    const post = (path: string, body?: Buffer) =>
      fetch(origin + path, {
        method: "POST",
        headers,
        ...(body ? { body: Buffer.from(body) } : {}),
      });
    try {
      const route =
        "/v1/recordings/" + (await (await post("/v1/recordings")).text()).trim().split("\t")[1];
      await post(route + "/chunks?sequence=0", Buffer.alloc(2));
      NodeAssert.equal(await (await post(route + "/finish")).text(), "PENDING\n");
      const response = await fetch(origin + route, { headers });
      NodeAssert.equal(response.status, 422);
      NodeAssert.match(await response.text(), /^ERROR\t/);
    } finally {
      await close(gateway);
    }
  }
});

NodeTest.test("recording expiry follows idle time, not elapsed recording time", (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const recordings = new Recordings(
    async () => "text",
    () => true,
    () => {},
    (text) => text,
  );
  try {
    const id = recordings.create().trim().split("\t")[1]!;
    for (let sequence = 0; sequence < 6; sequence++) {
      context.mock.timers.tick(4 * 60_000);
      NodeAssert.equal(
        recordings.append(id, String(sequence), Buffer.alloc(8192)),
        `OK\t${sequence + 1}\n`,
      );
    }
    NodeAssert.equal(recordings.poll(id), "PENDING\n");
    context.mock.timers.tick(5 * 60_000);
    NodeAssert.throws(() => recordings.poll(id), /Recording not found/);
  } finally {
    recordings.close();
  }
});

NodeTest.test("closing gateway aborts active transcription and removes its audio", async () => {
  const started = Promise.withResolvers<void>();
  const aborted = Promise.withResolvers<void>();
  let pcmPath = "";
  const gateway = createGateway({
    serverUrl: "http://127.0.0.1:1",
    t3Token: "unused",
    token: "test",
    transcribeFile: async (path, signal) => {
      pcmPath = path;
      started.resolve();
      return new Promise<string>((_resolve, reject) =>
        signal.addEventListener(
          "abort",
          () => {
            aborted.resolve();
            reject(new Error("cancelled"));
          },
          { once: true },
        ),
      );
    },
  });
  const origin = await listen(gateway);
  const headers = { authorization: "Bearer test" };
  try {
    const route =
      "/v1/recordings/" +
      (await (await fetch(origin + "/v1/recordings", { method: "POST", headers })).text())
        .trim()
        .split("\t")[1];
    await fetch(origin + route + "/chunks?sequence=0", {
      method: "POST",
      headers,
      body: Buffer.alloc(2),
    });
    await fetch(origin + route + "/finish", { method: "POST", headers });
    await started.promise;
    await close(gateway);
    await aborted.promise;
    await NodeAssert.rejects(NodeFSP.stat(NodePath.dirname(pcmPath)), { code: "ENOENT" });
  } finally {
    if (gateway.listening) await close(gateway);
  }
});
