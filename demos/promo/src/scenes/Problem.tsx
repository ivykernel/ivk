import React from 'react';
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {Bar, FadeUp} from '../bits';
import {T} from '../theme';

const COLS = 20;
const ROWS = 5;

/** 100 agent dots popping in, then the disk-cost bar. */
export const Problem: React.FC = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();

  const gb = interpolate(frame, [120, 185], [0, 64.85], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: (t) => 1 - Math.pow(1 - t, 3),
  });

  return (
    <AbsoluteFill
      style={{justifyContent: 'center', alignItems: 'center', padding: 120}}
    >
      <FadeUp delay={0}>
        <div
          style={{
            fontFamily: T.sans,
            fontWeight: 800,
            fontSize: 76,
            color: T.ink,
            textAlign: 'center',
          }}
        >
          100 AI agents. One repo.
        </div>
      </FadeUp>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: `repeat(${COLS}, 40px)`,
          gap: 18,
          marginTop: 70,
          marginBottom: 80,
        }}
      >
        {Array.from({length: COLS * ROWS}, (_, i) => {
          const pop = spring({
            frame: frame - 14 - i * 0.55,
            fps,
            config: {damping: 14, stiffness: 180},
          });
          return (
            <div
              key={i}
              style={{
                width: 40,
                height: 40,
                borderRadius: 10,
                background: T.surface,
                border: `2px solid ${T.line}`,
                transform: `scale(${pop})`,
                boxShadow: `inset 0 0 0 3px rgba(61,220,151,${0.12 * pop})`,
              }}
            />
          );
        })}
      </div>

      <FadeUp delay={95}>
        <div
          style={{
            fontFamily: T.sans,
            fontSize: 44,
            color: T.inkDim,
            marginBottom: 60,
            textAlign: 'center',
          }}
        >
          Each one needs an isolated working tree. With plain{' '}
          <span style={{fontFamily: T.mono, color: T.ink}}>git worktree</span>,
          that costs you:
        </div>
      </FadeUp>

      <div style={{width: 1500}}>
        <Bar
          label="git worktree × 100"
          value={`${gb.toFixed(2)} GB`}
          width={1200}
          color={T.blue}
          grow={{from: 120, over: 65}}
        />
      </div>
    </AbsoluteFill>
  );
};
