#!/usr/bin/env node
// Android animal VectorDrawable → SVG(一次性移植;入库留档,资产变更时重跑)。
// 92 个文件全部为单 path;viewport 混有 24/512/576/640,须逐文件读取。
// fill 固定 #fff:行星图标恒为「彩底+白动物」,中性网格场景由 CSS 反色。
import { readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const src = process.argv[2];
if (!src) {
  console.error('usage: node port-animal-icons.mjs <android-res-drawable-dir>');
  process.exit(1);
}
const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'icons', 'animals');
mkdirSync(out, { recursive: true });
let n = 0;
for (const f of readdirSync(src).sort()) {
  if (!f.startsWith('animal_') || !f.endsWith('.xml')) continue;
  const xml = readFileSync(join(src, f), 'utf8');
  const d = xml.match(/android:pathData="([^"]+)"/)?.[1];
  const vw = xml.match(/android:viewportWidth="([\d.]+)"/)?.[1];
  const vh = xml.match(/android:viewportHeight="([\d.]+)"/)?.[1];
  if (!d || !vw || !vh) throw new Error(`missing pathData/viewport: ${f}`);
  const key = f.slice('animal_'.length, -'.xml'.length).replace(/_/g, '-');
  writeFileSync(
    join(out, `${key}.svg`),
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${vw} ${vh}"><path fill="#fff" d="${d}"/></svg>\n`,
  );
  n++;
}
console.log(`ported ${n} animal icons -> ${out}`);
