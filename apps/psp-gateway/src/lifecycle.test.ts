import * as NodeAssert from "node:assert/strict";
import * as NodeFSP from "node:fs/promises";
import * as NodeTest from "node:test";
import { createGateway } from "./gateway.ts";
import { Recordings } from "./recordings.ts";

NodeTest.test("closing a legacy HTTP request aborts its transcription", async () => {
  const started = Promise.withResolvers<void>();
  const aborted = Promise.withResolvers<void>();
  const gateway = createGateway({
    serverUrl: "http://127.0.0.1:1",
    t3Token: "unused",
    token: "test",
    transcribe: async (_pcm, signal) => {
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
  await new Promise<void>((resolve) => gateway.listen(0, "127.0.0.1", resolve));
  const address = gateway.address();
  NodeAssert.ok(address && typeof address !== "string");
  const request = fetch(`http://127.0.0.1:${address.port}/v1/transcriptions`, {
    method: "POST",
    headers: { authorization: "Bearer test" },
    body: Buffer.alloc(2),
  }).catch(() => undefined);
  try {
    await started.promise;
    const closed = new Promise<void>((resolve, reject) =>
      gateway.close((error) => (error ? reject(error) : resolve())),
    );
    gateway.closeAllConnections();
    await closed;
    await aborted.promise;
    await request;
  } finally {
    if (gateway.listening) {
      gateway.closeAllConnections();
      await new Promise<void>((resolve) => gateway.close(() => resolve()));
    }
  }
});

NodeTest.test("cancelling a recording leaves file cleanup to its active reader", async () => {
  const started = Promise.withResolvers<string>();
  const finish = Promise.withResolvers<void>();
  const released = Promise.withResolvers<void>();
  let aborted = false;
  const recordings = new Recordings(
    async (path, signal) => {
      started.resolve(path);
      signal.addEventListener(
        "abort",
        () => {
          aborted = true;
        },
        { once: true },
      );
      await finish.promise;
      NodeAssert.equal((await NodeFSP.readFile(path)).length, 2);
      signal.throwIfAborted();
      return "unreachable";
    },
    () => true,
    () => released.resolve(),
    (text) => `TEXT\t${text}\n`,
  );
  const id = recordings.create().trim().split("\t")[1]!;
  try {
    recordings.append(id, "0", Buffer.alloc(2));
    recordings.finish(id);
    const path = await started.promise;
    NodeAssert.equal(recordings.cancel(id), "OK\n");
    NodeAssert.equal(aborted, true);
    NodeAssert.equal((await NodeFSP.stat(path)).size, 2);
    finish.resolve();
    await released.promise;
    await NodeAssert.rejects(NodeFSP.stat(path), { code: "ENOENT" });
  } finally {
    finish.resolve();
    recordings.close();
  }
});
