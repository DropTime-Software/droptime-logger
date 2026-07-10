/**
 * The wizard's progress rail: numbered pills joined by a connector, with labels
 * on wider surfaces. Completed steps fill forest with a check; the current step
 * wears the lemon accent; upcoming steps stay quiet. Styled to the house
 * language — no new global CSS.
 */
import { Check } from '@phosphor-icons/react';

export interface StepperItem {
  id: string;
  label: string;
}

export function Stepper({
  steps,
  currentIndex,
}: {
  steps: StepperItem[];
  currentIndex: number;
}) {
  return (
    <ol className="flex w-full items-center" aria-label="Setup progress">
      {steps.map((step, i) => {
        const state = i < currentIndex ? 'done' : i === currentIndex ? 'current' : 'todo';
        const isLast = i === steps.length - 1;
        return (
          <li
            key={step.id}
            className="flex items-center"
            style={{ flex: isLast ? '0 0 auto' : '1 1 0%' }}
            aria-current={state === 'current' ? 'step' : undefined}
          >
            <div className="flex flex-col items-center gap-1.5">
              <span
                className="flex h-8 w-8 items-center justify-center rounded-full text-[13px] font-semibold tabular-nums transition-colors"
                style={pillStyle(state)}
              >
                {state === 'done' ? <Check size={15} weight="bold" /> : i + 1}
              </span>
              <span
                className="hidden text-[11px] font-semibold tracking-[-0.02em] sm:block"
                style={{
                  color:
                    state === 'todo' ? 'rgba(36,36,36,0.4)' : 'var(--color-forest)',
                }}
              >
                {step.label}
              </span>
            </div>
            {!isLast ? (
              <span
                className="mx-2 mb-4 h-[2px] flex-1 rounded-full transition-colors sm:mx-3"
                style={{
                  background:
                    i < currentIndex ? 'var(--color-forest)' : 'var(--color-line2)',
                }}
              />
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}

function pillStyle(state: 'done' | 'current' | 'todo'): React.CSSProperties {
  switch (state) {
    case 'done':
      return { background: 'var(--color-forest)', color: '#ffffff' };
    case 'current':
      return {
        background: 'var(--color-lemon)',
        color: 'var(--color-forest)',
        boxShadow: '0 0 0 4px rgba(218,246,152,0.35)',
      };
    default:
      return {
        background: '#ffffff',
        color: 'rgba(36,36,36,0.45)',
        boxShadow: '0 0 0 1.5px var(--color-line2) inset',
      };
  }
}
