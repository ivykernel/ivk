# ivk promo video (Remotion)

36 s · 1920×1080 @ 30 fps · rendered artifact lives at
[`demos/ivk-promo.mp4`](../ivk-promo.mp4).

Five scenes: tagline → the 100-agents disk problem → `ivk new` + the 65×
comparison → kernel features → install CTA.

## Editing

```bash
cd demos/promo
npm install
npm run studio     # live-preview + scrub in the browser
```

Scene timings live in `src/Promo.tsx` (`SCENES`); copy, colors, and fonts in
`src/theme.ts` and `src/scenes/*.tsx`. The bar palette
(`#3B82F6` / `#12A87B`) is validated for CVD separation and contrast on the
dark surface — if you change it, re-validate.

## Rendering

```bash
npm run render     # → out/ivk-promo.mp4
cp out/ivk-promo.mp4 ../ivk-promo.mp4   # refresh the committed artifact
```

`out/` and `node_modules/` are git-ignored; only the source and the final
mp4 (one directory up) are committed.
