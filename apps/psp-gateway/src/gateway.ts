import * as NodeHttp from "node:http";
import * as NodeCrypto from "node:crypto";
import {
  ClientOrchestrationCommand,
  DispatchResult,
  OrchestrationShellSnapshot,
  OrchestrationThreadDetailSnapshot,
  type OrchestrationThread,
  type OrchestrationThreadShell,
} from "@t3tools/contracts";
import * as Schema from "effect/Schema";
import { HttpError, Recordings, type FileTranscriber } from "./recordings.ts";

const decodeShell = Schema.decodeUnknownSync(OrchestrationShellSnapshot);
const decodeDetail = Schema.decodeUnknownSync(OrchestrationThreadDetailSnapshot);
const decodeCommand = Schema.decodeUnknownSync(ClientOrchestrationCommand);
const decodeDispatch = Schema.decodeUnknownSync(DispatchResult);

export interface GatewayOptions {
  serverUrl: string;
  t3Token: string;
  token: string;
  transcribe?: (pcm: Buffer, signal: AbortSignal) => Promise<string>;
  transcribeFile?: FileTranscriber;
}

export const escapeField = (value: string) =>
  value
    .replaceAll("\\", "\\\\")
    .replaceAll("\t", "\\t")
    .replaceAll("\r", "\\r")
    .replaceAll("\n", "\\n");
const line = (...fields: string[]) => fields.map(escapeField).join("\t") + "\n";

const status = (thread: OrchestrationThreadShell | OrchestrationThread) => {
  if ("hasPendingApprovals" in thread && (thread.hasPendingApprovals || thread.hasPendingUserInput))
    return "waiting";
  if (thread.session?.status === "starting" || thread.session?.status === "running")
    return "running";
  if (
    thread.session?.status === "error" &&
    (!thread.latestTurn || thread.session.updatedAt >= thread.latestTurn.requestedAt)
  )
    return "error";
  if (thread.latestTurn?.state === "running") return "running";
  if ("backgroundLiveness" in thread && thread.backgroundLiveness) return "running";
  if (thread.latestTurn?.state === "error") return "error";
  return "idle";
};
const threadLine = (
  thread: OrchestrationThreadShell | OrchestrationThread,
  state = status(thread),
  projectName?: string,
) =>
  line(
    "THREAD",
    thread.id,
    state,
    thread.title.slice(0, 160),
    ...(projectName === undefined ? [] : [projectName]),
  );

async function body(request: NodeHttp.IncomingMessage, limit: number) {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.from(chunk);
    size += buffer.length;
    if (size > limit) throw new HttpError(413, "Request too large");
    chunks.push(buffer);
  }
  return Buffer.concat(chunks);
}

export function createGateway(options: GatewayOptions) {
  if (!options.token || !options.t3Token)
    throw new Error("Both T3_PSP_TOKEN and T3_PSP_T3_TOKEN are required");
  const origin = new URL(options.serverUrl);
  if (!["http:", "https:"].includes(origin.protocol))
    throw new Error("T3_PSP_SERVER_URL must be HTTP or HTTPS");
  const authorization = Buffer.from(`Bearer ${options.token}`);
  const requestUpstream = async (path: string, command?: ClientOrchestrationCommand) => {
    const response = await fetch(new URL(path, origin), {
      headers: { authorization: `Bearer ${options.t3Token}`, "content-type": "application/json" },
      ...(command ? { method: "POST", body: JSON.stringify(command) } : {}),
      signal: AbortSignal.timeout(15_000),
      redirect: "error",
    });
    if (!response.ok)
      throw new HttpError(
        response.status === 404 ? 404 : 502,
        `T3 request failed (${response.status})`,
      );
    return response.json() as Promise<unknown>;
  };
  const shell = async () => decodeShell(await requestUpstream("/api/orchestration/shell"));
  // Cache only explicit request IDs. A repeated POST cannot start a second turn.
  const requests = new Map<string, { text: string; result: Promise<string> }>();
  let transcribing = false;
  const recordings = new Recordings(
    options.transcribeFile,
    () => {
      if (transcribing) return false;
      transcribing = true;
      return true;
    },
    () => {
      transcribing = false;
    },
    (text) => line("TEXT", text),
  );

  const server = NodeHttp.createServer(async (request, response) => {
    response.setHeader("content-type", "text/plain; charset=utf-8");
    response.setHeader("cache-control", "no-store");
    try {
      const supplied = Buffer.from(request.headers.authorization ?? "");
      if (
        supplied.length !== authorization.length ||
        !NodeCrypto.timingSafeEqual(supplied, authorization)
      )
        throw new HttpError(401, "Unauthorized");
      const url = new URL(request.url ?? "/", "http://gateway");
      // Older clients split THREAD into four fields, so project names are opt-in.
      const includeProjectNames = url.searchParams.get("projectNames") === "1";
      let output: string;
      const recordingRoute =
        /^\/v1\/recordings\/([a-zA-Z0-9_-]{1,100})(?:\/(chunks|finish|cancel))?$/.exec(
          url.pathname,
        );
      if (request.method === "POST" && url.pathname === "/v1/recordings") {
        await body(request, 0);
        output = recordings.create();
      } else if (recordingRoute?.[1]) {
        const id = recordingRoute[1];
        const action = recordingRoute[2];
        if (request.method === "GET" && !action) output = recordings.poll(id);
        else if (request.method === "POST" && action === "chunks")
          output = recordings.append(
            id,
            url.searchParams.get("sequence"),
            await body(request, 8192),
          );
        else if (request.method === "POST" && (action === "finish" || action === "cancel")) {
          await body(request, 0);
          output = action === "finish" ? recordings.finish(id) : recordings.cancel(id);
        } else throw new HttpError(405, "Method not allowed");
      } else if (request.method === "GET" && url.pathname === "/v1/threads") {
        const snapshot = await shell();
        output = snapshot.threads
          .filter((thread) => !thread.archivedAt)
          .toSorted((a, b) => b.updatedAt.localeCompare(a.updatedAt))
          .slice(0, 32)
          .map((thread) =>
            threadLine(
              thread,
              status(thread),
              includeProjectNames
                ? (snapshot.projects.find((project) => project.id === thread.projectId)?.title ??
                    "")
                : undefined,
            ),
          )
          .join("");
      } else if (request.method === "POST" && url.pathname === "/v1/transcriptions") {
        if (!options.transcribe) throw new HttpError(503, "Transcription is not configured");
        if (transcribing) throw new HttpError(429, "Transcription is busy");
        transcribing = true;
        const abort = new AbortController();
        const disconnect = () => {
          if (!response.writableFinished) abort.abort();
        };
        response.once("close", disconnect);
        try {
          const pcm = await body(request, 11025 * 2 * 30);
          if (!pcm.length || pcm.length % 2)
            throw new HttpError(400, "Expected PCM16 mono 11025 Hz audio");
          const text = (await options.transcribe(pcm, abort.signal)).trim();
          output = line("TEXT", text);
          if (Buffer.byteLength(text) > 60_000 || Buffer.byteLength(output) > 65_535)
            throw new HttpError(413, "Transcript is too large; record a shorter passage");
        } finally {
          response.off("close", disconnect);
          transcribing = false;
        }
      } else {
        const match = /^\/v1\/threads\/([^/]+)(?:\/(prompt|interrupt))?$/.exec(url.pathname);
        if (!match?.[1]) throw new HttpError(404, "Not found");
        const id = decodeURIComponent(match[1]);
        const action = match[2];
        if (request.method === "GET" && !action) {
          const snapshot = decodeDetail(
            await requestUpstream(
              `/api/orchestration/threads/${encodeURIComponent(id)}?turnLimit=4`,
            ),
          );
          const shellSnapshot = await shell();
          const summary = shellSnapshot.threads.find((thread) => thread.id === id);
          output = threadLine(
            snapshot.thread,
            summary ? status(summary) : status(snapshot.thread),
            includeProjectNames
              ? (shellSnapshot.projects.find((project) => project.id === snapshot.thread.projectId)
                  ?.title ?? "")
              : undefined,
          );
          for (const message of snapshot.thread.messages.slice(-8))
            output += line("MSG", message.role, message.text.slice(-3000));
          for (const activity of snapshot.thread.activities.slice(-5))
            output += line("ACT", activity.summary.slice(0, 200));
        } else if (request.method === "POST" && action) {
          const text = (await body(request, 60_000)).toString("utf8");
          if (action === "prompt" && !text.trim()) throw new HttpError(400, "Prompt is empty");
          const requestId = request.headers["x-request-id"];
          if (
            requestId !== undefined &&
            (typeof requestId !== "string" || !/^[a-zA-Z0-9_-]{1,100}$/.test(requestId))
          )
            throw new HttpError(400, "Invalid request ID");
          const key = requestId ? `${id}/${action}/${requestId}` : undefined;
          const previous = key ? requests.get(key) : undefined;
          if (previous && previous.text !== text)
            throw new HttpError(409, "Request ID already used for a different body");
          if (previous) output = await previous.result;
          else {
            const dispatch = async () => {
              const thread = (await shell()).threads.find((thread) => thread.id === id);
              if (!thread) throw new HttpError(404, "Thread not found");
              const base = {
                commandId: key
                  ? `psp-${NodeCrypto.createHash("sha256").update(key).digest("hex")}`
                  : NodeCrypto.randomUUID(),
                threadId: id,
                createdAt: new Date().toISOString(),
              };
              const command = decodeCommand(
                action === "prompt"
                  ? {
                      ...base,
                      type: "thread.turn.start",
                      message: {
                        messageId: NodeCrypto.randomUUID(),
                        role: "user",
                        text,
                        attachments: [],
                      },
                      modelSelection: thread.modelSelection,
                      runtimeMode: thread.runtimeMode,
                      interactionMode: thread.interactionMode,
                    }
                  : { ...base, type: "thread.turn.interrupt" },
              );
              const result = decodeDispatch(
                await requestUpstream("/api/orchestration/dispatch", command),
              );
              return line("OK", String(result.sequence));
            };
            const result = dispatch();
            if (key) {
              if (requests.size >= 256) requests.delete(requests.keys().next().value!);
              requests.set(key, { text, result });
            }
            output = await result;
          }
        } else throw new HttpError(405, "Method not allowed");
      }
      // A hard byte ceiling protects the PSP receive buffer, including escaped UTF-8.
      if (Buffer.byteLength(output) > 65535) {
        let bounded = "";
        for (const record of output.split("\n")) {
          if (Buffer.byteLength(bounded + record + "\n") > 65535) break;
          bounded += record + "\n";
        }
        output = bounded;
      }
      response.statusCode = 200;
      response.end(output);
    } catch (error) {
      response.statusCode = error instanceof HttpError ? error.status : 502;
      response.end(
        line("ERROR", error instanceof HttpError ? error.message : "Gateway request failed"),
      );
    }
  });
  server.on("close", () => recordings.close());
  return server;
}
