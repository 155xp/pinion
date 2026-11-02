// Bundles ui/ into dist/ for Tauri. `--watch` rebuilds on change.
import * as esbuild from "esbuild";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "dist");
const watch = process.argv.includes("--watch");

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "fonts"), { recursive: true });
for (const f of ["index.html", "pill.html"]) cpSync(join(root, "ui", f), join(out, f));
const fonts = {
  "inter-latin-wght-normal.woff2": "@fontsource-variable/inter/files/inter-latin-wght-normal.woff2",
  "literata-latin-wght-normal.woff2": "@fontsource-variable/literata/files/literata-latin-wght-normal.woff2",
  "literata-latin-wght-italic.woff2": "@fontsource-variable/literata/files/literata-latin-wght-italic.woff2",
};
for (const [name, src] of Object.entries(fonts)) cpSync(join(root, "node_modules", src), join(out, "fonts", name));

const options = {
  entryPoints: [
    { in: join(root, "ui/src/main.js"), out: "app" },
    { in: join(root, "ui/styles/app.css"), out: "app" },
    { in: join(root, "ui/src/pill.js"), out: "pill" },
    { in: join(root, "ui/styles/pill.css"), out: "pill" },
  ],
  outdir: out,
  bundle: true,
  minify: !watch,
  format: "esm",
  target: ["es2021", "safari15"],
  external: ["fonts/*"],
  legalComments: "none",
  logLevel: "warning",
};

if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
