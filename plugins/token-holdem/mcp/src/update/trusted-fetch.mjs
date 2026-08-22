import { Readable } from "node:stream";

const FETCH_TIMEOUT_MS = 30_000;
const TRUSTED_DOWNLOAD_HOSTS = new Set([
  "github.com",
  "objects.githubusercontent.com",
  "release-assets.githubusercontent.com",
]);

export async function fetchTrusted(initialUrl, fetchImpl) {
  let currentUrl = new URL(initialUrl);
  for (let redirectCount = 0; redirectCount <= 5; redirectCount += 1) {
    assertTrustedUrl(currentUrl);
    const response = await fetchImpl(currentUrl, {
      redirect: "manual",
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
      headers: { Accept: "application/json, application/octet-stream;q=0.9" },
    });
    if (![301, 302, 303, 307, 308].includes(response.status)) return response;
    const location = response.headers.get("location");
    if (location === null) throw new Error("The update server returned an invalid redirect");
    currentUrl = new URL(location, currentUrl);
  }
  throw new Error("The update server returned too many redirects");
}

export function ensureSuccessfulResponse(response, label) {
  if (!response.ok) {
    throw new Error(`The ${label} request failed with HTTP ${String(response.status)}`);
  }
}

export async function readResponseBytes(response, maximumBytes) {
  if (response.body === null) throw new Error("The update response has no body");
  const chunks = [];
  let total = 0;
  for await (const rawChunk of Readable.fromWeb(response.body)) {
    const chunk = Buffer.from(rawChunk);
    total += chunk.byteLength;
    if (total > maximumBytes) throw new Error("The update response is too large");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks, total);
}

function assertTrustedUrl(url) {
  if (url.protocol !== "https:" || !TRUSTED_DOWNLOAD_HOSTS.has(url.hostname)) {
    throw new Error("The update request was redirected to an untrusted origin");
  }
  if (url.username !== "" || url.password !== "") {
    throw new Error("The update URL must not contain credentials");
  }
}
