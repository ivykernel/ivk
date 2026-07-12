import React from 'react';
import {
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {T} from './theme';

/** Spring-in from below with fade. `delay` in frames, relative to the enclosing Sequence. */
export const FadeUp: React.FC<{
  delay?: number;
  distance?: number;
  children: React.ReactNode;
  style?: React.CSSProperties;
}> = ({delay = 0, distance = 44, children, style}) => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const p = spring({
    frame: frame - delay,
    fps,
    config: {damping: 200, stiffness: 120},
  });
  return (
    <div
      style={{
        opacity: p,
        transform: `translateY(${(1 - p) * distance}px)`,
        ...style,
      }}
    >
      {children}
    </div>
  );
};

/** Fades the whole scene in over the first frames and out over the last ones. */
export const SceneFade: React.FC<{
  durationInFrames: number;
  children: React.ReactNode;
}> = ({durationInFrames, children}) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(
    frame,
    [0, 12, durationInFrames - 14, durationInFrames - 2],
    [0, 1, 1, 0],
    {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'},
  );
  return <AbsoluteFill style={{opacity}}>{children}</AbsoluteFill>;
};

/** Shared dark backdrop with a faint ivy glow. */
export const Backdrop: React.FC = () => (
  <AbsoluteFill
    style={{
      background: `radial-gradient(1100px 700px at 28% 18%, rgba(18,168,123,0.10), transparent 60%),
                   radial-gradient(900px 600px at 78% 85%, rgba(59,130,246,0.05), transparent 60%),
                   ${T.bg}`,
    }}
  />
);

/** Monospace line that types itself. Returns true-ish caret while typing. */
export const TypeLine: React.FC<{
  text: string;
  startFrame: number;
  charsPerFrame?: number;
  prompt?: string;
  fontSize?: number;
  color?: string;
}> = ({
  text,
  startFrame,
  charsPerFrame = 1.1,
  prompt = '$ ',
  fontSize = 44,
  color = T.ink,
}) => {
  const frame = useCurrentFrame();
  const chars = Math.max(0, Math.floor((frame - startFrame) * charsPerFrame));
  const shown = text.slice(0, chars);
  const done = chars >= text.length;
  const caretOn = Math.floor(frame / 16) % 2 === 0;
  return (
    <div
      style={{
        fontFamily: T.mono,
        fontSize,
        color,
        whiteSpace: 'pre',
        lineHeight: 1.5,
      }}
    >
      <span style={{color: T.greenBright}}>{prompt}</span>
      {shown}
      <span
        style={{
          display: 'inline-block',
          width: fontSize * 0.55,
          height: fontSize * 1.05,
          verticalAlign: 'text-bottom',
          background:
            frame >= startFrame && (!done || caretOn) ? T.inkDim : 'transparent',
          marginLeft: 4,
        }}
      />
    </div>
  );
};

/** Terminal window chrome. */
export const Terminal: React.FC<{
  width: number;
  children: React.ReactNode;
  title?: string;
}> = ({width, children, title = 'zsh — my-repo'}) => (
  <div
    style={{
      width,
      background: T.surface,
      border: `2px solid ${T.line}`,
      borderRadius: 18,
      overflow: 'hidden',
      boxShadow: '0 30px 80px rgba(0,0,0,0.45)',
    }}
  >
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '16px 22px',
        borderBottom: `2px solid ${T.line}`,
      }}
    >
      {['#FF5F57', '#FEBC2E', '#28C840'].map((c) => (
        <div
          key={c}
          style={{width: 16, height: 16, borderRadius: 8, background: c}}
        />
      ))}
      <div
        style={{
          marginLeft: 14,
          fontFamily: T.mono,
          fontSize: 22,
          color: T.inkFaint,
        }}
      >
        {title}
      </div>
    </div>
    <div style={{padding: '30px 36px'}}>{children}</div>
  </div>
);

/**
 * One horizontal comparison bar. Thin mark, rounded data end, direct label —
 * identity is carried by the label, never by color alone.
 */
export const Bar: React.FC<{
  label: string;
  value: string;
  width: number;
  color: string;
  grow: {from: number; over: number};
  emphasized?: boolean;
}> = ({label, value, width, color, grow, emphasized}) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [grow.from, grow.from + grow.over], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: (t) => 1 - Math.pow(1 - t, 3),
  });
  const w = p === 0 ? 0 : Math.max(10, width * p);
  // The whole row (label + mark) stays hidden until its growth begins, so
  // no stray labels or 10px stubs float around while earlier beats play.
  const rowOpacity = interpolate(frame, [grow.from - 10, grow.from], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  return (
    <div style={{marginBottom: 44, opacity: rowOpacity}}>
      <div
        style={{
          fontFamily: T.mono,
          fontSize: 30,
          color: T.inkDim,
          marginBottom: 14,
        }}
      >
        {label}
      </div>
      <div style={{display: 'flex', alignItems: 'center', gap: 26}}>
        <div
          style={{
            width: w,
            height: 42,
            background: color,
            borderRadius: 6,
          }}
        />
        <div
          style={{
            fontFamily: T.mono,
            fontSize: emphasized ? 44 : 36,
            fontWeight: 700,
            color: emphasized ? T.greenBright : T.ink,
            opacity: p > 0.15 ? 1 : p * 6,
            whiteSpace: 'nowrap',
          }}
        >
          {value}
        </div>
      </div>
    </div>
  );
};
