import { ANIMAL_SVGS, animalFor, colorFor } from '../icons/planetIcon';
import { cn } from '@/lib/utils';

/// 行星图标:底色圆角块 + 白色动物剪影(svg fill=#fff,不做反色)。
/// 旧 .planet-icon 系规则对译:inline-flex 居中、12px 圆角、不缩、白字 11px、
/// img 60%;默认 12px 圆角,分组头内由调用方收 5px(原 .pgroup-head 覆写位)
export function PlanetIconView({ icon, color, name, id, size = 72, className }: {
  icon?: string | null; color?: string | null; name: string; id: string; size?: number; className?: string;
}) {
  const animal = animalFor(icon, name);
  const src = ANIMAL_SVGS[animal];
  return (
    <span
      className={cn(
        'inline-flex flex-none items-center justify-center rounded-xl text-[11px] text-white',
        className,
      )}
      style={{ width: size, height: size, background: colorFor(color, id) }}
    >
      {src ? <img src={src} alt={animal} className="h-3/5 w-3/5" /> : animal}
    </span>
  );
}

export default PlanetIconView;
