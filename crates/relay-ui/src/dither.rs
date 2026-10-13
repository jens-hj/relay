//! Dither: thresholded fractal noise over a gradient, flooded with one
//! colour. Density is the value, so it appears only where something is
//! happening or filling up.
//!
//! The noise is the SVG `feTurbulence` fractal-noise algorithm (the
//! reference lattice, gradients and seeded generator), so a seed draws the
//! same grain as the design boards. Each pixel is on when
//! `gradient_alpha + 2·noise − 1 ≥ ½`, the boards' arithmetic composite
//! followed by a two-step discrete transfer.
//!
//! Masks are generated on the CPU at physical pixel size (the image sampler
//! is linear-only, so drawing at device resolution is what keeps the grain
//! crisp), cached per element by size, seed, gradient and colour, and
//! regenerated when any of those change.

use crate::Live;
use crate::motion::{Breath, breathe};
use crate::tokens::{Signal, ground, srgb_alpha};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::render::ImageData;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// How the dither's density falls off across its reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gradient {
    /// Dense at the left edge, gone at the reach: 0.62 → 0 ("gW", live rows
    /// and the session header).
    Horizontal,
    /// The amber approval wash: 0.6 → 0.08 ("gWa").
    Amber,
    /// Usage bars: sparse at the start, densest at the crisp end that marks
    /// the value: 0.22 → 0.95.
    Usage,
    /// Diagonal from the top-left: 0.55 → 0 by 70% ("gH").
    Diagonal,
    /// The model glow: radial from the lower-left corner, 0.8 → 0.35 → 0.
    /// The reach is the radius as a fraction of the width.
    Corner,
}

impl Gradient {
    /// Source alpha at `(u, v)`, the position inside the reach box in 0..1.
    pub fn alpha(self, u: f32, v: f32) -> f32 {
        match self {
            Gradient::Horizontal => 0.62 * (1.0 - u),
            Gradient::Amber => 0.6 - 0.52 * u,
            Gradient::Usage => 0.22 + 0.73 * u,
            Gradient::Diagonal => {
                let t = (u + v) / 2.0;
                if t >= 0.7 {
                    0.0
                } else {
                    0.55 * (1.0 - t / 0.7)
                }
            }
            Gradient::Corner => {
                // `u` runs over the radius; `v` is measured up from the
                // bottom edge in the same unit.
                let t = (u * u + v * v).sqrt();
                if t >= 1.0 {
                    0.0
                } else if t <= 0.45 {
                    0.8 - (0.8 - 0.35) * t / 0.45
                } else {
                    0.35 * (1.0 - (t - 0.45) / 0.55)
                }
            }
        }
    }
}

/// Wash reach for a live row: 22% of the row plus 13% per effort level
/// (1-based), capped at the full width.
pub fn live_reach(effort_level: u8) -> f32 {
    (0.22 + 0.13 * effort_level as f32).min(1.0)
}

/// The SVG turbulence lattice for one seed.
pub struct Turbulence {
    lattice: [usize; LATTICE * 2 + 2],
    gradient: Box<[[[f64; 2]; LATTICE * 2 + 2]; 4]>,
}

const LATTICE: usize = 0x100;
const MASK: usize = 0xff;
const PERLIN_N: f64 = 4096.0;
const RAND_M: i64 = 2147483647;
const RAND_A: i64 = 16807;
const RAND_Q: i64 = 127773;
const RAND_R: i64 = 2836;

fn random(seed: i64) -> i64 {
    let result = RAND_A * (seed % RAND_Q) - RAND_R * (seed / RAND_Q);
    if result <= 0 { result + RAND_M } else { result }
}

impl Turbulence {
    /// Builds the lattice exactly as the SVG reference `init` does.
    pub fn new(seed: i32) -> Self {
        let mut seed = seed as i64;
        if seed <= 0 {
            seed = -(seed % (RAND_M - 1)) + 1;
        }
        if seed > RAND_M - 1 {
            seed = RAND_M - 1;
        }
        let mut lattice = [0usize; LATTICE * 2 + 2];
        let mut gradient = Box::new([[[0.0f64; 2]; LATTICE * 2 + 2]; 4]);
        for channel in gradient.iter_mut() {
            for (i, cell) in channel.iter_mut().take(LATTICE).enumerate() {
                lattice[i] = i;
                for component in cell.iter_mut() {
                    seed = random(seed);
                    *component =
                        ((seed % (LATTICE as i64 * 2)) - LATTICE as i64) as f64 / LATTICE as f64;
                }
                let length = (cell[0] * cell[0] + cell[1] * cell[1]).sqrt();
                if length > 0.0 {
                    cell[0] /= length;
                    cell[1] /= length;
                }
            }
        }
        let mut i = LATTICE - 1;
        while i > 0 {
            let k = lattice[i];
            seed = random(seed);
            let j = (seed % LATTICE as i64) as usize;
            lattice[i] = lattice[j];
            lattice[j] = k;
            i -= 1;
        }
        for i in 0..LATTICE + 2 {
            lattice[LATTICE + i] = lattice[i];
            for channel in gradient.iter_mut() {
                channel[LATTICE + i] = channel[i];
            }
        }
        Turbulence { lattice, gradient }
    }

    fn noise2(&self, channel: usize, x: f64, y: f64) -> f64 {
        let s_curve = |t: f64| t * t * (3.0 - 2.0 * t);
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let t = x + PERLIN_N;
        let bx0 = (t as usize) & MASK;
        let bx1 = (bx0 + 1) & MASK;
        let rx0 = t - t.trunc();
        let rx1 = rx0 - 1.0;
        let t = y + PERLIN_N;
        let by0 = (t as usize) & MASK;
        let by1 = (by0 + 1) & MASK;
        let ry0 = t - t.trunc();
        let ry1 = ry0 - 1.0;
        let i = self.lattice[bx0];
        let j = self.lattice[bx1];
        let b00 = self.lattice[i + by0];
        let b10 = self.lattice[j + by0];
        let b01 = self.lattice[i + by1];
        let b11 = self.lattice[j + by1];
        let sx = s_curve(rx0);
        let sy = s_curve(ry0);
        let g = &self.gradient[channel];
        let u = rx0 * g[b00][0] + ry0 * g[b00][1];
        let v = rx1 * g[b10][0] + ry0 * g[b10][1];
        let a = lerp(sx, u, v);
        let u = rx0 * g[b01][0] + ry1 * g[b01][1];
        let v = rx1 * g[b11][0] + ry1 * g[b11][1];
        let b = lerp(sx, u, v);
        lerp(sy, a, b)
    }

    /// Fractal-noise alpha (channel 3) at a user-space point, in 0..1.
    pub fn fractal_alpha(&self, x: f64, y: f64, frequency: f64, octaves: u32) -> f64 {
        let (mut vx, mut vy) = (x * frequency, y * frequency);
        let mut sum = 0.0;
        let mut ratio = 1.0;
        for _ in 0..octaves {
            sum += self.noise2(3, vx, vy) / ratio;
            vx *= 2.0;
            vy *= 2.0;
            ratio *= 2.0;
        }
        ((sum + 1.0) / 2.0).clamp(0.0, 1.0)
    }
}

/// What a dither mask is generated from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DitherSpec {
    /// Logical size of the element.
    pub width: f32,
    pub height: f32,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    pub seed: i32,
    pub gradient: Gradient,
    /// Fraction of width spanned; a corner radius may exceed the width.
    pub reach: f32,
    /// Noise base frequency per logical pixel.
    pub frequency: f32,
}

impl DitherSpec {
    pub fn new(width: f32, height: f32, scale: f32, gradient: Gradient, reach: f32) -> Self {
        DitherSpec {
            width,
            height,
            scale,
            seed: 7,
            gradient,
            reach,
            frequency: if gradient == Gradient::Corner {
                0.9
            } else {
                0.85
            },
        }
    }

    /// Physical pixel dimensions of the mask.
    pub fn pixels(&self) -> (u32, u32) {
        (
            (self.width * self.scale).round().max(0.0) as u32,
            (self.height * self.scale).round().max(0.0) as u32,
        )
    }

    /// The thresholded mask, row-major, one `bool` per physical pixel.
    pub fn mask(&self) -> Vec<bool> {
        let (pw, ph) = self.pixels();
        let mut out = vec![false; pw as usize * ph as usize];
        if pw == 0 || ph == 0 || self.reach <= 0.0 {
            return out;
        }
        let noise = Turbulence::new(self.seed);
        // A narrow model cell needs a radius larger than its width for the
        // lower-left glow to rise into the family name. Linear washes still
        // stop at the full width; the corner mask remains clipped to its cell.
        let reach = if self.gradient == Gradient::Corner {
            self.reach.max(0.0)
        } else {
            self.reach.clamp(0.0, 1.0)
        };
        let reach_px = (self.width * reach).max(f32::EPSILON);
        let scale = self.scale.max(f32::EPSILON);
        for py in 0..ph {
            let y = (py as f32 + 0.5) / scale;
            for px_ in 0..pw {
                let x = (px_ as f32 + 0.5) / scale;
                let (u, v) = if self.gradient == Gradient::Corner {
                    (x / reach_px, (self.height - y) / reach_px)
                } else {
                    (x / reach_px, y / self.height.max(f32::EPSILON))
                };
                if u > 1.0 && self.gradient != Gradient::Corner {
                    continue;
                }
                let source = self.gradient.alpha(u.min(1.0), v).max(0.0);
                if source <= 0.0 {
                    continue;
                }
                let n = noise.fractal_alpha(x as f64, y as f64, self.frequency as f64, 2) as f32;
                let value = (source + 2.0 * n - 1.0).clamp(0.0, 1.0);
                out[py as usize * pw as usize + px_ as usize] = value >= 0.5;
            }
        }
        out
    }

    /// The mask flooded with `tint` at `opacity`, as straight-alpha RGBA.
    pub fn image(&self, tint: Color, opacity: f32) -> ImageData {
        let (pw, ph) = self.pixels();
        let [r, g, b, a] = tint.to_srgb8();
        let alpha = (a as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
        let mut data = vec![0u8; pw as usize * ph as usize * 4];
        for (index, on) in self.mask().into_iter().enumerate() {
            if on {
                data[index * 4..index * 4 + 4].copy_from_slice(&[r, g, b, alpha]);
            }
        }
        ImageData::new(pw, ph, data.into_boxed_slice())
    }
}

/// The last generated mask of a dither element.
type CachedMask = (CacheKey, Arc<ImageData>);

#[derive(Clone, Copy, PartialEq)]
struct CacheKey {
    pixels: (u32, u32),
    spec: DitherSpec,
    tint: [u8; 4],
    opacity: f32,
}

/// A dithered fill covering its parent (place it first in a `stack`).
/// Purely decorative: it never hit-tests.
#[component]
pub fn Dither(
    /// Flood colour.
    colour: ColorToken,
    /// Density fall-off.
    #[prop(default = Gradient::Horizontal)]
    gradient: Gradient,
    /// Fraction of the width the dither reaches, 0..1 for linear washes.
    /// Corner glows may have a radius larger than their cell's width.
    #[prop(default = Live::fixed(1.0))]
    reach: Live<f32>,
    /// Overall opacity: row 0.3, header 0.14, approval 0.22.
    #[prop(default = 0.3)]
    opacity: f32,
    /// The ground it sits on. Opacity is matched to an sRGB blend over
    /// this colour, as the design specifies it.
    #[prop(default = ground.panel)]
    over: ColorToken,
    /// Noise seed; the same seed draws the same grain.
    #[prop(default = 7)]
    seed: i32,
    /// Breathes the opacity (the model glow).
    #[prop(optional)]
    breath: Option<Breath>,
) -> Element {
    let wash = view! {
        el width:fill height:fill nohit {}
    };
    let level = breath.map(|breath| breathe(&wash, breath));
    {
        let target = wash.clone();
        let reach = reach.clone();
        Effect::new(move || {
            let _ = reach.get();
            target.paint_dirty();
        });
    }
    let cache: Rc<RefCell<Option<CachedMask>>> = Rc::new(RefCell::new(None));
    wash.paint(move |ctx| {
        let rect = ctx.rect;
        let mut spec = DitherSpec::new(
            rect.size.width,
            rect.size.height,
            ctx.scale,
            gradient,
            reach.get_untracked(),
        );
        spec.seed = seed;
        let tint = color(colour);
        let under = color(over);
        let opacity = srgb_alpha(tint, under, opacity);
        let key = CacheKey {
            pixels: spec.pixels(),
            spec,
            tint: tint.to_srgb8(),
            opacity,
        };
        if key.pixels.0 == 0 || key.pixels.1 == 0 {
            return;
        }
        let image = {
            let mut slot = cache.borrow_mut();
            match slot.as_ref() {
                Some((cached, image)) if *cached == key => image.clone(),
                _ => {
                    let image = Arc::new(spec.image(tint, opacity));
                    *slot = Some((key, image.clone()));
                    image
                }
            }
        };
        let source = Rect::from_xywh(0.0, 0.0, key.pixels.0 as f32, key.pixels.1 as f32);
        match &level {
            Some(level) => {
                ctx.scene.push_layer(srgb_alpha(tint, under, level.get()));
                ctx.scene.image(image, source, rect);
                ctx.scene.pop_layer();
            }
            None => {
                ctx.scene.image(image, source, rect);
            }
        }
    });
    wash
}

/// A status wash: violet for live work (horizontal fall-off), amber for an
/// approval (the amber gradient). Covers its parent; never hit-tests.
#[component]
pub fn Wash(
    /// Live (violet) or NeedsYou (amber).
    #[prop(default = Signal::Live)]
    signal: Signal,
    /// Fraction of the width reached; use [`live_reach`] for effort.
    #[prop(default = Live::fixed(1.0))]
    reach: Live<f32>,
    /// Row 0.3 (default), header 0.14, approval 0.22.
    #[prop(default = 0.3)]
    opacity: f32,
    /// The ground it sits on (see [`Dither`]).
    #[prop(default = ground.panel)]
    over: ColorToken,
) -> Element {
    let (gradient, seed) = match signal {
        Signal::NeedsYou => (Gradient::Amber, 3),
        _ => (Gradient::Horizontal, 7),
    };
    view! {
        Dither colour:(signal.token()) gradient:(gradient) reach:(reach) opacity:(opacity)
            seed:(seed) over:(over)
    }
}
