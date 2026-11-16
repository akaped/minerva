// Builds the web clipper browser extension for Chrome (and Edge, Brave…) and
// Firefox: dist/clipper/{chrome,firefox}/ to load unpacked, and a zip of each
// to upload to the stores. The code is bundled but not minified, so store
// reviewers can read it.
//
//   npm run build:clipper
import { build } from "esbuild";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { manifest } from "../clipper/manifest.js";

const root = new URL("..", import.meta.url).pathname;
const source = join(root, "clipper");
const out = join(root, "dist", "clipper");
const version = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root, "Cargo.toml"), "utf8"))[1];

rmSync(out, { recursive: true, force: true });
for (const browser of ["chrome", "firefox"]) {
  const dir = join(out, browser);
  mkdirSync(dir, { recursive: true });
  await build({
    entryPoints: ["background", "popup", "options", "extract"].map((name) => join(source, "src", `${name}.js`)),
    outdir: dir,
    bundle: true,
    format: "iife",
    target: browser === "firefox" ? "firefox140" : "chrome116",
    legalComments: "inline",
    logLevel: "warning",
  });
  for (const file of ["popup.html", "options.html", "clipper.css"]) cpSync(join(source, "src", file), join(dir, file));
  cpSync(join(source, "icons"), join(dir, "icons"), { recursive: true });
  cpSync(join(source, "_locales"), join(dir, "_locales"), { recursive: true });
  cpSync(join(root, "LICENSE"), join(dir, "LICENSE"));
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest(browser, version), null, 2) + "\n");
  const zip = join(out, `minerva-clipper-${browser}-${version}.zip`);
  try {
    execFileSync("zip", ["-qr", "-X", zip, "."], { cwd: dir });
  } catch {
    console.warn(`zip is not installed: load ${dir} unpacked, or zip it yourself.`);
  }
  console.log(`${browser}: ${dir}${existsSync(zip) ? ` and ${zip}` : ""}`);
}
