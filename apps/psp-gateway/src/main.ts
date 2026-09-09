import { createGateway } from "./gateway.ts";
import { whisperTranscriber, whisperFileTranscriber } from "./transcribe.ts";
import {
  AuthAccessTokenResult,
  AuthAccessTokenType,
  AuthEnvironmentBootstrapTokenType,
  AuthTokenExchangeGrantType,
} from "@t3tools/contracts";
import * as Schema from "effect/Schema";

const required = (name: string) => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
};
const port = Number(process.env.T3_PSP_PORT ?? "8787");
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid T3_PSP_PORT");
const host = process.env.T3_PSP_HOST ?? "127.0.0.1";
const model = process.env.T3_PSP_WHISPER_MODEL;
const serverUrl = required("T3_PSP_SERVER_URL");
const token = required("T3_PSP_TOKEN");
let t3Token = process.env.T3_PSP_T3_TOKEN;
if (!t3Token) {
  const response = await fetch(new URL("/oauth/token", serverUrl), {
    method: "POST",
    redirect: "error",
    signal: AbortSignal.timeout(15_000),
    body: new URLSearchParams({
      grant_type: AuthTokenExchangeGrantType,
      subject_token: required("T3_PSP_PAIRING_TOKEN"),
      subject_token_type: AuthEnvironmentBootstrapTokenType,
      requested_token_type: AuthAccessTokenType,
      client_label: "PSP gateway",
    }),
  });
  if (!response.ok) throw new Error(`T3 pairing failed (${response.status})`);
  const session = Schema.decodeUnknownSync(AuthAccessTokenResult)(await response.json());
  if (session.token_type !== "Bearer") throw new Error("T3 pairing did not issue a bearer token");
  t3Token = session.access_token;
  console.log(
    `T3 paired; credential expires in ${session.expires_in} seconds. Restart with a fresh pairing token when it expires.`,
  );
}
const fileTranscriber = model
  ? whisperFileTranscriber(
      process.env.T3_PSP_WHISPER_BIN ?? "whisper-cli",
      model,
      process.env.T3_PSP_WHISPER_LANGUAGE ?? "auto",
    )
  : undefined;
const server = createGateway({
  serverUrl,
  t3Token,
  token,
  ...(model
    ? {
        transcribe: whisperTranscriber(
          process.env.T3_PSP_WHISPER_BIN ?? "whisper-cli",
          model,
          process.env.T3_PSP_WHISPER_LANGUAGE ?? "auto",
        ),
        transcribeFile: fileTranscriber!,
      }
    : {}),
});
server.requestTimeout = 75_000;
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    server.close();
    server.closeAllConnections();
  });
}
server.listen(port, host, () => console.log(`PSP gateway listening at http://${host}:${port}`));
