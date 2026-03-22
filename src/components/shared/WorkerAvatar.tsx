/**
 * WorkerAvatar - White-square initial letter indicator
 *
 * Replaces the old SheepAvatar with a clean, architectural glyph
 * following the Alchemical Ledger design language.
 */

interface WorkerAvatarProps {
  name: string;
  size?: number;
  class?: string;
}

export function WorkerAvatar(props: WorkerAvatarProps) {
  const s = () => props.size ?? 34;
  return (
    <div
      class={`flex items-center justify-center bg-white text-black font-bold uppercase flex-shrink-0 ${props.class ?? ''}`}
      style={{
        width: `${s()}px`,
        height: `${s()}px`,
        'font-size': `${Math.round(s() * 0.4)}px`,
      }}
    >
      {props.name.charAt(0)}
    </div>
  );
}
