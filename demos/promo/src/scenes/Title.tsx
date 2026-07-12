import React from 'react';
import {AbsoluteFill} from 'remotion';
import {FadeUp} from '../bits';
import {T} from '../theme';

export const Title: React.FC = () => (
  <AbsoluteFill
    style={{justifyContent: 'center', alignItems: 'center', textAlign: 'center'}}
  >
    <FadeUp delay={4}>
      <div
        style={{
          fontFamily: T.mono,
          fontSize: 30,
          letterSpacing: 14,
          color: T.greenBright,
          marginBottom: 46,
        }}
      >
        IVY&nbsp;KERNEL
      </div>
    </FadeUp>
    <FadeUp delay={16}>
      <div
        style={{
          fontFamily: T.sans,
          fontWeight: 800,
          fontSize: 96,
          color: T.ink,
          lineHeight: 1.16,
        }}
      >
        Git makes branches cheap.
      </div>
    </FadeUp>
    <FadeUp delay={40}>
      <div
        style={{
          fontFamily: T.sans,
          fontWeight: 800,
          fontSize: 96,
          color: T.greenBright,
          lineHeight: 1.16,
        }}
      >
        ivk makes workspaces cheap.
      </div>
    </FadeUp>
    <FadeUp delay={70}>
      <div
        style={{
          marginTop: 52,
          fontFamily: T.sans,
          fontSize: 40,
          color: T.inkDim,
        }}
      >
        A parallel workspace kernel for AI agents
      </div>
    </FadeUp>
  </AbsoluteFill>
);
