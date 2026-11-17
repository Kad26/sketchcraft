// guard job, before anything is built: says which path this tag takes and fails early if it can't be taken.
//   node tools/release/preflight.mjs <repo-root>
//   v0.3.6 -> "bridge": needs the new public key and the latest-v2.json endpoint in tauri.conf.json
//   later  -> "post":   additionally needs the frozen bridge file (tools/release/bridge-latest.json)
import fs from "node:fs";
import path from "node:path";
import { BRIDGE_FILE, Refusal, readConfig, refuse, validateBridge } from "./bridge.mjs";

const root = process.argv[2];
const repo = process.env.GITHUB_REPOSITORY || "jasonsutter87/sketchcraft";
try {
  const { version, mode, old } = readConfig(root, repo);
  if (mode === "post") {
    const bf = path.join(root, BRIDGE_FILE);
    if (!fs.existsSync(bf)) refuse(`${BRIDGE_FILE} is missing: freeze v0.3.6's latest.json first (docs/SIGNING.md, "Key rotation")`);
    validateBridge(JSON.parse(fs.readFileSync(bf, "utf8")), repo, old);
  }
  console.log(`v${version} takes the ${mode} path`);
} catch (e) {
  if (!(e instanceof Refusal)) throw e;
  console.error(`::error::${e.message}`);
  process.exit(1);
}
