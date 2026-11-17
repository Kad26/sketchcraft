// publish job, before anything is signed for real:
//   node tools/release/key-check.mjs <repo-root> <path to tauri.js>      (secret in TAURI_SIGNING_PRIVATE_KEY[_PASSWORD])
// Signs a throwaway file with the secret and verifies it against the key this release must be signed with:
//   v0.3.6 (bridge)  -> the OLD public key (tools/release/old-pubkey.b64), the secret as it stands today
//   anything later   -> the public key in tauri.conf.json (the NEW key)
// A wrong or un-swapped secret fails here, instead of shipping updates nobody can install.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { readConfig, verifyData, Refusal } from "./bridge.mjs";

const [root, cli] = process.argv.slice(2);
try {
  const { version, mode, pub, old } = readConfig(root, process.env.GITHUB_REPOSITORY || "jasonsutter87/sketchcraft");
  const expected = mode === "bridge" ? old : pub;
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "keycheck-"));
  const f = path.join(dir, "probe.txt");
  const data = Buffer.from(`key check ${Date.now()}`);
  fs.writeFileSync(f, data);
  const r = spawnSync(process.execPath, [cli, "signer", "sign", "--app-version", version, f], { encoding: "utf8", env: process.env });
  if (r.status !== 0) throw new Refusal("the signing key could not sign (missing, wrong password, or unreadable)");
  verifyData(expected, data, fs.readFileSync(`${f}.sig`, "utf8"));
  fs.rmSync(dir, { recursive: true, force: true });
  console.log(`key ok: v${version} is a ${mode} release and the secret signs for the ${mode === "bridge" ? "OLD" : "NEW"} public key`);
} catch (e) {
  if (!(e instanceof Refusal)) throw e;
  console.error(`::error::The signing secret doesn't match the public key this release needs: ${e.message}. After v0.3.6 ships, swap TAURI_SIGNING_PRIVATE_KEY to the v2 key and set TAURI_SIGNING_PRIVATE_KEY_PASSWORD (docs/SIGNING.md).`);
  process.exit(1);
}
