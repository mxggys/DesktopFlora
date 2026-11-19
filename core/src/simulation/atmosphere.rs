//! What the weather does to the screen itself rather than to what falls: frost growing in from the edges, glass misting
//! over that a quick swipe of the cursor wipes clear, drops left on the glass after rain drying away, and the moon in its
//! phase at night; and two kinds of drift, dandelion seeds floating in summer air and fine snow streaming along the
//! drifts in a strong wind.
//!
//! What only sits there, the frost and the mist once they have grown, and the moon, doesn't keep the screen redrawing.

use std::f32::consts::{PI, TAU};

use super::{Mode, Random, SnowSimulation, Sprite};
use crate::geometry::{V2, smoothstep};

const MEGAPOINT: f32 = 1_000_000.0;
/// How big a patch of mist is, in points: wiping clears whole patches.
const MIST_CELL: f32 = 34.0;
/// How long a wiped patch takes to mist over again, in seconds.
const MIST_REGROWTH: f32 = 45.0;
/// How long frost takes to grow all the way in, in seconds.
const FROST_GROWTH: f32 = 90.0;
const MAX_FROST: usize = 3000;
const MAX_DROPS: usize = 400;

/// What the app asks for: how far frost and mist cover the glass, 0 to 1, and the moon's phase while it shows, from new
/// (0) through full (0.5) and back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Atmosphere {
    pub frost: f32,
    pub mist: f32,
    pub moon: Option<f32>,
}

/// A point along a frost crystal, and how far along its growth it appears.
#[derive(Clone, Copy)]
struct Rime {
    position: V2,
    /// From 0 at the edge to 1 at its farthest reach.
    order: f32,
    size: f32,
    /// The way the crystal runs here, for its streak.
    along: V2,
}

#[derive(Clone, Copy)]
struct Droplet {
    position: V2,
    radius: f32,
    age: f32,
    life: f32,
}

#[derive(Clone, Copy)]
struct Seed {
    position: V2,
    velocity: V2,
    phase: f32,
    size: f32,
    shown: f32,
}

#[derive(Clone, Copy)]
struct Powder {
    position: V2,
    velocity: V2,
    age: f32,
    life: f32,
    size: f32,
}

pub(super) struct Weathering {
    random: Random,
    wanted: Atmosphere,
    /// How far the frost has grown in, and how far the moon shows, 0 to 1.
    frost: f32,
    rime: Vec<Rime>,
    rime_size: (f32, f32),
    mist: Vec<f32>,
    mist_columns: usize,
    mist_settled: bool,
    moon: f32,
    phase: f32,
    droplets: Vec<Droplet>,
    was_raining: f32,
    seeds: Vec<Seed>,
    powder: Vec<Powder>,
    last_cursor: Option<V2>,
}

impl Default for Weathering {
    fn default() -> Weathering {
        Weathering {
            random: Random::new(),
            wanted: Atmosphere::default(),
            frost: 0.0,
            rime: Vec::new(),
            rime_size: (0.0, 0.0),
            mist: Vec::new(),
            mist_columns: 0,
            mist_settled: true,
            moon: 0.0,
            phase: 0.0,
            droplets: Vec::new(),
            was_raining: 0.0,
            seeds: Vec::new(),
            powder: Vec::new(),
            last_cursor: None,
        }
    }
}

impl Weathering {
    fn frost_shown(&self) -> usize {
        if self.frost <= 0.0 { 0 } else { self.rime.iter().filter(|rime| rime.order <= self.frost).count() }
    }

    fn mist_shown(&self) -> usize {
        self.mist.iter().filter(|&&density| density > 0.02).count()
    }

    pub(super) fn sprite_count(&self) -> usize {
        self.frost_shown() + self.mist_shown() + if self.moon > 0.0 { 2 } else { 0 } + self.droplets.len() + self.seeds.len() * 2 + self.powder.len()
    }

    /// What is drawn but only sits there, and doesn't need the screen redrawn for its sake.
    pub(super) fn resting(&self) -> usize {
        let frost_still = self.frost == 0.0 || self.frost >= 1.0 || self.frost == self.wanted.frost.min(1.0);
        (if frost_still { self.frost_shown() } else { 0 })
            + if self.mist_settled { self.mist_shown() } else { 0 }
            + if self.moon >= 1.0 { 2 } else { 0 }
    }

    pub(super) fn lively(&self) -> bool {
        !self.powder.is_empty()
    }
}

impl SnowSimulation {
    #[cfg(test)]
    pub(crate) fn weathering_powder(&self) -> usize {
        self.weathering.powder.len()
    }

    /// How far frost and mist should cover the glass, and the moon in its phase while it shows.
    pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
        self.weathering.wanted = atmosphere;
    }

    pub(super) fn advance_atmosphere(&mut self, dt: f32) {
        let (width, height) = (self.width, self.height);
        let area = (width * height / MEGAPOINT).max(0.2);
        let amount = self.intensity.clamp(0.0, 1.0);
        let wind = self.air.breeze();
        let held = self.snowfall_held;
        let modes = self.air.modes.clone();
        let wants = |mode: Mode| modes.contains(&mode) && !held;
        let raining = self.air.modes.iter().any(|mode| mode.rainy()) && !held;
        let cursor = self.air.cursor;
        let ground = &self.layers[0];
        let floor = |x: f32| ground.surface(ground.clamped_index(x.clamp(0.0, width - 1.0)));
        let ground_snow = |x: f32| floor(x) - ground.bed[ground.clamped_index(x.clamp(0.0, width - 1.0))];
        let w = &mut self.weathering;

        // Frost: crystals laid out once for the screen, shown as far as they have grown.
        if (w.rime_size != (width, height) || w.rime.is_empty()) && w.wanted.frost > 0.0 {
            w.rime = grow_rime(&mut w.random, width, height);
            w.rime_size = (width, height);
        }
        let frost_target = w.wanted.frost.clamp(0.0, 1.0);
        if w.frost < frost_target {
            w.frost = (w.frost + dt / FROST_GROWTH).min(frost_target);
        } else if w.frost > frost_target {
            // Thawing goes faster than growing.
            w.frost = (w.frost - dt / 20.0).max(frost_target);
        }

        // Mist: patches over the whole glass, each misting over toward what is wanted and wiped clear by a fast swipe.
        let columns = (width / MIST_CELL).ceil() as usize + 1;
        let rows = (height / MIST_CELL).ceil() as usize + 1;
        if w.mist.len() != columns * rows {
            w.mist = vec![0.0; columns * rows];
            w.mist_columns = columns;
        }
        let mist_target = w.wanted.mist.clamp(0.0, 1.0);
        let mut settled = true;
        if mist_target > 0.0 || w.mist.iter().any(|&density| density > 0.0) {
            for density in w.mist.iter_mut() {
                let before = *density;
                if *density < mist_target {
                    *density = (*density + dt / MIST_REGROWTH).min(mist_target);
                } else if *density > mist_target {
                    *density = (*density - dt / 8.0).max(mist_target);
                }
                settled &= *density == before;
            }
        }
        if let Some(at) = cursor {
            let from = w.last_cursor.unwrap_or(at);
            let travel = (at - from).length();
            // Only a quick swipe wipes, as a hand across the glass; a cursor going about its work leaves the mist.
            if travel / dt.max(1.0 / 240.0) > 600.0 && travel < 800.0 && mist_target > 0.0 {
                let steps = (travel / (MIST_CELL * 0.5)) as usize + 1;
                for step in 0..=steps {
                    let point = from + (at - from) * (step as f32 / steps as f32);
                    for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let column = (point.x / MIST_CELL).round() as isize + dx;
                        let row = (point.y / MIST_CELL).round() as isize + dy;
                        if column >= 0 && row >= 0 && (column as usize) < columns && (row as usize) < rows {
                            let cell = &mut w.mist[row as usize * columns + column as usize];
                            if *cell > 0.0 {
                                *cell = 0.0;
                                settled = false;
                            }
                        }
                    }
                }
            }
        }
        w.last_cursor = cursor;
        w.mist_settled = settled;

        // The moon: fades in at nightfall and out at dawn.
        if let Some(phase) = w.wanted.moon {
            w.phase = phase.rem_euclid(1.0);
        }
        w.moon = (w.moon + dt / if w.wanted.moon.is_some() { 5.0 } else { -3.0 }).clamp(0.0, 1.0);

        // Drops left on the glass once the rain stops, drying from the edges in, the small ones first.
        if raining {
            w.was_raining = (w.was_raining + dt / 60.0).min(1.0);
        } else if w.was_raining > 0.0 {
            let count = (((20.0 + 160.0 * amount) * area * w.was_raining) as usize).min(MAX_DROPS);
            for _ in 0..count {
                let random = &mut w.random;
                let radius = random.range(1.6, 5.5) * if random.next() < 0.08 { 1.8 } else { 1.0 };
                w.droplets.push(Droplet {
                    position: V2::new(random.range(0.0, width), random.range(height * 0.05, height)),
                    radius,
                    age: 0.0,
                    life: random.range(40.0, 70.0) + radius * 18.0,
                });
            }
            w.was_raining = 0.0;
        }
        for drop in w.droplets.iter_mut() {
            drop.age += dt;
        }
        w.droplets.retain(|drop| drop.age < drop.life);

        // Dandelion seeds: floating on the summer air, rising and sinking on it, carried along by the breeze.
        let seed_target = if wants(Mode::Pollen) { ((14.0 + 70.0 * amount) * area).min(220.0) as usize } else { 0 };
        while w.seeds.len() < seed_target {
            let random = &mut w.random;
            w.seeds.push(Seed {
                position: V2::new(random.range(0.0, width), random.range(height * 0.05, height * 0.95)),
                velocity: V2::new(random.range(-8.0, 8.0), random.range(-6.0, 6.0)),
                phase: random.range(0.0, TAU),
                size: random.range(7.0, 12.0),
                shown: 0.0,
            });
        }
        for (i, seed) in w.seeds.iter_mut().enumerate() {
            let keep = i < seed_target;
            seed.shown = (seed.shown + dt / if keep { 3.0 } else { -2.0 }).clamp(0.0, 1.0);
            seed.phase += dt * 0.6;
            let lift = (seed.phase).sin() * 9.0 + (seed.phase * 0.37 + 1.3).sin() * 6.0 - 2.5;
            seed.velocity.x += ((wind * 1.6 + (seed.phase * 0.8).cos() * 6.0) - seed.velocity.x) * (dt * 0.7).min(1.0);
            seed.velocity.y += (lift - seed.velocity.y) * (dt * 0.8).min(1.0);
            if let Some(at) = cursor {
                // The air the cursor moves sends them off, as a breath does.
                let away = seed.position - at;
                let distance = away.length();
                if distance < 90.0 && distance > 0.5 {
                    seed.velocity += away * (1.0 / distance) * (90.0 - distance) * 1.2 * dt;
                }
            }
            seed.position += seed.velocity * dt;
            seed.position.x = (seed.position.x + 20.0).rem_euclid(width + 40.0) - 20.0;
            if seed.position.y < floor(seed.position.x) + 10.0 {
                seed.velocity.y = seed.velocity.y.abs() + 4.0;
            } else if seed.position.y > height + 20.0 {
                seed.position.y = -10.0;
            }
        }
        w.seeds.retain(|seed| seed.shown > 0.0 || seed_target > 0);

        // Spindrift: in a strong wind, fine snow lifts off the drifts along the ground and streams along them.
        let strength = smoothstep(9.0, 22.0, wind.abs());
        let snowy = wants(Mode::Snow) || ground.top > 1.0;
        if strength > 0.0 && snowy {
            let wanted = ((420.0 * strength) * area) as usize;
            let room = wanted.saturating_sub(w.powder.len()).min((wanted / 20).max(2));
            for _ in 0..room {
                let random = &mut w.random;
                let x = random.range(0.0, width);
                let depth = ground_snow(x);
                if depth < 1.2 {
                    continue;
                }
                w.powder.push(Powder {
                    position: V2::new(x, floor(x) + random.range(0.0, 3.0)),
                    velocity: V2::new(wind.signum() * random.range(160.0, 420.0) * strength, random.range(10.0, 60.0)),
                    age: 0.0,
                    life: random.range(0.8, 2.6),
                    size: random.range(1.0, 2.2),
                });
            }
        }
        for grain in w.powder.iter_mut() {
            grain.age += dt;
            let swirl = (grain.age * 7.0 + grain.position.x * 0.02).sin() * 50.0;
            grain.velocity.y += (swirl - 60.0) * dt;
            grain.position += grain.velocity * dt;
            let rest = floor(grain.position.x);
            if grain.position.y < rest {
                grain.position.y = rest;
                grain.velocity.y = grain.velocity.y.abs() * 0.5 + 20.0;
            }
        }
        w.powder.retain(|grain| grain.age < grain.life && grain.position.x > -10.0 && grain.position.x < width + 10.0);
    }

    /// The moon, drawn behind everything that falls.
    pub(super) fn sky_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let w = &self.weathering;
        let shown = smoothstep(0.0, 1.0, w.moon) * self.falling_opacity;
        let at = V2::new(self.width * 0.84, self.height * 0.84);
        // How much of it is lit, for how bright its glow is.
        let lit = 0.5 - 0.5 * (w.phase * TAU).cos();
        let halo = (shown > 0.0).then(|| Sprite {
            position: at,
            size: 230.0,
            alpha: 0.09 * shown * (0.3 + 0.7 * lit),
            soft: 1.0,
            tint_amount: 1.0,
            tint: [0.9, 0.92, 1.0],
            ..Sprite::default()
        });
        let disc = (shown > 0.0).then(|| Sprite {
            position: at,
            size: 60.0,
            alpha: shown,
            soft: 0.0,
            tint_amount: 1.0,
            tint: [0.96, 0.95, 0.9],
            // The shader draws the moon's face, lit by its phase.
            lump: w.phase.max(0.001),
            petal: 5.0,
            ..Sprite::default()
        });
        halo.into_iter().chain(disc)
    }

    /// Seeds and spindrift, then what is on the glass, in front of everything: drops, frost, and mist over it all.
    pub(super) fn glass_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let w = &self.weathering;
        let opacity = self.falling_opacity;
        let seeds = w.seeds.iter().flat_map(move |seed| {
            let alpha = 0.75 * smoothstep(0.0, 1.0, seed.shown) * opacity;
            [
                // Its fluffy crown, with the fine rays of its hairs, and its seed hanging under it.
                Sprite { position: seed.position, size: seed.size, alpha: alpha * 0.8, soft: 0.6, tint_amount: 1.0, tint: [1.0, 1.0, 0.97], petal: 2.0, ..Sprite::default() },
                Sprite {
                    position: seed.position - V2::new(0.0, seed.size * 0.45),
                    size: 1.8,
                    alpha,
                    soft: 0.3,
                    tint_amount: 1.0,
                    tint: [0.55, 0.45, 0.32],
                    ..Sprite::default()
                },
            ]
        });
        let powder = w.powder.iter().map(move |grain| {
            let t = grain.age / grain.life;
            Sprite {
                position: grain.position,
                velocity: grain.velocity,
                size: grain.size,
                alpha: 0.7 * (1.0 - t * t) * smoothstep(0.0, 0.15, t) * opacity,
                soft: 0.7,
                ..Sprite::default()
            }
        });
        let drops = w.droplets.iter().map(move |drop| {
            let t = drop.age / drop.life;
            Sprite {
                position: drop.position,
                size: drop.radius * 2.0 * (1.0 - t).sqrt().max(0.25),
                alpha: (1.0 - smoothstep(0.75, 1.0, t)) * opacity,
                soft: 0.0,
                tint_amount: 1.0,
                tint: [0.9, 0.95, 1.0],
                // Six marks a drop on the glass in the shader: a little lens.
                petal: 6.0,
                ..Sprite::default()
            }
        });
        let frost = w.rime.iter().filter(move |rime| w.frost > 0.0 && rime.order <= w.frost).map(move |rime| {
            let fresh = smoothstep(0.0, 0.08, w.frost - rime.order);
            Sprite {
                position: rime.position,
                velocity: rime.along * 120.0,
                size: rime.size,
                alpha: (0.25 + 0.4 * (1.0 - rime.order)) * fresh * opacity,
                soft: 0.55,
                tint_amount: 0.35,
                tint: [0.82, 0.92, 1.0],
                ..Sprite::default()
            }
        });
        let columns = w.mist_columns.max(1);
        let height = self.height.max(1.0);
        let mist = w.mist.iter().enumerate().filter(|(_, density)| **density > 0.02).map(move |(i, &density)| {
            let (column, row) = (i % columns, i / columns);
            // A little uneven, so the patches don't line up.
            let jitter = ((i as f32 * 12.9898).sin() * 43758.545).fract();
            Sprite {
                position: V2::new(column as f32 * MIST_CELL + (jitter - 0.5) * 10.0, row as f32 * MIST_CELL + (jitter * 7.0).fract() * 10.0 - 5.0),
                size: MIST_CELL * 2.3,
                // Patches overlap about four deep; condensation gathers thicker toward the bottom of the glass.
                alpha: 0.045 * density * (0.85 + 0.3 * jitter) * (1.0 - 0.45 * (row as f32 * MIST_CELL / height).min(1.0)) * opacity,
                soft: 1.0,
                tint_amount: 1.0,
                tint: [0.9, 0.93, 0.96],
                ..Sprite::default()
            }
        });
        seeds.chain(powder).chain(drops).chain(frost).chain(mist)
    }
}

/// Frost crystals growing in from the edges of a screen: feathery branches, each with side branches at sixty degrees,
/// thickest along the bottom and in the corners, as on a real window, as points close enough together to read as lines.
fn grow_rime(random: &mut Random, width: f32, height: f32) -> Vec<Rime> {
    let mut rime = Vec::new();
    let reach = width.min(height) * 0.16;
    let roots = ((width + height) / 70.0) as usize;
    for _ in 0..roots {
        if rime.len() >= MAX_FROST {
            break;
        }
        // Where along the edges it starts, and which way is in: more along the bottom and near the corners.
        let side = random.next();
        let (start, inward) = if side < 0.4 {
            (V2::new(random.range(0.0, width), 0.0), V2::new(0.0, 1.0))
        } else if side < 0.6 {
            (V2::new(random.range(0.0, width), height), V2::new(0.0, -1.0))
        } else if side < 0.8 {
            (V2::new(0.0, random.range(0.0, height)), V2::new(1.0, 0.0))
        } else {
            (V2::new(width, random.range(0.0, height)), V2::new(-1.0, 0.0))
        };
        let corner = {
            let dx = start.x.min(width - start.x) / width;
            let dy = start.y.min(height - start.y) / height;
            1.0 - smoothstep(0.0, 0.25, dx.min(dy))
        };
        let length = reach * random.range(0.35, 0.9) * (0.7 + 0.6 * corner) * if inward.y > 0.0 { 1.2 } else { 1.0 };
        let angle = inward.y.atan2(inward.x) + random.range(-0.6, 0.6);
        branch(random, &mut rime, start, angle, length, 0.0, length, 2);
    }
    rime.truncate(MAX_FROST);
    rime
}

#[allow(clippy::too_many_arguments)]
fn branch(random: &mut Random, rime: &mut Vec<Rime>, from: V2, angle: f32, length: f32, order: f32, scale: f32, depth: u32) {
    let step = 2.4;
    let steps = (length / step) as usize;
    let mut point = from;
    let mut heading = angle;
    for n in 0..steps {
        if rime.len() >= MAX_FROST {
            return;
        }
        heading += random.range(-0.06, 0.06);
        let along = V2::new(heading.cos(), heading.sin());
        point += along * step;
        let travelled = n as f32 * step;
        let here = (order + travelled / scale.max(1.0)).min(1.0);
        rime.push(Rime { position: point, order: here, size: 1.3 + 0.9 * (1.0 - travelled / length.max(1.0)) * (depth as f32 * 0.4 + 0.2), along });
        // Side branches every few steps, to either side, at sixty degrees, shorter the farther out.
        if depth > 0 && n % 5 == 2 && random.next() < 0.75 {
            let left = length - travelled;
            for side in [-1.0, 1.0] {
                if random.next() < 0.8 {
                    let length = left * random.range(0.25, 0.45);
                    branch(random, rime, point, heading + side * PI / 3.0, length, here, scale, depth - 1);
                }
            }
        }
    }
}
