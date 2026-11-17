# Opal (vendored)

SketchCraft runs SketchUp Ruby scripts by compiling them to JavaScript with
[Opal](https://opalrb.com) (MIT), inside the plugin sandbox.

| File | From | Version |
| --- | --- | --- |
| `opal.js` | npm `opal-runtime` (`src/opal.js`) | 3.0.0 (Opal 1.7.3) |
| `opal-compiler.js` | npm `opal-compiler` (`src/opal-builder.js`) | 3.0.0 (Opal 1.7.3) |

Licenses: `LICENSE-opal-runtime`, `LICENSE-opal-compiler`. Only the compiler
(`opal/compiler`) and the core runtime are used; nothing is loaded at run time
from the network.

**One local change to `opal.js`:** Opal IRB's `ensure_loaded` (it fetched code from
`cdn.opalrb.com` with a synchronous request and ran it with `new Function`) is replaced by an
error. It was unreachable (the plugin sandbox removes `XMLHttpRequest`, and the app's content
security policy allows no other host) and is now gone. To check the rest against upstream, unpack
npm `opal-runtime@3.0.0` and `diff` its `src/opal.js` with this file: that block is the only
difference. `SHA256SUMS` is therefore the sum of this edited copy, not of the npm file.

`SHA256SUMS` fixes the exact bytes of each file; CI checks it
(`sha256sum -c SHA256SUMS`), so a changed copy of the compiler or runtime can't
slip in unnoticed. After a deliberate update, regenerate it
(`shasum -a 256 opal.js opal-compiler.js LICENSE-opal-* > SHA256SUMS`).
