import { createInterface } from "node:readline";

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
const lifetimeTokens = Number(process.env.TOKEN_HOLDEM_TEST_LIFETIME_TOKENS ?? "35500000000");
const mode = process.env.TOKEN_HOLDEM_TEST_APP_SERVER_MODE ?? "ok";

lines.on("line", (line) => {
  const request = JSON.parse(line);
  switch (request.method) {
    case "initialize":
      respond(request.id, { userAgent: "mock-codex-app-server" });
      break;
    case "initialized":
      break;
    case "account/read":
      respond(request.id, {
        account: { type: "chatgpt", email: "Player@Example.com", planType: "pro" },
        requiresOpenaiAuth: true,
      });
      break;
    case "account/usage/read":
      if (mode === "unsupported") {
        process.stdout.write(
          `${JSON.stringify({
            id: request.id,
            error: {
              code: -32600,
              message:
                "Invalid request: unknown variant `account/usage/read`, expected account/read",
            },
          })}\n`,
        );
      } else {
        respond(request.id, {
          summary: {
            lifetimeTokens,
            peakDailyTokens: 1_760_000_000,
            longestRunningTurnSec: 6_336,
            currentStreakDays: 114,
            longestStreakDays: 114,
          },
          dailyUsageBuckets: [{ startDate: "2026-08-21", tokens: 1_000_000 }],
        });
      }
      break;
    default:
      process.stdout.write(
        `${JSON.stringify({
          id: request.id,
          error: { code: -32601, message: `Method not found: ${String(request.method)}` },
        })}\n`,
      );
  }
});

function respond(id, result) {
  process.stdout.write(`${JSON.stringify({ id, result })}\n`);
}
