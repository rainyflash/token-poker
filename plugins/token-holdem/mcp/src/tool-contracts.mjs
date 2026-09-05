import { z } from "zod";

const safeInteger = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
const boundedText = (maximum) => z.string().trim().min(1).max(maximum);
const address = boundedText(2_048);
const recoverySecret = z.string().min(12).max(256);
const requestId = z
  .string()
  .regex(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu);

const commandVariants = [
  z
    .object({
      type: z.literal("join_public_pool"),
      level_id: boundedText(64),
      buy_in: safeInteger,
    })
    .strict(),
  z.object({ type: z.literal("cancel_public_pool") }).strict(),
  z
    .object({ type: z.literal("set_volunteer_consent"), enabled: z.boolean() })
    .strict(),
  z
    .object({
      type: z.literal("create_friend_room"),
      level_id: boundedText(64),
      buy_in: safeInteger,
    })
    .strict(),
  z
    .object({
      type: z.literal("join_friend_room"),
      invite_code: boundedText(16_384),
      buy_in: safeInteger,
    })
    .strict(),
  z
    .object({
      type: z.literal("ensure_identity"),
      expected_account_fingerprint: boundedText(128),
      recovery_secret: recoverySecret,
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("create_identity"),
      expected_account_fingerprint: boundedText(128),
      recovery_secret: recoverySecret,
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("restore_identity"),
      expected_account_fingerprint: boundedText(128),
      recovery_envelope: boundedText(65_536),
      recovery_secret: recoverySecret,
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("restore_remote_identity"),
      expected_account_fingerprint: boundedText(128),
      recovery_secret: recoverySecret,
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("configure_archive_nodes"),
      addresses: z.array(address).max(16),
      minimum_confirmed_replicas: z.number().int().min(1).max(16),
    })
    .strict(),
  z.object({ type: z.literal("use_relay"), address }).strict(),
  z
    .object({
      type: z.literal("configure_discovery"),
      addresses: z.array(address).max(8),
      namespace: boundedText(64).optional(),
    })
    .strict(),
  z.object({ type: z.literal("add_external_address"), address }).strict(),
  z
    .object({
      type: z.literal("submit_action"),
      expected: z.object({
        table_id: boundedText(128), hand_number: safeInteger, sequence: safeInteger,
        public_state_hash: z.string().regex(/^[0-9a-f]{64}$/u),
      }).strict(),
      action: z.enum(["fold", "check", "call", "raise"]),
      amount: safeInteger.optional(),
    })
    .strict(),
  z.object({ type: z.literal("leave_table") }).strict(),
  z.object({ type: z.literal("sync_statistics") }).strict(),
];

export const hostCommandSchema = z.discriminatedUnion("type", commandVariants);

export const commandToolSchema = z
  .object({ request_id: requestId, command: hostCommandSchema })
  .strict();

export const pollToolSchema = z
  .object({
    after_sequence: safeInteger.default(0),
    wait_ms: z.number().int().min(0).max(25_000).default(20_000),
  })
  .strict();

export const refreshOfficialUsageToolSchema = z.object({}).strict();
