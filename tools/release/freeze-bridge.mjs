// Run once, by the owner, right after v0.3.6 is published (docs/SIGNING.md):
//   gh release download v0.3.6 -R jasonsutter87/sketchcraft -p latest.json -D /tmp/bridge
//   node tools/release/freeze-bridge.mjs /tmp/bridge/latest.json
// Checks the file is 0.3.6's real manifest (version, v0.3.6 URLs only, OLD-key signatures) and
// saves it, byte for byte, as tools/release/bridge-latest.json. Then commit that file.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { BRIDGE_FILE, Refusal, readOldPubkey, validateBridge } from "./bridge.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const [src, repoArg] = process.argv.slice(2);
if (!src) { console.error("usage: freeze-bridge.mjs <path to v0.3.6's latest.json> [owner/repo]"); process.exit(2); }
try {
  const text = fs.readFileSync(src, "utf8");
  validateBridge(JSON.parse(text), repoArg || "jasonsutter87/sketchcraft", readOldPubkey(root));
  fs.writeFileSync(path.join(root, BRIDGE_FILE), text);
  console.log(`saved ${BRIDGE_FILE}; commit it before tagging the next release`);
} catch (e) {
  if (!(e instanceof Refusal)) throw e;
  console.error(`refused: ${e.message}`);
  process.exit(1);
}
