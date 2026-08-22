import { randomUUID } from "node:crypto";
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseCommunityDirectory } from "./community-network.mjs";

const options = parseArguments(process.argv.slice(2));
const directoryPath = resolve(options.directory);
const current = JSON.parse(await readFile(directoryPath, "utf8"));
const protocol = /^\d{1,3}(?:\.\d{1,3}){3}$/u.test(options.host) ? "ip4" : "dns4";
const base = `/${protocol}/${options.host}`;
const addresses = [
  `${base}/tcp/${String(options.port)}/p2p/${options.peerId}`,
  `${base}/udp/${String(options.port)}/quic-v1/p2p/${options.peerId}`,
];
const existingNodes = Array.isArray(current.nodes)
  ? current.nodes.filter((node) => node?.name !== options.name)
  : [];
const next = {
  schema_version: 1,
  namespace: "token-holdem/v1",
  nodes: [
    {
      name: options.name,
      roles: ["rendezvous", "relay", "archive"],
      addresses,
    },
    ...existingNodes,
  ],
};
parseCommunityDirectory(next, directoryPath);

const temporaryPath = `${directoryPath}.${String(process.pid)}.${randomUUID()}.tmp`;
await writeFile(temporaryPath, `${JSON.stringify(next, null, 2)}\n`, {
  encoding: "utf8",
  flag: "wx",
});
try {
  await rename(temporaryPath, directoryPath);
} catch (error) {
  await rm(temporaryPath, { force: true });
  throw error;
}

process.stdout.write(
  `已写入 ${directoryPath}\n节点 ${options.name}\n${addresses.join("\n")}\n`,
);

function parseArguments(argumentsList) {
  const result = {
    host: "",
    peerId: "",
    port: 4001,
    name: "primary-vps",
    directory: "config/community-nodes.json",
  };
  for (let index = 0; index < argumentsList.length; index += 1) {
    const argument = argumentsList[index];
    const value = argumentsList[index + 1];
    if (argument === "--host" && value !== undefined) result.host = value;
    else if (argument === "--peer-id" && value !== undefined) result.peerId = value;
    else if (argument === "--name" && value !== undefined) result.name = value;
    else if (argument === "--directory" && value !== undefined) result.directory = value;
    else if (argument === "--port" && value !== undefined) result.port = Number.parseInt(value, 10);
    else throw new Error(`未知或缺值参数：${String(argument)}`);
    index += 1;
  }
  if (!/^(?:[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?|\d{1,3}(?:\.\d{1,3}){3})$/iu.test(result.host)) {
    throw new Error("--host 必须是 DNS 名称或 IPv4 地址");
  }
  if (!/^12D3KooW[1-9A-HJ-NP-Za-km-z]{20,100}$/u.test(result.peerId)) {
    throw new Error("--peer-id 必须是 sidecar ready 事件输出的 Ed25519 PeerId");
  }
  if (!Number.isInteger(result.port) || result.port < 1 || result.port > 65_535) {
    throw new Error("--port 必须是 1 到 65535 之间的整数");
  }
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/u.test(result.name)) {
    throw new Error("--name 只能包含小写字母、数字和短横线");
  }
  return result;
}
