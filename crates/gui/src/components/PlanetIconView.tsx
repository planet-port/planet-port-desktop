import { ANIMAL_SVGS, animalFor, colorFor } from '../icons/planetIcon';

/// 行星图标:底色圆角块 + 白色动物剪影(svg fill=#fff,不做反色)
export function PlanetIconView({ icon, color, name, id, size = 72 }: {
  icon?: string | null; color?: string | null; name: string; id: string; size?: number;
}) {
  const animal = animalFor(icon, name);
  const src = ANIMAL_SVGS[animal];
  return (
    <span className="planet-icon" style={{ width: size, height: size, background: colorFor(color, id) }}>
      {src ? <img src={src} alt={animal} /> : animal}
    </span>
  );
}

export default PlanetIconView;
