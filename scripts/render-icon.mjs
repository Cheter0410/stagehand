// Renders assets/icon.png (1024) and assets/icon.ico from assets/icon.svg with
// the Playwright Chromium. Run after changing the icon:  node scripts/render-icon.mjs
// (needs python3 + Pillow for the .ico)
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const svg = readFileSync(join(root, "assets/icon.svg")).toString("base64");

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1024, height: 1024 } });
await page.setContent(
  `<html><body style="margin:0;background:transparent"><img style="display:block" width="1024" height="1024" src="data:image/svg+xml;base64,${svg}"></body></html>`,
);
await page.screenshot({ path: join(root, "assets/icon.png"), omitBackground: true });
await browser.close();
execFileSync("python3", [
  "-c",
  "import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2], sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])",
  join(root, "assets/icon.png"),
  join(root, "assets/icon.ico"),
]);
console.log("icons written");
