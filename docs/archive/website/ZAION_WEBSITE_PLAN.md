# ZAION Official Website - Master Plan

> Archived 2026-07-13. The standalone public website was intentionally
> retired; this file is retained only as historical design evidence.

## I. Concept
Awakening of the Autonomous Mind - a journey from void to consciousness.

### Visual Metaphor Mapping

| Zaion Concept | Visual | Scene |
|---|---|---|
| Ed25519 Identity | Metallic particle glyph | Scene 1 |
| Hash-chain Ledger | Fractal 3D chain links | Scene 2 |
| Ego | Sphere with iris tracking mouse | Scene 2 |
| Metabolic/Curiosity | Firefly particles | Scene 3 |
| Self-evolution | Morphing geometry | Scene 4 |
| Ouroboros Self-healing | Torus knot serpent | Scene 5 |
| Trinity | Three orbiting orbs | Scene 5 |

## II. Architecture: Single-File SPA
- index.html self-contained, zero build step
- CDN: Three.js r180, GSAP 3.14.2, ScrollTrigger, Geist font
- InstancedMesh particles, procedural shaders, no texture files

## III. Scenes (600vh total)

### Scene 1: Genesis (0-100vh)
Black void + ZAION metallic title + 50k curl-noise particles
Mouse drives particle swirl, spring physics on letters

### Scene 2: Architecture (100-300vh)
6 totem containers: Identity/Memory/Ledger/Ego/Evolve/Trinity
Camera dolly path, hover detection, elastic scale

### Scene 3: Immersion (300-400vh)
Curved ring surfaces + neural pathway shader + water ripple

### Scene 4: Evolution (400-500vh)
Risograph halftone background + 3D extruded numbers
Spring physics text (Resn-style stretch/squeeze)

### Scene 5: Singularity (500-600vh)
Convergence to rotating core + Ouroboros torus knot + Trinity orbitals
Watermark: 10px, letter-spacing 0.2em, rgba(255,255,255,0.15)

## IV. Performance
60fps / <15 draw calls / <200MB GPU / <200KB file / <2s load

## V. Phases
1. Foundation (HTML+Renderer+Scroll+Camera+Particles)
2. Genesis (Title+Curl noise+Mouse+Fracture)
3. Architecture (Totems+Dolly+Hover+Panels)
4. Immersion (Rings+Neural shader+Ripple)
5. Evolution (Risograph+Typography+Physics)
6. Singularity (Core+Ouroboros+Trinity+Watermark)
7. Polish (Cursor+Loader+Mobile+Browser tests)

Version 1.0 | 2026-04-17 | Ready for Implementation
