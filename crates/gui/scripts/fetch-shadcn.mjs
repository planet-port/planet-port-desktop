// scripts/fetch-shadcn.mjs — 经 api.github.com contents 拉取 shadcn new-york-v4 组件源码
// (ui.shadcn.com 与 raw.githubusercontent.com 在本网络不可达,api 域可达)
// 用法: node scripts/fetch-shadcn.mjs switch button input label
// 可重复执行(幂等):同名覆盖落盘,内容无变化时跳过写。
// 唯一允许的适配:路径归一(不改逻辑):
//   `from "cn"`                    → `from "@/lib/utils"`(shadcn 仓库内 cn 别名指向 lib/utils,本仓库对应 src/lib/utils.ts)
//   `@/registry/new-york-v4/ui/`   → `@/components/ui/`(shadcn 仓库内注册表自引用路径,归一到本仓库落盘位置)
// 其余 import 原样保留并分类打印:radix-ui 聚合包 / 本地 @/ 交叉引用 / 待安装外部包。
import { writeFile, mkdir, readFile } from 'node:fs/promises';

const REG = 'https://api.github.com/repos/shadcn-ui/ui/contents/apps/v4/registry/new-york-v4/ui';
const OUT_DIR = 'src/components/ui';

function classifyImports(src) {
  const specs = [...src.matchAll(/from\s+["']([^"']+)["']/g)].map((m) => m[1]);
  const seen = new Set();
  const radix = [];
  const local = [];
  const external = [];
  for (const spec of specs) {
    if (seen.has(spec)) continue;
    seen.add(spec);
    if (spec === '@/lib/utils') continue;
    if (spec === 'radix-ui') radix.push(spec);
    else if (spec.startsWith('@/')) local.push(spec);
    else external.push(spec);
  }
  return { radix, local, external };
}

const names = process.argv.slice(2);
if (names.length === 0) {
  console.error('用法: node scripts/fetch-shadcn.mjs <name...>');
  process.exit(1);
}

await mkdir(OUT_DIR, { recursive: true });
const failed = [];

for (const name of names) {
  let res;
  try {
    res = await fetch(`${REG}/${name}.tsx?ref=main`, {
      headers: { Accept: 'application/vnd.github.raw+json' },
    });
  } catch (err) {
    console.error(`FAIL ${name}: ${err.message}`);
    failed.push(name);
    continue;
  }
  if (!res.ok) {
    console.error(`FAIL ${name}: ${res.status}`);
    failed.push(name);
    continue;
  }
  const src = (await res.text())
    .replaceAll('from "cn"', 'from "@/lib/utils"')
    .replaceAll('from "@/registry/new-york-v4/ui/', 'from "@/components/ui/');
  const { radix, local, external } = classifyImports(src);

  const target = `${OUT_DIR}/${name}.tsx`;
  let prev = null;
  try {
    prev = await readFile(target, 'utf8');
  } catch {
    // 首次拉取,无旧文件
  }
  if (prev === src) {
    console.log(`SKIP ${name} (内容无变化)`);
  } else {
    await writeFile(target, src);
    console.log(`OK ${name} (${src.length}B${prev == null ? ', 新增' : ', 覆盖'})`);
  }
  console.log(`     deps radix-ui=[${radix.join(',')}] local=[${local.join(',')}] external=[${external.join(',')}]`);
}

if (failed.length > 0) {
  console.error(`共 ${failed.length} 个拉取失败: ${failed.join(', ')}`);
  process.exitCode = 1;
}
