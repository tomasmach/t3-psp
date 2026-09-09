import * as NodeCrypto from "node:crypto";
import * as NodeFS from "node:fs";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";

export class HttpError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

export type FileTranscriber = (path: string, signal: AbortSignal) => Promise<string>;
type Recording = {
  directory: string;
  path: string;
  sequence: number;
  lastHash?: string;
  partial: boolean;
  state: "recording" | "pending" | "done" | "error";
  output?: string;
  timer?: NodeJS.Timeout;
  abort: AbortController;
};

// Only the most recent chunk hash is retained; audio is never accumulated in RAM.
export class Recordings {
  private readonly sessions = new Map<string, Recording>();
  private readonly transcribe: FileTranscriber | undefined;
  private readonly acquire: () => boolean;
  private readonly release: () => void;
  private readonly textLine: (text: string) => string;
  constructor(
    transcribe: FileTranscriber | undefined,
    acquire: () => boolean,
    release: () => void,
    textLine: (text: string) => string,
  ) {
    this.transcribe = transcribe;
    this.acquire = acquire;
    this.release = release;
    this.textLine = textLine;
  }

  private refresh(id: string, session: Recording) {
    clearTimeout(session.timer);
    session.timer = setTimeout(() => this.cancel(id), 5 * 60_000);
    session.timer.unref();
  }

  create() {
    if (!this.transcribe) throw new HttpError(503, "Transcription is not configured");
    if (this.sessions.size >= 4) throw new HttpError(429, "Too many recordings");
    const id = NodeCrypto.randomUUID();
    const directory = NodeFS.mkdtempSync(NodePath.join(NodeOS.tmpdir(), "t3-psp-recording-"));
    const path = NodePath.join(directory, "audio.pcm");
    try {
      NodeFS.writeFileSync(path, Buffer.alloc(0), { mode: 0o600 });
    } catch (error) {
      NodeFS.rmSync(directory, { recursive: true, force: true });
      throw error;
    }
    const session: Recording = {
      directory,
      path,
      sequence: 0,
      partial: false,
      state: "recording",
      abort: new AbortController(),
    };
    this.sessions.set(id, session);
    this.refresh(id, session);
    return `RECORDING\t${id}\n`;
  }

  private get(id: string) {
    const session = this.sessions.get(id);
    if (!session) throw new HttpError(404, "Recording not found");
    return session;
  }

  append(id: string, sequence: string | null, pcm: Buffer) {
    const session = this.get(id);
    if (session.state !== "recording") throw new HttpError(409, "Recording is already finished");
    if (!sequence || !/^(0|[1-9][0-9]*)$/.test(sequence) || !Number.isSafeInteger(Number(sequence)))
      throw new HttpError(400, "Invalid chunk sequence");
    if (!pcm.length || pcm.length > 8192 || pcm.length % 2)
      throw new HttpError(400, "Expected 1 to 4096 PCM16 samples");
    const hash = NodeCrypto.createHash("sha256").update(pcm).digest("hex");
    const index = Number(sequence);
    if (index === session.sequence - 1 && hash === session.lastHash) {
      this.refresh(id, session);
      return `OK\t${session.sequence}\n`;
    }
    if (index !== session.sequence || session.partial)
      throw new HttpError(409, "Chunk is out of order or follows a final partial chunk");
    try {
      NodeFS.appendFileSync(session.path, pcm);
    } catch (error) {
      this.cancel(id);
      throw error;
    }
    session.sequence++;
    session.lastHash = hash;
    session.partial = pcm.length < 8192;
    this.refresh(id, session);
    return `OK\t${session.sequence}\n`;
  }

  finish(id: string) {
    const session = this.get(id);
    if (session.state !== "recording") return "PENDING\n";
    if (!session.sequence) throw new HttpError(400, "Recording is empty");
    if (!this.acquire()) throw new HttpError(429, "Transcription is busy");
    session.state = "pending";
    clearTimeout(session.timer);
    void this.run(id, session);
    return "PENDING\n";
  }

  private async run(id: string, session: Recording) {
    try {
      const text = (await this.transcribe!(session.path, session.abort.signal)).trim();
      const output = this.textLine(text);
      if (Buffer.byteLength(text) > 60_000 || Buffer.byteLength(output) > 65_535)
        throw new HttpError(413, "Transcript is too large; record a shorter passage");
      session.output = output;
      session.state = "done";
    } catch (error) {
      session.output = error instanceof HttpError ? error.message : "Transcription failed";
      session.state = "error";
    } finally {
      this.release();
      NodeFS.rmSync(session.directory, { recursive: true, force: true });
      if (this.sessions.get(id) === session) this.refresh(id, session);
    }
  }

  poll(id: string) {
    const session = this.get(id);
    if (session.state === "error") throw new HttpError(422, session.output!);
    return session.state === "done" ? session.output! : "PENDING\n";
  }

  cancel(id: string) {
    const session = this.sessions.get(id);
    if (session) {
      this.sessions.delete(id);
      clearTimeout(session.timer);
      session.abort.abort();
      // The transcription task owns any open file handles. Let its finally
      // remove the files after cancellation has stopped conversion/the child.
      if (session.state !== "pending")
        NodeFS.rmSync(session.directory, { recursive: true, force: true });
    }
    return "OK\n";
  }

  close() {
    for (const id of this.sessions.keys()) this.cancel(id);
  }
}
