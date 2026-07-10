/**
 * INTERNAL. Self-contained keyframes for the console's two motion cues
 * (leading-edge ping, current-phase pulse). Shipped as an inline <style> so
 * the animations work regardless of the consumer's Tailwind configuration;
 * duplicate mounts re-declare identical rules, which is harmless.
 * Reduced motion is respected via the `rc-anim` escape hatch.
 */
export function RcKeyframes() {
  return (
    <style>{`
@keyframes rc-ping{0%{transform:translate(-50%,-50%) scale(1);opacity:.45}80%,100%{transform:translate(-50%,-50%) scale(2.6);opacity:0}}
@keyframes rc-pulse{0%,100%{opacity:1}50%{opacity:.55}}
@media (prefers-reduced-motion:reduce){.rc-anim{animation:none!important}}
`}</style>
  );
}
