//! What the weather does to the screen itself rather than to what falls: frost growing in from the edges, glass misting
//! over that a quick swipe of the cursor wipes clear, drops left on the glass after rain drying away, and the moon in its
//! phase at night; and fine snow streaming along the drifts in a strong wind.
//!
//! What only sits there, the frost and the mist once they have grown, and the moon, doesn't keep the screen redrawing.

use std::f32::consts::{PI, TAU};

use std::collections::HashMap;

use super::{Mode, Random, SnowSimulation, Sprite, WindowId, spin, tumble, tumble_speed};
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
/// How far apart the glints of water along a wet window edge are, in points.
const WET_SPACING: f32 = 6.0;

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

/// An icicle hanging under a window's top edge: where along it, how long it can grow, how thick, and its own pace.
#[derive(Clone, Copy)]
struct Icicle {
    along: f32,
    length: f32,
    thickness: f32,
    pace: f32,
}

/// A wet leaf a storm has thrown against the glass: it sticks a while, slides down a little, then peels off and falls.
#[derive(Clone, Copy)]
struct Stuck {
    position: V2,
    velocity: V2,
    seed: f32,
    turn: f32,
    size: f32,
    age: f32,
    life: f32,
}

/// A crown of water thrown up where a raindrop strikes the ground or a window's top, gone in a fifth of a second.
#[derive(Clone, Copy)]
struct Crown {
    position: V2,
    size: f32,
    age: f32,
}

/// A puddle along the ground: where it lies and how wide it can spread.
#[derive(Clone, Copy)]
struct Puddle {
    along: f32,
    width: f32,
}

/// A drop of meltwater: clinging under a window's edge as it swells, then falling.
#[derive(Clone, Copy)]
struct Drip {
    position: V2,
    velocity: f32,
    age: f32,
}

/// A leaf caught on a window's top edge, hanging half over it and stirring in the wind.
#[derive(Clone, Copy)]
struct Hanging {
    along: f32,
    seed: f32,
    size: f32,
    phase: f32,
    petal: bool,
}

/// Frost growing in from a window's edges while it stays open in the cold.
struct WindowFrost {
    open: f32,
    grown: f32,
    size: (f32, f32),
    rime: Vec<Rime>,
}

/// A window left open this long in freezing weather starts to frost over, and is frosted as far as it gets after this.
const FROST_STARTS: f32 = 120.0;
const FROST_FULL: f32 = 720.0;
const MAX_WINDOW_FROST: usize = 1200;

/// A petal swept along in a gust's swarm.
#[derive(Clone, Copy)]
struct Swept {
    position: V2,
    velocity: V2,
    seed: f32,
    phase: f32,
    size: f32,
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
    /// How far it is growing toward.
    frost_aim: f32,
    rime: Vec<Rime>,
    rime_size: (f32, f32),
    mist: Vec<f32>,
    mist_columns: usize,
    mist_settled: bool,
    moon: f32,
    phase: f32,
    droplets: Vec<Droplet>,
    was_raining: f32,
    powder: Vec<Powder>,
    last_cursor: Option<V2>,
    /// Icicles by window, and how far they have grown, 0 to 1.
    icicles: HashMap<WindowId, Vec<Icicle>>,
    ice: f32,
    stuck: Vec<Stuck>,
    next_stuck: f64,
    swept: Vec<Swept>,
    next_swarm: f64,
    /// How wet the windows' top edges are, 0 to 1: they glisten while it rains and dry over a minute after.
    wet: f32,
    crowns: Vec<Crown>,
    /// How far the puddles have spread, 0 to 1, and where they lie.
    puddling: f32,
    puddles: Vec<Puddle>,
    raining_hard: f32,
    drips: Vec<Drip>,
    hanging: HashMap<WindowId, Vec<Hanging>>,
    /// Frost on each window left open in the cold: how long it has been open, how far its frost has grown, the size
    /// it was laid out for, and its crystals in points from the window's bottom left.
    window_frost: HashMap<WindowId, WindowFrost>,
    /// How many glints along the edges were drawn last, for the count of what is drawn.
    wet_spots: usize,
}

impl Default for Weathering {
    fn default() -> Weathering {
        Weathering {
            random: Random::new(),
            wanted: Atmosphere::default(),
            frost: 0.0,
            frost_aim: 0.0,
            rime: Vec::new(),
            rime_size: (0.0, 0.0),
            mist: Vec::new(),
            mist_columns: 0,
            mist_settled: true,
            moon: 0.0,
            phase: 0.0,
            droplets: Vec::new(),
            was_raining: 0.0,
            powder: Vec::new(),
            last_cursor: None,
            icicles: HashMap::new(),
            ice: 0.0,
            stuck: Vec::new(),
            next_stuck: 2.0,
            swept: Vec::new(),
            next_swarm: 3.0,
            wet: 0.0,
            wet_spots: 0,
            crowns: Vec::new(),
            puddling: 0.0,
            puddles: Vec::new(),
            raining_hard: 0.0,
            drips: Vec::new(),
            hanging: HashMap::new(),
            window_frost: HashMap::new(),
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

    fn icicles_shown(&self) -> usize {
        if self.ice <= 0.0 { 0 } else { self.icicles.values().map(Vec::len).sum() }
    }

    pub(super) fn sprite_count(&self) -> usize {
        self.frost_shown()
            + self.mist_shown()
            + if self.moon > 0.0 { 2 } else { 0 }
            + self.droplets.len()
            + self.powder.len()
            + self.icicles_shown()
            + self.stuck.len()
            + self.swept.len()
            + self.wet_spots
            + self.crowns.len()
            + if self.puddling > 0.0 { self.puddles.len() } else { 0 }
            + self.drips.len()
            + self.hanging.values().map(Vec::len).sum::<usize>()
            + self.window_frost.values().filter(|frost| frost.grown > 0.0).map(|frost| frost.rime.len()).sum::<usize>()
    }

    /// What is drawn but only sits there, and doesn't need the screen redrawn for its sake.
    pub(super) fn resting(&self) -> usize {
        let frost_still = self.frost == 0.0 || self.frost >= 1.0 || self.frost == self.frost_aim;
        (if frost_still { self.frost_shown() } else { 0 })
            + if self.mist_settled { self.mist_shown() } else { 0 }
            + if self.moon >= 1.0 { 2 } else { 0 }
            + if self.ice == 0.0 || self.ice >= 1.0 { self.icicles_shown() } else { 0 }
            // A window's frost grows over minutes: between frames it is as good as still.
            + self.window_frost.values().filter(|frost| frost.grown > 0.0).map(|frost| frost.rime.iter().filter(|rime| rime.order <= frost.grown).count()).sum::<usize>()
    }

    pub(super) fn lively(&self) -> bool {
        !self.powder.is_empty() || !self.swept.is_empty() || !self.crowns.is_empty() || !self.drips.is_empty()
    }
}

impl SnowSimulation {
    #[cfg(test)]
    pub(crate) fn weathering_powder(&self) -> usize {
        self.weathering.powder.len()
    }

    #[cfg(test)]
    pub(crate) fn weathering_swept(&self) -> usize {
        self.weathering.swept.len()
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
        // Freezing rain glazes the glass with frost too.
        let glazed = if self.air.asked.contains(&Mode::FreezingRain) && !held { 0.45 } else { 0.0 };
        let cursor = self.air.cursor;
        let ground = &self.layers[0];
        let floor = |x: f32| ground.surface(ground.clamped_index(x.clamp(0.0, width - 1.0)));
        let ground_snow = |x: f32| floor(x) - ground.bed[ground.clamped_index(x.clamp(0.0, width - 1.0))];
        let w = &mut self.weathering;

        // Frost: crystals laid out once for the screen, shown as far as they have grown.
        if (w.rime_size != (width, height) || w.rime.is_empty()) && w.wanted.frost.max(glazed) > 0.0 {
            w.rime = grow_rime(&mut w.random, width, height);
            w.rime_size = (width, height);
        }
        let frost_target = w.wanted.frost.max(glazed).clamp(0.0, 1.0);
        w.frost_aim = frost_target;
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

        // Spindrift: in a strong wind, fine snow lifts off the drifts along the ground and streams along them.
        let strength = smoothstep(9.0, 22.0, wind.abs());
        // Only from snow: a heap of leaves doesn't smoke in the wind.
        let snowy = wants(Mode::Snow) && ground.top > 1.0;
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

        let clock = self.air.clock;
        let freezing = self.air.asked.contains(&Mode::FreezingRain) && !held;
        let storming = self.air.asked.contains(&Mode::AutumnStorm) && !held;
        let blowing = self.air.asked.contains(&Mode::BlossomWind) && !held;
        let w = &mut self.weathering;

        // The windows' top edges glisten wet while it rains, and dry over a minute once it stops.
        w.wet = (w.wet + dt / if raining { 6.0 } else { -60.0 }).clamp(0.0, 1.0);
        w.wet_spots = if w.wet > 0.0 { self.layers.iter().skip(1).map(|layer| (layer.width() / WET_SPACING) as usize + 1).sum() } else { 0 };

        // Freezing rain: icicles grow under every window's top edge over a minute and a half, and melt once it stops.
        w.ice = (w.ice + dt / if freezing { 90.0 } else { -40.0 }).clamp(0.0, 1.0);
        if w.ice > 0.0 {
            for li in 1..self.layers.len() {
                let id = self.ids[li];
                let span = self.layers[li].width();
                let random = &mut w.random;
                w.icicles.entry(id).or_insert_with(|| {
                    let count = (span / 22.0) as usize;
                    (0..count)
                        .map(|_| Icicle {
                            along: random.next(),
                            // Mostly short, now and then a long one.
                            length: 5.0 + 38.0 * random.next().powf(2.2),
                            thickness: random.range(0.16, 0.3),
                            pace: random.range(0.5, 1.0),
                        })
                        .collect()
                });
            }
            let present: Vec<WindowId> = self.ids.iter().skip(1).copied().collect();
            w.icicles.retain(|id, _| present.contains(id));
        } else {
            w.icicles.clear();
        }

        // An autumn storm now and then throws a wet leaf against the glass, where it sticks and slides before it
        // peels off.
        if storming && clock >= w.next_stuck && w.stuck.len() < 10 {
            let random = &mut w.random;
            w.stuck.push(Stuck {
                position: V2::new(random.range(width * 0.04, width * 0.96), random.range(height * 0.25, height * 0.95)),
                velocity: V2::ZERO,
                seed: random.range(0.001, 1.0),
                turn: random.range(0.0, TAU),
                size: random.range(18.0, 28.0),
                age: 0.0,
                life: random.range(6.0, 16.0),
            });
            w.next_stuck = clock + random.range(3.0, 9.0) as f64;
        }
        for leaf in w.stuck.iter_mut() {
            leaf.age += dt;
            if leaf.age < leaf.life {
                // Held by the water, it creeps down the glass, a little at a time.
                leaf.position.y -= (2.0 + 4.0 * (leaf.age * 1.3 + leaf.seed * 9.0).sin().max(0.0)) * dt;
            } else {
                // Peeled off by the wind: it falls away, carried by the gusts.
                leaf.velocity += V2::new(wind * 3.0 - leaf.velocity.x, -300.0) * dt;
                leaf.position += leaf.velocity * dt;
                leaf.turn += dt * 3.0;
            }
        }
        w.stuck.retain(|leaf| leaf.age < leaf.life + 3.0 && leaf.position.y > -40.0);

        // Blossom wind: now and then a gust sweeps a swarm of petals across the screen in a band, rising and dipping.
        if blowing && clock >= w.next_swarm && w.swept.len() < 600 {
            let random = &mut w.random;
            let way = if wind < 0.0 { -1.0 } else { 1.0 };
            let band = random.range(height * 0.25, height * 0.85);
            let count = ((60.0 + 90.0 * amount) * area.sqrt()) as usize;
            let speed = random.range(280.0, 420.0);
            for _ in 0..count {
                w.swept.push(Swept {
                    position: V2::new(if way > 0.0 { -random.range(10.0, 320.0) } else { width + random.range(10.0, 320.0) }, band + random.range(-70.0, 70.0)),
                    velocity: V2::new(way * speed * random.range(0.8, 1.2), random.range(-20.0, 20.0)),
                    seed: random.range(0.001, 1.0),
                    phase: random.range(0.0, TAU),
                    size: random.range(8.0, 13.0),
                });
            }
            w.next_swarm = clock + random.range(7.0, 14.0) as f64;
        }
        for petal in w.swept.iter_mut() {
            petal.phase += dt * 1.4;
            let lift = petal.phase.sin() * 70.0 + (petal.position.x * 0.004 + clock as f32 * 0.7).sin() * 50.0;
            petal.position += V2::new(petal.velocity.x, petal.velocity.y + lift) * dt;
            // The gust dies down as it goes, and the petals start to drop.
            petal.velocity.x *= (-0.12 * dt).exp();
            petal.velocity.y -= 18.0 * dt;
        }
        w.swept.retain(|petal| petal.position.x > -340.0 && petal.position.x < width + 340.0 && petal.position.y > -20.0);

        // Crowns of water where the rain strikes the ground or a window's top, as many as it rains hard.
        let raining = self.air.modes.iter().any(|mode| mode.rainy()) && !held;
        w.raining_hard += ((if raining { amount } else { 0.0 }) - w.raining_hard) * (dt / 4.0).min(1.0);
        if raining {
            let per_second = (30.0 + 260.0 * amount) * area;
            let count = (per_second * dt + w.random.next()) as usize;
            for _ in 0..count.min(12) {
                let random = &mut w.random;
                // Most strike the ground; some the windows' tops, by how wide they are.
                let li = if self.layers.len() > 1 && random.next() < 0.3 { 1 + (random.next() * (self.layers.len() - 1) as f32) as usize } else { 0 };
                let layer = &self.layers[li.min(self.layers.len() - 1)];
                let x = layer.base().x + random.next() * layer.width();
                if x < 0.0 || x > width {
                    continue;
                }
                let surface = layer.surface(layer.clamped_index(x));
                w.crowns.push(Crown { position: V2::new(x, surface + 1.0), size: random.range(7.0, 13.0), age: 0.0 });
            }
        }
        for crown in w.crowns.iter_mut() {
            crown.age += dt;
        }
        w.crowns.retain(|crown| crown.age < 0.2);

        // Puddles spread along the ground in steady rain over a couple of minutes, and dry slowly once it stops.
        w.puddling = (w.puddling + dt / if raining { 150.0 } else { -300.0 }).clamp(0.0, 1.0);
        if w.puddling > 0.0 && w.puddles.is_empty() {
            let random = &mut w.random;
            let count = 3 + (width / 500.0) as usize;
            w.puddles = (0..count).map(|_| Puddle { along: random.next(), width: random.range(90.0, 260.0) }).collect();
        } else if w.puddling <= 0.0 {
            w.puddles.clear();
        }

        // Thawing snow drips off the windows' top edges: a drop swells under the edge, then lets go.
        let thawing = self.thaws && !wants(Mode::Snow) && !held;
        if thawing && w.drips.len() < 80 {
            for li in 1..self.layers.len() {
                let layer = &self.layers[li];
                if layer.top < 1.0 || layer.overall().leaves > 0.5 {
                    continue;
                }
                let random = &mut w.random;
                if random.next() < layer.width() / 900.0 * dt * 6.0 {
                    let i = (random.next() * layer.count() as f32) as usize;
                    if layer.heights.get(i).is_some_and(|&h| h > 0.5) {
                        let x = layer.base().x + (i as f32 + 0.5) * super::COLUMN_WIDTH;
                        w.drips.push(Drip { position: V2::new(x, layer.base().y - 1.0), velocity: 0.0, age: 0.0 });
                    }
                }
            }
        }
        for drip in w.drips.iter_mut() {
            drip.age += dt;
            if drip.age > 0.7 {
                drip.velocity -= 1100.0 * dt;
                drip.position.y += drip.velocity * dt;
            }
        }
        w.drips.retain(|drip| drip.position.y > -10.0 && drip.age < 4.0);

        // Leaves caught on the windows' top edges, a few on each window with leaves lying on it; a gust or a swipe of
        // the cursor sends one off.
        let wake = self.air.cursor_wake();
        let mut let_go: Vec<(V2, f32, f32, bool)> = Vec::new();
        for li in 1..self.layers.len() {
            let id = self.ids[li];
            let layer = &self.layers[li];
            let blend = layer.overall();
            if layer.top < 1.0 || blend.leaves < 0.4 {
                // The heap they hung from is gone: they fall with it.
                if let Some(hanging) = w.hanging.remove(&id) {
                    let base = layer.base();
                    let span = layer.width();
                    for leaf in hanging {
                        let_go.push((V2::new(base.x + leaf.along * span, base.y - leaf.size * 0.25), leaf.seed, leaf.size, leaf.petal));
                    }
                }
                continue;
            }
            let span = layer.width();
            let base = layer.base();
            let random = &mut w.random;
            let petals = blend.petals;
            let hanging = w.hanging.entry(id).or_insert_with(|| {
                (0..(span / 170.0) as usize + 1)
                    .map(|_| Hanging {
                        along: random.next(),
                        seed: random.range(0.001, 1.0),
                        size: random.range(15.0, 24.0),
                        phase: random.range(0.0, TAU),
                        petal: random.next() < petals,
                    })
                    .collect()
            });
            hanging.retain_mut(|leaf| {
                leaf.phase += dt * (1.2 + wind.abs() * 0.08);
                let at = V2::new(base.x + leaf.along * span, base.y - leaf.size * 0.25);
                let swept = wake.is_some_and(|(cursor, velocity)| (cursor - at).length() < 30.0 && velocity.length() > 250.0);
                let blown = wind.abs() > 24.0 && random.next() < dt * 0.4;
                if swept || blown {
                    let_go.push((at, leaf.seed, leaf.size, leaf.petal));
                    return false;
                }
                true
            });
        }
        for (at, seed, size, _) in let_go {
            w.stuck.push(Stuck { position: at, velocity: V2::new(wind * 2.0, 40.0), seed, turn: -1.0, size, age: 1.0, life: 1.0 });
        }
        let present: Vec<WindowId> = self.ids.iter().skip(1).copied().collect();
        w.hanging.retain(|id, _| present.contains(id));

        // In freezing weather a window left open a while frosts over from its edges in, slowly; it thaws when it
        // warms up, and a window closed takes its frost with it.
        let cold = w.wanted.frost.max(glazed) > 0.15 && !held;
        for (&id, (frame, _)) in &self.frames {
            let random = &mut w.random;
            let frost = w.window_frost.entry(id).or_insert_with(|| WindowFrost { open: 0.0, grown: 0.0, size: (0.0, 0.0), rime: Vec::new() });
            frost.open += dt;
            let size = (frame.width, frame.height);
            if cold && ((frost.size.0 - size.0).abs() > 40.0 || (frost.size.1 - size.1).abs() > 40.0 || frost.rime.is_empty()) {
                frost.rime = grow_rime(random, size.0, size.1);
                frost.rime.truncate(MAX_WINDOW_FROST);
                frost.size = size;
            }
            let target = if cold { smoothstep(FROST_STARTS, FROST_FULL, frost.open) * w.wanted.frost.max(glazed).min(1.0) * 0.8 } else { 0.0 };
            frost.grown = if frost.grown < target { (frost.grown + dt / FROST_GROWTH).min(target) } else { (frost.grown - dt / 30.0).max(target) };
        }
        let open: Vec<WindowId> = self.frames.keys().copied().collect();
        w.window_frost.retain(|id, _| open.contains(id));
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

    /// Spindrift, then what is on the glass, in front of everything: drops, frost, and mist over it all.
    pub(super) fn glass_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let w = &self.weathering;
        let opacity = self.falling_opacity;
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
        let clock = self.air.clock;
        let ice = smoothstep(0.0, 1.0, w.ice);
        let layers = &self.layers;
        let icicles = self.ids.iter().enumerate().skip(1).filter(move |_| ice > 0.0).flat_map(move |(li, id)| {
            let layer = &layers[li];
            let base = layer.base();
            let span = layer.width();
            let hidden = &layer.face_occluders;
            w.icicles.get(id).into_iter().flatten().filter_map(move |icicle| {
                let length = icicle.length * (ice * (1.0 + icicle.pace) - icicle.pace * 0.5).clamp(0.0, 1.0);
                let x = base.x + icicle.along * span;
                let middle = base.y - length * 0.5;
                // Not over a window in front of this one.
                let covered = hidden.iter().any(|r| x >= r[0] && x <= r[2] && middle >= r[1] && middle <= r[3]);
                (length > 1.5 && !covered).then(|| Sprite {
                    position: V2::new(x, middle + 1.0),
                    size: length,
                    alpha: 0.9 * opacity,
                    soft: 0.0,
                    tint_amount: 1.0,
                    tint: [0.86, 0.93, 1.0],
                    // Eight marks an icicle in the shader, its thickness in `lump`.
                    lump: icicle.thickness,
                    petal: 8.0,
                    ..Sprite::default()
                })
            })
        });
        let stuck = w.stuck.iter().map(move |leaf| {
            let falling = leaf.age >= leaf.life;
            Sprite {
                position: leaf.position,
                velocity: leaf.velocity,
                size: leaf.size,
                alpha: (1.0 - smoothstep(leaf.life + 1.5, leaf.life + 3.0, leaf.age)) * smoothstep(0.0, 0.25, leaf.age) * opacity,
                // Pressed flat against the glass, face on; turning over once it peels off.
                leaf: [leaf.seed, leaf.turn, if falling { tumble(clock * 2.0, leaf.seed) } else { 1.0 }],
                spin: if falling { [3.0, tumble_speed(clock * 2.0, leaf.seed) * 2.0] } else { [0.0, 0.0] },
                ..Sprite::default()
            }
        });
        let swept = w.swept.iter().map(move |petal| Sprite {
            position: petal.position,
            velocity: petal.velocity,
            size: petal.size,
            alpha: opacity,
            leaf: [petal.seed, petal.seed * 40.0 + spin(clock, 2.0 + petal.seed * 3.0), tumble(clock * 1.5, petal.seed)],
            spin: [2.0 + petal.seed * 3.0, tumble_speed(clock * 1.5, petal.seed) * 1.5],
            petal: 1.0,
            ..Sprite::default()
        });
        // Water along each window's top edge: a row of tiny glints, brighter here and there, catching the light.
        let wet = smoothstep(0.0, 1.0, w.wet);
        let edges = self.layers.iter().skip(1).filter(move |_| wet > 0.0).flat_map(move |layer| {
            let base = layer.base();
            let count = (layer.width() / WET_SPACING) as usize + 1;
            let hidden = &layer.face_occluders;
            (0..count).filter_map(move |k| {
                let x = base.x + k as f32 * WET_SPACING + 2.0;
                let sparkle = ((x * 12.9898).sin() * 43758.545).fract();
                let covered = hidden.iter().any(|r| x >= r[0] && x <= r[2] && base.y >= r[1] && base.y <= r[3]);
                (!covered && sparkle > 0.25).then(|| Sprite {
                    position: V2::new(x, base.y - 0.5),
                    velocity: V2::new(140.0, 0.0),
                    size: 1.3 + sparkle * 1.2,
                    alpha: wet * (0.12 + 0.35 * sparkle * sparkle) * opacity,
                    soft: 0.5,
                    tint_amount: 0.4,
                    tint: [0.9, 0.95, 1.0],
                    ..Sprite::default()
                })
            })
        });
        let ground = &self.layers[0];
        let width = self.width;
        let puddling = smoothstep(0.0, 1.0, w.puddling);
        let ripples = w.raining_hard;
        let puddles = w.puddles.iter().filter(move |_| puddling > 0.0).map(move |puddle| {
            let x = puddle.along * width;
            let surface = ground.surface(ground.clamped_index(x.clamp(0.0, width - 1.0)));
            let spread = puddle.width * (0.4 + 0.6 * puddling);
            Sprite {
                // The sheet lies along the bottom of its sprite, on the ground.
                position: V2::new(x, surface + 1.0 + spread * 0.45),
                size: spread,
                alpha: 0.85 * puddling * opacity,
                // How hard it rains, for the rings the drops make on it.
                soft: ripples,
                tint_amount: 1.0,
                tint: [0.55, 0.62, 0.72],
                lump: (puddle.along * 7.3).fract().max(0.02),
                // Twelve marks a puddle in the shader.
                petal: 12.0,
                ..Sprite::default()
            }
        });
        let crowns = w.crowns.iter().map(move |crown| Sprite {
            // The crown rises from low in its sprite, where the drop struck.
            position: crown.position + V2::new(0.0, crown.size * 0.3),
            size: crown.size,
            alpha: 0.6 * opacity,
            soft: 0.0,
            tint_amount: 1.0,
            tint: [0.85, 0.9, 0.97],
            // How far along it is, and eleven for a crown of water.
            lump: (crown.age / 0.2).clamp(0.02, 1.0),
            petal: 11.0,
            ..Sprite::default()
        });
        let drips = w.drips.iter().map(move |drip| {
            let swell = (drip.age / 0.7).min(1.0);
            Sprite {
                position: drip.position,
                velocity: V2::new(0.0, drip.velocity),
                size: 1.2 + 1.6 * swell,
                alpha: 0.7 * opacity,
                soft: 0.0,
                tint_amount: 1.0,
                tint: [0.9, 0.95, 1.0],
                petal: 6.0,
                ..Sprite::default()
            }
        });
        let hanging = self.ids.iter().enumerate().skip(1).flat_map(move |(li, id)| {
            let layer = &layers[li];
            let base = layer.base();
            let span = layer.width();
            w.hanging.get(id).into_iter().flatten().map(move |leaf| {
                // Tipped over the edge, nodding in the wind.
                let nod = leaf.phase.sin() * 0.25 + (leaf.phase * 2.3 + leaf.seed * 9.0).sin() * 0.1;
                Sprite {
                    position: V2::new(base.x + leaf.along * span, base.y - leaf.size * 0.22),
                    size: leaf.size,
                    alpha: opacity,
                    leaf: [leaf.seed, -1.25 + nod + leaf.seed, 0.75],
                    petal: if leaf.petal { 1.0 } else { 0.0 },
                    ..Sprite::default()
                }
            })
        });
        let frames = &self.frames;
        let window_frost = w.window_frost.iter().filter(|(_, frost)| frost.grown > 0.0).flat_map(move |(id, frost)| {
            let place = frames.get(id);
            frost.rime.iter().filter(move |rime| rime.order <= frost.grown).filter_map(move |rime| {
                let (frame, cover) = place?;
                // Laid out for the size it had; it stretches with the window if that changes a little.
                let x = frame.x + rime.position.x * frame.width / frost.size.0.max(1.0);
                let y = frame.y + rime.position.y * frame.height / frost.size.1.max(1.0);
                let hidden = cover.iter().any(|r| x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height);
                let fresh = smoothstep(0.0, 0.08, frost.grown - rime.order);
                (!hidden).then(|| Sprite {
                    position: V2::new(x, y),
                    velocity: rime.along * 120.0,
                    size: rime.size * 0.85,
                    alpha: (0.22 + 0.35 * (1.0 - rime.order)) * fresh * opacity,
                    soft: 0.55,
                    tint_amount: 0.35,
                    tint: [0.82, 0.92, 1.0],
                    ..Sprite::default()
                })
            })
        });
        powder.chain(swept).chain(stuck).chain(icicles).chain(drops).chain(edges).chain(puddles).chain(crowns).chain(drips).chain(hanging).chain(window_frost).chain(frost).chain(mist)
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
