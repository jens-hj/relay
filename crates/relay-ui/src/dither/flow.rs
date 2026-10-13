//! A dither field made entirely from grains born at its dense edge.

use super::{DitherSpec, Gradient};
use mosaic::prelude::*;
use mosaic::render::{Scene, Shape};

#[derive(Clone, Copy)]
struct Particle {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    lifetime: f32,
    offset: f32,
}

impl Particle {
    fn at(self, seconds: f64) -> (f32, f32, f32) {
        let age = (seconds + f64::from(self.offset)).rem_euclid(f64::from(self.lifetime)) as f32;
        let phase = age / self.lifetime;
        // Grains arrive at full strength. Exponential decay falls quickly
        // near the emitter, then tapers slowly; normalization ends at zero.
        let tail = (-4.0f32).exp();
        let alpha = ((-4.0 * phase).exp() - tail) / (1.0 - tail);
        (self.x + self.dx * age, self.y + self.dy * age, alpha)
    }
}

pub(super) struct Flow {
    particles: Vec<Particle>,
    size: f32,
}

impl Flow {
    pub(super) fn new(spec: &DitherSpec) -> Self {
        let scale = spec.scale.max(f32::EPSILON);
        let size = (1.0 / scale).max(0.7);
        let reach = (spec.width * spec.reach).min(spec.width).max(0.0);
        // More births overlap at the emitter without changing grain size.
        let count = ((reach * spec.height / (size * size) * 0.9) as usize).min(48_000);
        let mut particles = Vec::with_capacity(count);
        let mut state = spec.seed as u32;
        let mut random = || {
            state = state.wrapping_add(0x9e3779b9);
            let mut value = state;
            value = (value ^ (value >> 16)).wrapping_mul(0x21f0aaad);
            value = (value ^ (value >> 15)).wrapping_mul(0x735a2d97);
            (value ^ (value >> 15)) as f32 / u32::MAX as f32
        };
        for _ in 0..count {
            let speed = 5.0 + random() * 6.0;
            let y = random() * (spec.height - size).max(0.0);
            let (x, y, dx, dy, distance) = match spec.gradient {
                Gradient::Usage => ((reach - size).max(0.0), y, -speed, 0.0, reach),
                Gradient::Corner => {
                    let angle = random() * std::f32::consts::FRAC_PI_2;
                    let (sin, cos) = angle.sin_cos();
                    let distance = (spec.width * spec.reach)
                        .min((spec.width - size).max(0.0) / cos.max(0.001))
                        .min((spec.height - size).max(0.0) / sin.max(0.001));
                    (
                        0.0,
                        (spec.height - size).max(0.0),
                        speed * cos,
                        -speed * sin,
                        distance,
                    )
                }
                Gradient::Diagonal => (
                    0.0,
                    y,
                    speed * 0.8,
                    speed * 0.6,
                    reach.min((spec.height - size - y).max(0.0) / 0.75),
                ),
                _ => (0.0, y, speed, 0.0, reach),
            };
            // Different lifetimes reduce both density and opacity down the tail.
            // Many short-lived grains pack the dense edge, while a smaller
            // population of long-lived grains carries the sparse tail.
            let lifetime = (distance / speed * (0.03 + random().powi(2) * 0.97)).max(0.5);
            particles.push(Particle {
                x,
                y,
                dx,
                dy,
                lifetime,
                offset: random() * lifetime,
            });
        }
        Self { particles, size }
    }

    pub(super) fn paint(
        &self,
        scene: &mut Scene,
        rect: Rect,
        seconds: f64,
        tint: Color,
        opacity: f32,
    ) {
        for particle in &self.particles {
            let (x, y, alpha) = particle.at(seconds);
            // Keep even the one-pixel grains inside the component's bounds.
            if x < 0.0
                || y < 0.0
                || x + self.size > rect.size.width
                || y + self.size > rect.size.height
            {
                continue;
            }
            scene.shape(
                Shape::new(Rect::from_xywh(
                    rect.origin.x + x,
                    rect.origin.y + y,
                    self.size,
                    self.size,
                ))
                .fill(tint.alpha(opacity * alpha)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::tests::mount;
    use crate::tokens::status;
    use mosaic::render::PaintCmd;
    use std::time::Duration;

    #[test]
    fn animated_dither_contains_only_moving_particles_and_no_static_image() {
        for flowing in [false, true] {
            let size = Size::new(300.0, 40.0);
            let mounted = mount(size, || {
                view! {
                    Dither colour:(status.live) flowing:(flowing)
                }
            });
            let before = mounted.ui.scene().clone();
            mounted.ui.tick(Duration::from_millis(500));
            mounted.ui.frame(size, 1.0);
            let after = mounted.ui.scene().clone();
            assert_eq!(before != after, flowing);
            let has_image = |scene: &Scene| {
                scene
                    .cmds
                    .iter()
                    .any(|cmd| matches!(cmd, PaintCmd::Image { .. }))
            };
            assert_eq!(has_image(&before), !flowing);
            assert_eq!(has_image(&after), !flowing);
        }
    }

    #[test]
    fn emitting_edge_is_almost_fully_covered() {
        let flow = Flow::new(&DitherSpec::new(
            300.0,
            40.0,
            1.0,
            Gradient::Horizontal,
            1.0,
        ));
        for seconds in [0.0, 1.0, 4.0] {
            let mut covered = 0;
            for row in 2..38 {
                let y = row as f32 + 0.5;
                if flow.particles.iter().any(|particle| {
                    let (x, py, alpha) = particle.at(seconds);
                    alpha > 0.02 && x <= 1.5 && x + flow.size > 1.5 && py <= y && py + flow.size > y
                }) {
                    covered += 1;
                }
            }
            assert!(
                covered >= 35,
                "dense edge covers {covered}/36 samples at {seconds}s"
            );
        }
    }

    #[test]
    fn fade_drops_fast_at_first_and_tapers_into_a_long_tail() {
        let particle = Particle {
            x: 0.0,
            y: 0.0,
            dx: 1.0,
            dy: 0.0,
            lifetime: 4.0,
            offset: 0.0,
        };
        let levels: Vec<_> = (0..=4)
            .map(|step| {
                if step == 4 {
                    particle.at(3.99999).2
                } else {
                    particle.at(f64::from(step)).2
                }
            })
            .collect();
        assert_eq!(levels[0], 1.0);
        assert!(levels[1] < 0.4 && levels[2] > 0.1);
        assert!(levels[4] < 0.00001);
        let drops: Vec<_> = levels.windows(2).map(|pair| pair[0] - pair[1]).collect();
        assert!(drops.windows(2).all(|pair| pair[0] > pair[1]));
    }

    #[test]
    fn grains_follow_density_direction_and_fade_before_restarting() {
        for gradient in [Gradient::Horizontal, Gradient::Usage, Gradient::Corner] {
            let flow = Flow::new(&DitherSpec::new(300.0, 40.0, 1.0, gradient, 1.0));
            assert!(!flow.particles.is_empty());
            assert!(flow.particles.len() <= 48_000);
            for particle in flow.particles {
                match gradient {
                    Gradient::Usage => assert!(particle.dx < 0.0),
                    Gradient::Corner => assert!(particle.dx >= 0.0 && particle.dy <= 0.0),
                    _ => assert!(particle.dx > 0.0),
                }
                assert_eq!(particle.at(-f64::from(particle.offset)).2, 1.0);
                assert!(
                    particle
                        .at(f64::from(particle.lifetime) * 0.999 - f64::from(particle.offset))
                        .2
                        < 0.0001
                );
                let (x, y, _) =
                    particle.at(f64::from(particle.lifetime) * 0.5 - f64::from(particle.offset));
                assert!((x - particle.x - particle.dx * particle.lifetime * 0.5).abs() < 0.001);
                assert!((y - particle.y - particle.dy * particle.lifetime * 0.5).abs() < 0.001);
            }
        }
    }
}
