# ZAION Website - Multi-File Architecture

> Archived 2026-07-13. The standalone public website was intentionally
> retired; Zaion's supported browser surface is the Rust gateway `/ui`.

## Tech Stack

| Layer | Tech | Why |
|-------|------|-----|
| Bundler | Vite 6 | HMR dev, static build output, native ES module, zero config |
| Renderer | Three.js r170 | WebGL engine, EffectComposer, InstancedMesh |
| Animation | GSAP 3.12 + ScrollTrigger | Scroll-to-uniform mapping |
| Shaders | GLSL ES 1.0 (.glsl files) | Vite raw import, no fetch() hack |
| Style | Vanilla CSS | No Tailwind/CSS-in-JS overhead |
| Font | Geist (Google CDN) | Per requirement |
| Deploy | Static files (npm run build) | Any CDN / static host |

## Directory Structure

zaion-website/
  index.html                    Entry point
  package.json                  Vite + dependencies
  vite.config.js                GLSL plugin + build config
  css/
    main.css                    Base, Geist, layout, overlays
    animations.css              Keyframes for HTML elements
  js/
    main.js                     Bootstrap: renderer, scene, render loop
    camera.js                   Bezier camera path, lookAt interpolation
    scroll.js                   ScrollTrigger -> progress uniform driver
    cursor.js                   Custom glow cursor
    device.js                   Tier detection + performance scaling
    scenes/
      genesis.js                Scene 1: particles + title
      architecture.js           Scene 2: 6 totems + camera dolly
      immersion.js              Scene 3: ring surfaces + neural shader
      evolution.js              Scene 4: risograph + 3D typography
      singularity.js            Scene 5: core + ouroboros + trinity
    components/
      particles.js              InstancedMesh curl-noise particle system
      text3d.js                 Canvas-to-texture text sprites
      physics.js                Spring physics for HTML text
      background.js             Fullscreen quad background shader manager
    postprocessing/
      bloom.js                  UnrealBloomPass config
      grain.js                  Custom film grain pass
  shaders/
    background.vert
    background.frag             Scene-specific blends by uProgress
    particles.vert              Curl noise displacement
    particles.frag              Point sprite with glow
    totem.vert
    totem.frag                  Chrome/glass/emissive per uType
    ring.vert
    ring.frag                   Neural pathway + water ripple
    risograph.frag              Halftone CMYK noise
    core.vert
    core.frag                   Inner glow + hex grid
  assets/
    favicon.svg

## Data Flow

scroll position
    |
    v
scroll.js (ScrollTrigger scrub:1.5)
    |
    v
progress (0.0 -> 1.0)
    |
    +---> background.js: switches shader blends per scene
    +---> camera.js: bezier position + lookAt
    +---> particles.js: uProgress uniform
    +---> scenes/*.js: each scene reads progress range
    +---> postprocessing: bloom intensity, grain amount

## Loading Strategy

Phase 1 (instant): HTML + CSS + JS modules (ESM, parallel)
Phase 2 (<1s):     Three.js + GSAP from CDN or bundled
Phase 3 (async):   Shaders compile, particles init
Phase 4 (onload):  Render loop starts, loading screen fades out

## Build Output

npm run build -> dist/
  index.html    (~2KB)
  assets/
    index-*.js  (~150KB gzipped, Three.js included)
    index-*.css (~5KB)

Total: ~160KB gzipped. Zero external requests except Geist font.

## Dev vs Prod

Dev:  vite dev (localhost:5173, HMR, instant shader reload)
Prod: vite build -> dist/ (tree-shaken, minified, hashed)

Version 1.0 | 2026-04-17
