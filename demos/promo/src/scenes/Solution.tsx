import React from 'react';
import {AbsoluteFill, interpolate, useCurrentFrame} from 'remotion';
import {Bar, FadeUp, Terminal, TypeLine} from '../bits';
import {T} from '../theme';

const CMD = 'ivk new attempt-{1..100}';
const TYPED_AT = 10 + Math.ceil(CMD.length / 1.1); // frame when typing finishes

/** Terminal demo, then the disk comparison with the 65× headline. */
export const Solution: React.FC = () => {
  const frame = useCurrentFrame();

  const gb = interpolate(frame, [120, 165], [0, 1.0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  const headline = interpolate(frame, [170, 186], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <AbsoluteFill
      style={{justifyContent: 'center', alignItems: 'center', padding: 120}}
    >
      <FadeUp delay={0}>
        <Terminal width={1500}>
          <TypeLine text={CMD} startFrame={10} fontSize={44} />
          <div
            style={{
              fontFamily: T.mono,
              fontSize: 36,
              lineHeight: 1.6,
              color: T.inkDim,
              opacity: interpolate(
                frame,
                [TYPED_AT + 8, TYPED_AT + 18],
                [0, 1],
                {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'},
              ),
            }}
          >
            <span style={{color: T.greenBright}}>✓</span> created 100
            workspaces&nbsp;&nbsp;·&nbsp;&nbsp;strategy=apfs-clonefile
            &nbsp;&nbsp;·&nbsp;&nbsp;50 s
          </div>
        </Terminal>
      </FadeUp>

      <div style={{width: 1500, marginTop: 90}}>
        <Bar
          label="git worktree × 100"
          value="64.85 GB"
          width={1200}
          color={T.blue}
          grow={{from: 95, over: 1}}
        />
        <Bar
          label="ivk × 100 — block-shared via copy-on-write"
          value={`${gb.toFixed(2)} GB`}
          width={19}
          color={T.green}
          grow={{from: 120, over: 40}}
          emphasized
        />
      </div>

      <div
        style={{
          marginTop: 40,
          fontFamily: T.sans,
          fontWeight: 800,
          fontSize: 110,
          color: T.greenBright,
          opacity: headline,
          transform: `scale(${0.9 + headline * 0.1})`,
        }}
      >
        65× less disk
      </div>
    </AbsoluteFill>
  );
};
