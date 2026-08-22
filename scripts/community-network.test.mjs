import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  loadVerifiedNodeCache,
  loadVolunteerSettings,
  mergeNetworkSources,
  parseCommunityDirectory,
  recordVerifiedNode,
  saveVolunteerConsent,
} from "./community-network.mjs";

const PEER = "12D3KooWCQrKJT9mKBdRS33rQaADSw2Y3aQTp7wGBDciPu61YPbC";
const ADDRESS = `/dns4/node.example/tcp/4001/p2p/${PEER}`;

test("社区目录按角色合并并保持显式配置优先", () => {
  const directory = parseCommunityDirectory({
    schema_version: 1,
    namespace: "token-holdem/v1",
    nodes: [{ name: "首个节点", roles: ["rendezvous", "relay"], addresses: [ADDRESS] }],
  });
  const merged = mergeNetworkSources({
    explicitRendezvous: [`/dns4/explicit.example/tcp/4001/p2p/${PEER}`],
    directory,
    cache: [],
  });
  assert.equal(merged.rendezvous.length, 2);
  assert.match(merged.rendezvous[0], /explicit\.example/u);
  assert.deepEqual(merged.relays, [ADDRESS]);
  assert.deepEqual(merged.archives, []);
});

test("目录拒绝把私网地址发布成社区入口", () => {
  assert.throws(
    () =>
      parseCommunityDirectory({
        schema_version: 1,
        namespace: "token-holdem/v1",
        nodes: [
          {
            name: "错误节点",
            roles: ["relay"],
            addresses: [`/ip4/192.168.1.2/tcp/4001/p2p/${PEER}`],
          },
        ],
      }),
    /节点无效/u,
  );
});

test("授权和成功缓存使用有界 JSON 持久化", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-holdem-network-"));
  const settingsPath = join(directory, "settings.json");
  const cachePath = join(directory, "cache.json");
  try {
    assert.equal((await loadVolunteerSettings(settingsPath)).consent, "undecided");
    await saveVolunteerConsent(settingsPath, "granted", 1_000);
    assert.equal((await loadVolunteerSettings(settingsPath)).consent, "granted");
    await saveVolunteerConsent(settingsPath, "declined", 1_500);
    assert.equal((await loadVolunteerSettings(settingsPath)).consent, "declined");
    await recordVerifiedNode(
      cachePath,
      { peerId: PEER, address: ADDRESS, role: "relay" },
      2_000,
    );
    await recordVerifiedNode(
      cachePath,
      { peerId: PEER, address: ADDRESS, role: "rendezvous" },
      3_000,
    );
    const cache = await loadVerifiedNodeCache(cachePath, 3_000);
    assert.equal(cache.length, 1);
    assert.deepEqual(cache[0].roles, ["rendezvous", "relay"]);
    assert.match(await readFile(settingsPath, "utf8"), /"volunteer_consent": "declined"/u);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("多个对话并发更新社区节点缓存不会互相覆盖", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-holdem-cache-concurrent-"));
  const cachePath = join(directory, "cache.json");
  const observedAt = Date.now();
  try {
    await Promise.all(
      Array.from({ length: 16 }, (_, index) => {
        const peerId = `concurrent-peer-${String(index)}`;
        return recordVerifiedNode(
          cachePath,
          {
            peerId,
            address: `/dns4/concurrent-${String(index)}.example/tcp/4001/p2p/${peerId}`,
            role: "rendezvous",
          },
          observedAt + index,
        );
      }),
    );
    const cache = await loadVerifiedNodeCache(cachePath, observedAt + 16);
    assert.equal(cache.length, 16);
    assert.equal(new Set(cache.map((node) => node.peerId)).size, 16);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("成功缓存只保留最近三十二个节点", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-holdem-cache-cap-"));
  const cachePath = join(directory, "cache.json");
  try {
    for (let index = 0; index < 35; index += 1) {
      const peerId = `peer-${String(index)}`;
      await recordVerifiedNode(
        cachePath,
        {
          peerId,
          address: `/dns4/node-${String(index)}.example/tcp/4001/p2p/${peerId}`,
          role: "rendezvous",
        },
        index + 1,
      );
    }
    const cache = await loadVerifiedNodeCache(cachePath, 35);
    assert.equal(cache.length, 32);
    assert.equal(cache[0].peerId, "peer-34");
    assert.equal(cache.at(-1)?.peerId, "peer-3");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("超过三十天的节点不会参与冷启动", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-holdem-cache-"));
  const cachePath = join(directory, "cache.json");
  try {
    await recordVerifiedNode(cachePath, { peerId: PEER, address: ADDRESS, role: "relay" }, 1);
    const afterThirtyOneDays = 31 * 24 * 60 * 60 * 1_000;
    assert.deepEqual(await loadVerifiedNodeCache(cachePath, afterThirtyOneDays), []);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
