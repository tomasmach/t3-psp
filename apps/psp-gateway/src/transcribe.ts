import * as NodeChildProcess from "node:child_process";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { HttpError } from "./recordings.ts";

// Convert fixed-size windows, including the interpolation lookahead sample.
export async function pcmFileToWav(source: string, destination: string, signal: AbortSignal) {
  const input = await NodeFSP.open(source, "r");
  try {
    const size = (await input.stat()).size;
    if (!size || size % 2) throw new Error("Invalid PCM16 audio");
    const sourceSamples = size / 2;
    const samples = Math.floor((sourceSamples * 16000) / 11025);
    if (samples * 2 + 36 > 0xffffffff)
      throw new HttpError(413, "Recording exceeds the WAV file size limit");
    const header = pcmToWav(Buffer.alloc(2)).subarray(0, 44);
    header.writeUInt32LE(samples * 2 + 36, 4);
    header.writeUInt32LE(samples * 2, 40);
    const output = await NodeFSP.open(destination, "wx", 0o600);
    try {
      await output.writeFile(header);
      for (let start = 0; start < samples; start += 4096) {
        signal.throwIfAborted();
        const count = Math.min(4096, samples - start);
        const first = Math.floor((start * 11025) / 16000);
        const last = Math.min(
          sourceSamples - 1,
          Math.floor(((start + count - 1) * 11025) / 16000) + 1,
        );
        const sourceBytes = Buffer.alloc((last - first + 1) * 2);
        let read = 0;
        while (read < sourceBytes.length) {
          const { bytesRead } = await input.read(
            sourceBytes,
            read,
            sourceBytes.length - read,
            first * 2 + read,
          );
          if (!bytesRead) throw new Error("Recording ended unexpectedly");
          read += bytesRead;
        }
        const converted = Buffer.alloc(count * 2);
        for (let offset = 0; offset < count; offset++) {
          const position = ((start + offset) * 11025) / 16000;
          const index = Math.floor(position);
          const left = sourceBytes.readInt16LE((index - first) * 2);
          const right = sourceBytes.readInt16LE(
            (Math.min(index + 1, sourceSamples - 1) - first) * 2,
          );
          converted.writeInt16LE(
            Math.round(left + (right - left) * (position - index)),
            offset * 2,
          );
        }
        await output.writeFile(converted);
      }
    } finally {
      await output.close();
    }
    return sourceSamples / 11025;
  } finally {
    await input.close();
  }
}

export function whisperFileTranscriber(binary: string, model: string, language = "auto") {
  return async (pcmPath: string, signal: AbortSignal) => {
    const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "t3-psp-whisper-"));
    try {
      const input = NodePath.join(directory, "audio.wav");
      const output = NodePath.join(directory, "transcript");
      const seconds = await pcmFileToWav(pcmPath, input, signal);
      signal.throwIfAborted();
      await new Promise<void>((resolve, reject) => {
        const child = NodeChildProcess.spawn(
          binary,
          ["-m", model, "-f", input, "-l", language, "-otxt", "-of", output, "-nt"],
          { stdio: "ignore", signal, killSignal: "SIGKILL" },
        );
        const timeout = setTimeout(
          () => child.kill("SIGKILL"),
          Math.min(2_147_483_647, Math.max(60_000, seconds * 20_000)),
        );
        let spawnError: Error | undefined;
        child.once("error", (error) => {
          spawnError = error;
        });
        // close means the child has exited, so cancellation cannot free its CPU slot early.
        child.once("close", (code) => {
          clearTimeout(timeout);
          if (spawnError) reject(spawnError);
          else if (code !== 0) reject(new Error("Whisper transcription failed"));
          else resolve();
        });
      });
      signal.throwIfAborted();
      if ((await NodeFSP.stat(`${output}.txt`)).size > 60_000)
        throw new HttpError(413, "Transcript exceeds 60000 bytes; record a shorter passage");
      return await NodeFSP.readFile(`${output}.txt`, "utf8");
    } finally {
      await NodeFSP.rm(directory, { recursive: true, force: true });
    }
  };
}

// whisper.cpp expects 16 kHz WAV; PSP microphone capture is 11025 Hz PCM16.
export function pcmToWav(pcm: Buffer) {
  const sourceSamples = pcm.length / 2;
  if (!Number.isInteger(sourceSamples) || sourceSamples === 0)
    throw new Error("Invalid PCM16 audio");
  const samples = Math.floor((sourceSamples * 16000) / 11025);
  const wav = Buffer.alloc(44 + samples * 2);
  wav.write("RIFF", 0);
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(16000, 24);
  wav.writeUInt32LE(32000, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(samples * 2, 40);
  for (let sample = 0; sample < samples; sample++) {
    const position = (sample * 11025) / 16000;
    const index = Math.floor(position);
    const left = pcm.readInt16LE(index * 2);
    const right = pcm.readInt16LE(Math.min(index + 1, sourceSamples - 1) * 2);
    wav.writeInt16LE(Math.round(left + (right - left) * (position - index)), 44 + sample * 2);
  }
  return wav;
}

export function whisperTranscriber(binary: string, model: string, language = "auto") {
  const transcribeFile = whisperFileTranscriber(binary, model, language);
  return async (pcm: Buffer, signal: AbortSignal) => {
    signal.throwIfAborted();
    const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "t3-psp-voice-"));
    try {
      const input = NodePath.join(directory, "audio.pcm");
      await NodeFSP.writeFile(input, pcm, { mode: 0o600 });
      return await transcribeFile(input, signal);
    } finally {
      await NodeFSP.rm(directory, { recursive: true, force: true });
    }
  };
}
