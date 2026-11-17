#!/bin/bash
# Rehearses the 0.3.6 key rotation end to end with THROWAWAY keys (never the real ones).
#   tools/key-rotation-harness/rehearse.sh <scratch-dir>     (a folder outside the repo; needs the app's npm install for the Tauri CLI)
# Builds fake bundles, signs them with the real `tauri signer`, runs the real release scripts
# (tools/release/*.mjs) on them, serves the results from 127.0.0.1:5491 and lets the harness (the updater's
# own checks) play 0.3.4 / 0.3.5 / 0.3.6 clients against it. Prints PASS/FAIL per path; exits 1 on any FAIL.
set -uo pipefail
W=${1:?scratch dir}; mkdir -p "$W"; W=$(cd "$W" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$HERE/../.." && pwd)
CLI="$REPO/app/node_modules/@tauri-apps/cli/tauri.js"
H="$W/target/release/key-rotation-harness"
PORT=${PORT:-5491}
FAKE="$W/repo"; WEB="$W/web"; K="$W/keys"
fails=0
say() { printf '%s\n' "$*"; }
res() { # label, expected-substring, actual output
  if printf '%s' "$3" | grep -qF -- "$2"; then say "PASS  $1  [$(printf '%s' "$3" | tr '\n' ' ')]"; else say "FAIL  $1  expected '$2', got: $3"; fails=$((fails+1)); fi
}

[ -f "$H" ] || (cd "$HERE" && CARGO_TARGET_DIR="$W/target" nice -n 10 cargo build --offline -j 4 --release -q) || exit 2
mkdir -p "$K" "$FAKE/app/src-tauri" "$FAKE/tools/release" "$WEB"
node "$CLI" signer generate --ci -p "" -w "$K/old.key" >/dev/null
node "$CLI" signer generate --ci -p "rehearsal-pass" -w "$K/new.key" >/dev/null
OLDPUB=$(tr -d '\n' < "$K/old.key.pub"); NEWPUB=$(tr -d '\n' < "$K/new.key.pub")
cp "$REPO"/tools/release/*.mjs "$FAKE/tools/release/"
printf '%s' "$OLDPUB" > "$FAKE/tools/release/old-pubkey.b64"
printf '%s' "$OLDPUB" > "$W/old.pub.b64"; printf '%s' "$NEWPUB" > "$W/new.pub.b64"

setconf() { # version, pubkey
  node -e '
    const fs=require("fs");const c=JSON.parse(fs.readFileSync(process.argv[1]));
    c.version=process.argv[2]; c.plugins.updater.pubkey=process.argv[3];
    fs.writeFileSync(process.argv[4],JSON.stringify(c,null,2));' "$REPO/app/src-tauri/tauri.conf.json" "$1" "$2" "$FAKE/app/src-tauri/tauri.conf.json"
}
sign_with() { # keyname password bundlesdir version
  local f
  for f in $(node -p "Object.values(require('$3/manifest.json').update).join(' ')"); do
    TAURI_SIGNING_PRIVATE_KEY="$(cat "$K/$1.key")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$2" node "$CLI" signer sign --app-version "$4" "$3/$f" >/dev/null || return 1
  done
}
fake_bundles() { # dir version
  local d=${1:?} v=${2:?}; rm -rf "${d:?}"; mkdir -p "$d/macos" "$d/windows" "$d/linux"
  for f in macos/SketchCraft.app.tar.gz windows/SketchCraft_${v}_x64-setup.exe linux/SketchCraft_${v}_amd64.AppImage; do
    { head -c 3000 /dev/urandom; echo "fake $f $v"; } > "$d/$f"
  done
  node -e '
    const v=process.argv[1];const u={macos:"macos/SketchCraft.app.tar.gz",windows:`windows/SketchCraft_${v}_x64-setup.exe`,linux:`linux/SketchCraft_${v}_amd64.AppImage`};
    require("fs").writeFileSync(process.argv[2],JSON.stringify({update:u,upload:Object.values(u)}))' "$v" "$d/manifest.json"
}
manifests() { GITHUB_REF_NAME=$1 GITHUB_REPOSITORY=jasonsutter87/sketchcraft node "$FAKE/tools/release/make-manifests.mjs" "$FAKE" "$2" 2>&1; }
keycheck() { GITHUB_REPOSITORY=jasonsutter87/sketchcraft TAURI_SIGNING_PRIVATE_KEY="$(cat "$K/$1.key")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$2" node "$FAKE/tools/release/key-check.mjs" "$FAKE" "$CLI" 2>&1; }
publish_dir() { # bundles dir, version -> release assets folder
  local out="$W/rel/v${2:?}" f; rm -rf "${out:?}"; mkdir -p "$out"
  for f in $(node -p "const m=require('$1/manifest.json'); m.upload.concat(Object.values(m.update).map(u=>u+'.sig')).join(' ')"); do cp "$1/$f" "$out/$(basename "$f")"; done
  cp "$1/latest.json" "$1/latest-v2.json" "$out/"
}
state() { # name, release dir the "latest" manifests come from, release dirs to include...
  local name=${1:?} latest=${2:?}; shift 2
  local s="$WEB/$name/releases" r; rm -rf "${WEB:?}/${name:?}"; mkdir -p "$s/download" "$s/latest/download"
  for r in "$@"; do cp -R "$W/rel/$r" "$s/download/$r"; done
  cp "$W/rel/$latest/latest.json" "$W/rel/$latest/latest-v2.json" "$s/latest/download/"
}
client() { "$H" --base "http://127.0.0.1:$PORT/$1" --current "$2" --old-pub "$W/old.pub.b64" --new-pub "$W/new.pub.b64" --conf "$FAKE/app/src-tauri/tauri.conf.json" ${3:-}; }

say "== Build the 0.3.6 bridge release (OLD key) and the 0.3.7 release (NEW key) =="
setconf 0.3.6 "$NEWPUB"
say "preflight 0.3.6: $(cd "$FAKE" && node tools/release/preflight.mjs . 2>&1)"
fake_bundles "$W/b036" 0.3.6; sign_with old "" "$W/b036" 0.3.6
say "key-check (OLD secret): $(keycheck old "")"
say "$(manifests v0.3.6 "$W/b036")"; publish_dir "$W/b036" 0.3.6
cmp "$W/rel/v0.3.6/latest.json" "$W/rel/v0.3.6/latest-v2.json" && say "0.3.6: latest.json and latest-v2.json are identical"
node "$FAKE/tools/release/freeze-bridge.mjs" "$W/rel/v0.3.6/latest.json" && say "bridge-latest.json frozen from the 0.3.6 manifest"
setconf 0.3.7 "$NEWPUB"
say "preflight 0.3.7: $(cd "$FAKE" && node tools/release/preflight.mjs . 2>&1)"
fake_bundles "$W/b037" 0.3.7; sign_with new rehearsal-pass "$W/b037" 0.3.7
say "key-check (NEW secret): $(keycheck new rehearsal-pass)"
say "$(manifests v0.3.7 "$W/b037")"; publish_dir "$W/b037" 0.3.7
cmp "$W/rel/v0.3.7/latest.json" "$FAKE/tools/release/bridge-latest.json" && say "0.3.7: latest.json is a byte copy of the frozen 0.3.6 manifest"
say "0.3.7 latest-v2.json announces: $(node -p "require('$W/rel/v0.3.7/latest-v2.json').version"); latest.json announces: $(node -p "require('$W/rel/v0.3.7/latest.json').version")"

# states: what the server shows at releases/latest/download/ and releases/download/
state s_after036 v0.3.6 v0.3.6
state s_after037 v0.3.7 v0.3.6 v0.3.7
# 0.3.5 client offered a NEW-signed 0.3.7 at the old path
state s_newsigned_old v0.3.7 v0.3.7; cp "$W/rel/v0.3.7/latest-v2.json" "$WEB/s_newsigned_old/releases/latest/download/latest.json"
# 0.3.6 client offered an OLD-signed fake 0.3.7 at latest-v2.json
fake_bundles "$W/bfake" 0.3.7; sign_with old "" "$W/bfake" 0.3.7
mkdir -p "$W/rel/vfake"
for f in macos/SketchCraft.app.tar.gz windows/SketchCraft_0.3.7_x64-setup.exe linux/SketchCraft_0.3.7_amd64.AppImage; do cp "$W/bfake/$f" "$W/rel/vfake/$(basename "$f")"; done
node -e '
  const fs=require("fs"),w=process.argv[1];const b="https://github.com/jasonsutter87/sketchcraft/releases/download/v0.3.7/";
  const e=(f,n)=>({signature:fs.readFileSync(`${w}/bfake/${f}.sig`,"utf8").trim(),url:b+n});
  const m=e("macos/SketchCraft.app.tar.gz","SketchCraft.app.tar.gz");
  const j=JSON.stringify({version:"0.3.7",platforms:{"darwin-aarch64":m,"darwin-x86_64":m,"windows-x86_64":e("windows/SketchCraft_0.3.7_x64-setup.exe","SketchCraft_0.3.7_x64-setup.exe"),"linux-x86_64":e("linux/SketchCraft_0.3.7_amd64.AppImage","SketchCraft_0.3.7_amd64.AppImage")}});
  fs.writeFileSync(`${w}/rel/vfake/latest-v2.json`,j);fs.writeFileSync(`${w}/rel/vfake/latest.json`,j);' "$W"
state s_oldsigned_fake vfake; mkdir -p "$WEB/s_oldsigned_fake/releases/download/v0.3.7"; cp "$W"/rel/vfake/SketchCraft* "$WEB/s_oldsigned_fake/releases/download/v0.3.7/"
# a good 0.3.7 whose macOS bundle was altered in transit
state s_tampered v0.3.7 v0.3.6 v0.3.7; printf 'X' | dd of="$WEB/s_tampered/releases/download/v0.3.7/SketchCraft.app.tar.gz" bs=1 seek=100 conv=notrunc 2>/dev/null

( cd "$WEB" && exec python3 -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1 ) & SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT
sleep 1

say; say "== Clients =="
res "a. 0.3.5 client -> 0.3.6 (old key, latest.json)"           "0.3.5: ACCEPTED 0.3.6" "$(client s_after036 0.3.5)"
res "a'. then, as 0.3.6, nothing newer yet"                     "0.3.6: no update" "$(client s_after036 0.3.6)"
res "b. 0.3.6 client -> 0.3.7 (new key, latest-v2.json)"        "0.3.6: ACCEPTED 0.3.7" "$(client s_after037 0.3.6)"
out=$(client s_after037 0.3.4 --follow)
res "c. 0.3.4 skipped 0.3.6: latest.json bridges it to 0.3.6"   "0.3.4: ACCEPTED 0.3.6" "$out"
res "c'. ...and its next poll (now 0.3.6) gets 0.3.7"           "0.3.6: ACCEPTED 0.3.7" "$out"
res "d1. 0.3.5 client offered NEW-signed 0.3.7"                 "REJECTED" "$(client s_newsigned_old 0.3.5)"
res "d2. 0.3.6 client offered OLD-signed fake 0.3.7"            "REJECTED" "$(client s_oldsigned_fake 0.3.6)"
res "d3. 0.3.6 client, tampered 0.3.7 bundle"                   "REJECTED" "$(client s_tampered 0.3.6)"
res "d4. 0.3.7 client offered the older 0.3.6 manifest (downgrade)" "0.3.7: no update" "$(client s_after036 0.3.7)"
res "d4'. 0.3.7 client offered 0.3.7 again"                     "0.3.7: no update" "$(client s_after037 0.3.7)"

say; say "== Release scripts fail closed =="
setconf 0.3.7 "$NEWPUB"; cp "$FAKE/tools/release/bridge-latest.json" "$W/bridge.good"
rm -f "$FAKE/tools/release/bridge-latest.json"
res "e1. post-bridge release, no bridge file (preflight)"      "is missing" "$(cd "$FAKE" && node tools/release/preflight.mjs . 2>&1)"
res "e1'. same, at manifest time"                               "is missing" "$(manifests v0.3.7 "$W/b037")"
node -e 'const fs=require("fs");const m=JSON.parse(fs.readFileSync(process.argv[1]));m.version="0.3.5";fs.writeFileSync(process.argv[2],JSON.stringify(m))' "$W/bridge.good" "$FAKE/tools/release/bridge-latest.json"
res "e2. bridge file whose version isn't 0.3.6"                 "not 0.3.6" "$(manifests v0.3.7 "$W/b037")"
node -e 'const fs=require("fs");const m=JSON.parse(fs.readFileSync(process.argv[1]));for(const p of Object.values(m.platforms))p.url=p.url.replace("v0.3.6","v0.3.7");fs.writeFileSync(process.argv[2],JSON.stringify(m))' "$W/bridge.good" "$FAKE/tools/release/bridge-latest.json"
res "e3. bridge URLs not at the v0.3.6 release"                 "not an asset of the v0.3.6 release" "$(manifests v0.3.7 "$W/b037")"
cp "$W/rel/v0.3.7/latest-v2.json" "$FAKE/tools/release/bridge-latest.json"; sed -i '' -e 's/"version": "0.3.7"/"version": "0.3.6"/' -e 's#/download/v0.3.7/#/download/v0.3.6/#g' "$FAKE/tools/release/bridge-latest.json"
res "e4. a NEW-key manifest posing as the bridge"               "not signed with the OLD key" "$(manifests v0.3.7 "$W/b037")"
cp "$W/bridge.good" "$FAKE/tools/release/bridge-latest.json"
res "e5. post-bridge release, secret never swapped (OLD key)"  "doesn't match" "$(keycheck old "")"
setconf 0.3.6 "$NEWPUB"; res "e6. bridge release signed with the NEW key"      "doesn't match" "$(keycheck new rehearsal-pass)"
setconf 0.3.7 "$OLDPUB"; res "e7. tauri.conf.json still has the OLD pubkey"    "OLD public key" "$(manifests v0.3.7 "$W/b037")"
setconf 0.3.7 "$NEWPUB"; res "e8. tag doesn't match the app version"           "is not v0.3.7" "$(manifests v0.3.8 "$W/b037")"
setconf 0.3.5 "$NEWPUB"; res "e9. a version older than the bridge"             "older than the bridge" "$(manifests v0.3.5 "$W/b036")"
setconf 0.3.7 "$NEWPUB"; fake_bundles "$W/b037o" 0.3.7; sign_with old "" "$W/b037o" 0.3.7
res "e10. post-bridge bundles signed with the OLD key"          "signed with key" "$(manifests v0.3.7 "$W/b037o")"
fake_bundles "$W/b037x" 0.3.7; sign_with new rehearsal-pass "$W/b037x" 0.3.7; echo "tamper" >> "$W/b037x/macos/SketchCraft.app.tar.gz"
res "e11. a bundle changed after signing"                       "does not match" "$(manifests v0.3.7 "$W/b037x")"
res "e12. no manifest left behind by a refused run"             "none" "$(ls "$W"/b037x/latest*.json "$W"/b037o/latest*.json 2>/dev/null || echo none)"
say; if [ $fails -eq 0 ]; then say "ALL PASS"; else say "$fails FAILED"; fi
exit $((fails>0))
