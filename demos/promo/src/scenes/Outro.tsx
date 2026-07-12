import React from 'react';
import {AbsoluteFill} from 'remotion';
import {FadeUp, Terminal, TypeLine} from '../bits';
import {T} from '../theme';

export const Outro: React.FC = () => (
  <AbsoluteFill
    style={{justifyContent: 'center', alignItems: 'center', textAlign: 'center'}}
  >
    <FadeUp delay={0}>
      <div
        style={{
          fontFamily: T.mono,
          fontWeight: 800,
          fontSize: 150,
          color: T.greenBright,
          letterSpacing: 6,
        }}
      >
        ivk
      </div>
    </FadeUp>
    <FadeUp delay={14}>
      <div
        style={{
          fontFamily: T.sans,
          fontSize: 42,
          color: T.inkDim,
          marginTop: 10,
          marginBottom: 70,
        }}
      >
        100 agents · 1 repo · workspaces with a lifecycle
      </div>
    </FadeUp>
    <FadeUp delay={26}>
      <Terminal width={1360} title="get started">
        <TypeLine
          text="brew tap ivykernel/tap && brew install ivk"
          startFrame={34}
          charsPerFrame={1.4}
          fontSize={40}
        />
      </Terminal>
    </FadeUp>
    <FadeUp delay={95}>
      <div
        style={{
          marginTop: 66,
          fontFamily: T.mono,
          fontSize: 36,
          color: T.ink,
          letterSpacing: 2,
        }}
      >
        ivykernel.github.io/ivk
      </div>
    </FadeUp>
  </AbsoluteFill>
);
