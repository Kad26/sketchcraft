// Shared rules for the 0.3.6 key-rotation bridge (docs/SIGNING.md, "Key rotation (0.3.6 bridge)").
// No dependencies: Node's own crypto reads minisign keys and signatures.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

export const BRIDGE_VERSION = "0.3.6";
export const BRIDGE_FILE = "tools/release/bridge-latest.json";
export const OLD_PUBKEY_FILE = "tools/release/old-pubkey.b64";
export const V2_ENDPOINT_NAME = "latest-v2.json";
export const PLATFORM_KEYS = ["darwin-aarch64", "darwin-x86_64", "windows-x86_64", "linux-x86_64"];

export class Refusal extends Error {}
export const refuse = (m) => { throw new Refusal(m); };

export const parseVersion = (s) => {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(s || "");
  if (!m) refuse(`"${s}" is not a plain x.y.z version`);
  return m.slice(1).map(Number);
};
export const cmpVersion = (a, b) => {
  const x = parseVersion(a), y = parseVersion(b);
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  return 0;
};
/** "bridge" for exactly 0.3.6, "post" for anything greater, a refusal for anything lower. */
export const modeFor = (version) => {
  const c = cmpVersion(version, BRIDGE_VERSION);
  if (c === 0) return "bridge";
  if (c > 0) return "post";
  return refuse(`version ${version} is older than the bridge (${BRIDGE_VERSION}); this workflow only releases ${BRIDGE_VERSION} and later`);
};

// ---- minisign (the format tauri signer writes and tauri-plugin-updater reads) ----
const b64 = (s) => Buffer.from(String(s).trim(), "base64");
const lines = (buf) => buf.toString("utf8").split("\n");
export function decodePubkey(pubB64) {
  const l = lines(b64(pubB64));
  const raw = b64(l[1] || "");
  if (raw.length !== 42 || raw.subarray(0, 2).toString() !== "Ed") refuse("not a minisign public key");
  return { keyId: raw.subarray(2, 10).toString("hex"), key: raw.subarray(10) };
}
export const keyIdOfPubkey = (pubB64) => decodePubkey(pubB64).keyId;
export function decodeSignature(sigB64) {
  const l = lines(b64(sigB64));
  const raw = b64(l[1] || "");
  if (raw.length !== 74) refuse("not a minisign signature");
  const alg = raw.subarray(0, 2).toString();
  if (alg !== "Ed" && alg !== "ED") refuse("unknown minisign algorithm");
  const tc = (l[2] || "").startsWith("trusted comment: ") ? l[2].slice("trusted comment: ".length) : null;
  return { alg, keyId: raw.subarray(2, 10).toString("hex"), sig: raw.subarray(10), trustedComment: tc, globalSig: b64(l[3] || "") };
}
export const keyIdOfSignature = (sigB64) => decodeSignature(sigB64).keyId;
const edKey = (raw) => crypto.createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw]), format: "der", type: "spki" });

/** Throws unless `sigB64` is a valid minisign signature of `data` by `pubB64`. Returns the signed version, if the signature carries one. */
export function verifyData(pubB64, data, sigB64) {
  const pub = decodePubkey(pubB64), s = decodeSignature(sigB64);
  if (s.keyId !== pub.keyId) refuse(`signed with key ${s.keyId}, expected ${pub.keyId}`);
  const k = edKey(pub.key);
  const msg = s.alg === "ED" ? crypto.createHash("blake2b512").update(data).digest() : data;
  if (!crypto.verify(null, msg, k, s.sig)) refuse("signature does not match the file");
  if (s.trustedComment === null || !crypto.verify(null, Buffer.concat([s.sig, Buffer.from(s.trustedComment)]), k, s.globalSig)) refuse("trusted comment is not covered by the signature");
  const v = s.trustedComment.split("\t").find((f) => f.startsWith("version:"));
  return v ? v.slice(8) : null;
}

// ---- repository checks ----
export const readJson = (f) => JSON.parse(fs.readFileSync(f, "utf8"));
export const readOldPubkey = (root) => fs.readFileSync(path.join(root, OLD_PUBKEY_FILE), "utf8").trim();

/** The app config for this release: version, mode, and the rules the config must meet in both modes. */
export function readConfig(root, repo) {
  const conf = readJson(path.join(root, "app/src-tauri/tauri.conf.json"));
  const version = conf.version, mode = modeFor(version);
  const up = conf.plugins?.updater || {};
  const pub = String(up.pubkey || "").trim();
  const old = readOldPubkey(root);
  if (keyIdOfPubkey(pub) === keyIdOfPubkey(old)) refuse("tauri.conf.json still has the OLD public key; installed apps would never learn the new one");
  const want = [`https://github.com/${repo}/releases/latest/download/${V2_ENDPOINT_NAME}`];
  if (JSON.stringify(up.endpoints) !== JSON.stringify(want)) refuse(`updater endpoints must be exactly ${want[0]}`);
  return { version, mode, pub, old };
}

/** The frozen 0.3.6 manifest must be 0.3.6's, point only at the v0.3.6 release, and be signed by the OLD key. */
export function validateBridge(m, repo, oldPubB64) {
  if (!m || typeof m !== "object") refuse("bridge manifest is not a JSON object");
  if (m.version !== BRIDGE_VERSION) refuse(`bridge manifest version is ${JSON.stringify(m.version)}, not ${BRIDGE_VERSION}`);
  const prefix = `https://github.com/${repo}/releases/download/v${BRIDGE_VERSION}/`;
  const p = m.platforms || {};
  for (const k of PLATFORM_KEYS) if (!p[k]) refuse(`bridge manifest has no ${k}`);
  const oldId = keyIdOfPubkey(oldPubB64);
  for (const [k, e] of Object.entries(p)) {
    if (typeof e?.url !== "string" || !e.url.startsWith(prefix) || e.url.slice(prefix.length).includes("/") || e.url.includes("..")) refuse(`bridge ${k} url is not an asset of the v${BRIDGE_VERSION} release: ${e?.url}`);
    let id; try { id = keyIdOfSignature(e.signature); } catch { refuse(`bridge ${k} has no readable signature`); }
    if (id !== oldId) refuse(`bridge ${k} is not signed with the OLD key`);
  }
}
