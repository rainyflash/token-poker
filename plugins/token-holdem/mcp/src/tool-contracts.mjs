import { z } from "zod";

const safeInteger = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
const boundedText = (maximum) => z.string().trim().min(1).max(maximum);
const address = boundedText(2_048);

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
      recovery_secret: boundedText(256),
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("create_identity"),
      recovery_secret: boundedText(256),
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("restore_identity"),
      recovery_envelope: boundedText(65_536),
      recovery_secret: boundedText(256),
      device_label: boundedText(96),
    })
    .strict(),
  z
    .object({
      type: z.literal("restore_remote_identity"),
      recovery_secret: boundedText(256),
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
      action: z.enum(["fold", "check", "call", "raise"]),
      amount: safeInteger.optional(),
    })
    .strict(),
  z.object({ type: z.literal("leave_table") }).strict(),
  z.object({ type: z.literal("sync_statistics") }).strict(),
];

export const hostCommandSchema = z.discriminatedUnion("type", commandVariants);

export const commandToolSchema = z
  .object({ command: hostCommandSchema })
  .strict();

export const pollToolSchema = z
  .object({
    after_sequence: safeInteger.default(0),
    wait_ms: z.number().int().min(0).max(25_000).default(20_000),
  })
  .strict();

export const refreshOfficialUsageToolSchema = z.object({}).strict();
