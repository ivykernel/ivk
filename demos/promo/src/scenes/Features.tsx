import React from 'react';
import {AbsoluteFill} from 'remotion';
import {FadeUp} from '../bits';
import {T} from '../theme';

const FEATURES: Array<{head: string; detail: string}> = [
  {
    head: '5.4× faster workspace creation',
    detail: 'than git worktree — measured, reproducible benchmarks',
  },
  {
    head: 'Copy-on-write materialization',
    detail: 'APFS clonefile(2) on macOS · FICLONE on Linux btrfs/xfs/zfs',
  },
  {
    head: 'Crash-safe kernel state',
    detail: 'SQLite registry + intent journal — ivk doctor --repair after any kill -9',
  },
  {
    head: 'Agent-readable by design',
    detail: '--json / --agent output with next_command on every step',
  },
];

export const Features: React.FC = () => (
  <AbsoluteFill style={{justifyContent: 'center', padding: '0 240px'}}>
    <FadeUp delay={0}>
      <div
        style={{
          fontFamily: T.sans,
          fontWeight: 800,
          fontSize: 72,
          color: T.ink,
          marginBottom: 70,
        }}
      >
        Built as a kernel, not a script.
      </div>
    </FadeUp>
    {FEATURES.map((f, i) => (
      <FadeUp key={f.head} delay={18 + i * 16}>
        <div
          style={{
            display: 'flex',
            alignItems: 'baseline',
            gap: 30,
            marginBottom: 44,
          }}
        >
          <div
            style={{
              fontFamily: T.mono,
              fontSize: 44,
              color: T.greenBright,
              fontWeight: 700,
            }}
          >
            ▸
          </div>
          <div>
            <div
              style={{
                fontFamily: T.sans,
                fontWeight: 700,
                fontSize: 52,
                color: T.ink,
              }}
            >
              {f.head}
            </div>
            <div
              style={{
                fontFamily: T.mono,
                fontSize: 30,
                color: T.inkDim,
                marginTop: 8,
              }}
            >
              {f.detail}
            </div>
          </div>
        </div>
      </FadeUp>
    ))}
  </AbsoluteFill>
);
