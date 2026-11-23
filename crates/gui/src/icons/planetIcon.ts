/// 行星图标体系:与移动端(iOS/Android)逐字一致。
/// catalog 顺序两端一致;djb2 名称哈希(UTF-8 逐字节、64 位溢出回绕、
/// 先 %1_000_003 再 %表长)决定回退动物——任何一环不同,同一行星在
/// 两端会显示不同动物。
export const ANIMALS: readonly string[] = [
  'ant', 'anteater', 'armadillo', 'badger', 'bat', 'beaver', 'bee', 'bird',
  'bison', 'boar', 'bug', 'bull', 'butterfly', 'camel', 'cat', 'cobra',
  'cow', 'crab', 'crow', 'deer', 'dinosaur-rex', 'dodo', 'dog', 'dolphin',
  'donkey', 'dove', 'dragon', 'dragonfly', 'duck', 'eel', 'elephant', 'fish',
  'flamingo', 'fox', 'frog', 'gecko', 'goat', 'goose', 'gorilla', 'hedgehog',
  'heron', 'hippo', 'horse', 'hummingbird', 'ibis', 'jellyfish', 'kangaroo',
  'kiwi-bird', 'koala', 'ladybug', 'lion', 'locust', 'mammoth', 'mole', 'monkey',
  'mosquito', 'octopus', 'ostrich', 'otter', 'owl', 'panda', 'penguin', 'pig',
  'piranha', 'polar-bear', 'rabbit', 'rattlesnake', 'raven', 'rodent', 'rooster',
  'scorpion', 'seahorse', 'seal', 'shark', 'sheep', 'shrimp', 'sloth', 'snail',
  'snake', 'sperm-whale', 'spider', 'squid', 'squirrel', 'swallow', 'swan', 'tiger',
  'tortoise', 'turkey', 'turtle', 'unicorn', 'vulture', 'worm',
];

/// 预设底色 24 色(iOS IconPickerSheet 逐字;600/700 明度域,白图形对比可靠)
export const PALETTE: readonly string[] = [
  '#dc2626', '#e11d48', '#db2777', '#9f1239',
  '#ea580c', '#c2410c', '#d97706', '#ca8a04',
  '#65a30d', '#4d7c0f', '#16a34a', '#059669',
  '#0d9488', '#0891b2', '#0284c7', '#2563eb',
  '#1e40af', '#4f46e5', '#7c3aed', '#9333ea',
  '#c026d3', '#6b7280', '#475569', '#334155',
];

/// djb2 哈希:UTF-8 逐字节、64 位溢出回绕,返回 hash % 1_000_003。
/// BigInt 表达 64 位回绕(JS number 精度不足,32 位截断会落错动物)。
export function djb2(s: string): number {
  let h = 5381n;
  for (const b of new TextEncoder().encode(s)) h = (h * 33n + BigInt(b)) & 0xffff_ffff_ffff_ffffn;
  return Number(h % 1_000_003n);
}

/// 动物 key:有效自定义值永久生效,否则按名称哈希回退(名称不变图标不变)
export function animalFor(icon: string | null | undefined, name: string): string {
  // 官方行星专用图标(animals/zerotier.svg):不入 ANIMALS——表长参与
  // djb2 取模,追加会漂移既有行星的回退动物,破坏与移动端的一致契约
  if (icon === 'zerotier') return icon;
  if (icon && (ANIMALS as readonly string[]).includes(icon)) return icon;
  return ANIMALS[djb2(name) % ANIMALS.length]!;
}

function hslToHex(h: number, s: number, l: number): string {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x]
    : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  const to = (v: number) => Math.round((v + m) * 255).toString(16).padStart(2, '0');
  return `#${to(r)}${to(g)}${to(b)}`;
}

function hexToHsl(hex: string): { h: number; s: number; l: number } | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1]!, 16);
  const r = ((n >> 16) & 255) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) * 60;
  else if (max === g) h = ((b - r) / d + 2) * 60;
  else h = ((r - g) / d + 4) * 60;
  return { h, s, l };
}

/// 回退底色盘:固定基色(默认红 #D44037,桌面无移动端主题色概念)按
/// Android ThemeManager 派生式生成 8 色——基色 + 120°/240° 三相位,
/// 各取 (S.65,L.52)/(S.55,L.68) 两档,再加两相位深色 (S.70,L.32)。
/// 浮点舍入与移动端可能差 ±1(自定义底色两端精确一致才是硬契约)。
function fallbackTints(): string[] {
  const base = hexToHsl('#D44037');
  if (!base) return [...PALETTE];
  const hues = [base.h, (base.h + 120) % 360, (base.h + 240) % 360];
  const tints: string[] = [];
  for (const h of hues) tints.push(hslToHex(h, 0.65, 0.52));
  for (const h of hues) tints.push(hslToHex(h, 0.55, 0.68));
  tints.push(hslToHex(hues[1]!, 0.7, 0.32));
  tints.push(hslToHex(hues[2]!, 0.7, 0.32));
  return tints;
}

/// 底色:自定义 #RRGGBB 优先(两端精确一致),否则按 id 哈希落派生盘
export function colorFor(color: string | null | undefined, id: string): string {
  if (color && /^#[0-9a-fA-F]{6}$/.test(color)) return color.toLowerCase();
  const tints = fallbackTints();
  return tints[djb2(id) % tints.length]!;
}

/// 随机底色(iOS 参数:hue∈[0,360)、S∈[0.45,0.85]、L∈[0.32,0.55])
export function randomColorHex(): string {
  return hslToHex(Math.random() * 360, 0.45 + Math.random() * 0.4, 0.32 + Math.random() * 0.23);
}

/// 随机动物(添加行星时初始图标/「换个随机图标」用)
export function randomAnimal(): string {
  return ANIMALS[Math.floor(Math.random() * ANIMALS.length)]!;
}

/// SVG 资源映射(animals/*.svg;fill=#fff,中性面板场景由 CSS 反色)
export const ANIMAL_SVGS: Readonly<Record<string, string>> = Object.fromEntries(
  Object.entries(
    import.meta.glob('./animals/*.svg', { eager: true, query: '?url', import: 'default' }) as Record<string, string>,
  ).map(([k, v]) => [k.slice('./animals/'.length, -'.svg'.length), v]),
);
