#!/usr/bin/env node
// Downloads the Satoshi typeface into assets/fonts for the React UI and the
// native frontends to embed.
//
// Satoshi is licensed under the ITF Free Font License, which permits embedding
// in applications but not redistributing the font files through a repository,
// so the files are fetched locally and git-ignored. Run this before building a
// release bundle: `npm run fonts`.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = join(ROOT, "assets/fonts");
const ARCHIVE_URL = "https://api.fontshare.com/v2/fonts/download/satoshi";
const WANTED = [
  "Satoshi_Complete/Fonts/TTF/Satoshi-Variable.ttf",
  "Satoshi_Complete/Fonts/WEB/fonts/Satoshi-Variable.woff2",
  "Satoshi_Complete/Fonts/WEB/fonts/Satoshi-VariableItalic.woff2",
  "Satoshi_Complete/License/FFL.txt",
];

const renamed = (path) => join(TARGET, path.split("/").at(-1));

if (WANTED.every((path) => existsSync(renamed(path)))) {
  console.log("Satoshi is already in assets/fonts");
  process.exit(0);
}

mkdirSync(TARGET, { recursive: true });
const archive = join(TARGET, "satoshi.zip");
console.log(`Downloading Satoshi from ${ARCHIVE_URL}`);
const response = await fetch(ARCHIVE_URL);
if (!response.ok) {
  console.error(`Fontshare returned ${response.status}; keeping the system font`);
  process.exit(1);
}
writeFileSync(archive, Buffer.from(await response.arrayBuffer()));

for (const path of WANTED) {
  execFileSync("unzip", ["-o", "-j", archive, path, "-d", TARGET], { stdio: "inherit" });
}
execFileSync("rm", ["-f", archive]);

const license = readFileSync(join(TARGET, "FFL.txt"), "utf8");
writeFileSync(
  join(TARGET, "README.md"),
  [
    "# Satoshi",
    "",
    "Fetched by `npm run fonts` from https://www.fontshare.com/fonts/satoshi.",
    "",
    "Embedded in DBM's builds under the ITF Free Font License, which allows",
    "embedding in desktop applications but not redistributing the font files",
    "through a repository. Do not commit this directory.",
    "",
    "## License",
    "",
    license,
  ].join("\n"),
);
console.log("Satoshi is ready in assets/fonts");
