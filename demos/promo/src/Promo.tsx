import React from 'react';
import {AbsoluteFill, Sequence} from 'remotion';
import {Backdrop, SceneFade} from './bits';
import {Features} from './scenes/Features';
import {Outro} from './scenes/Outro';
import {Problem} from './scenes/Problem';
import {Solution} from './scenes/Solution';
import {Title} from './scenes/Title';

// 30 fps. Total: 1080 frames = 36 s.
export const SCENES = [
  {name: 'title', from: 0, duration: 150, node: <Title />},
  {name: 'problem', from: 150, duration: 240, node: <Problem />},
  {name: 'solution', from: 390, duration: 270, node: <Solution />},
  {name: 'features', from: 660, duration: 210, node: <Features />},
  {name: 'outro', from: 870, duration: 210, node: <Outro />},
] as const;

export const TOTAL_FRAMES = 1080;

export const Promo: React.FC = () => (
  <AbsoluteFill>
    <Backdrop />
    {SCENES.map((s) => (
      <Sequence key={s.name} from={s.from} durationInFrames={s.duration}>
        <SceneFade durationInFrames={s.duration}>{s.node}</SceneFade>
      </Sequence>
    ))}
  </AbsoluteFill>
);
