//! DesktopFlora's own weathers beside Snowhush's: a thunderstorm's lightning over its rain, fog drifting low, fireflies
//! glowing and wandering, confetti tumbling down and lying a while, fireworks bursting, and hail bouncing off the
//! windows and the ground, northern lights rippling over the sky, and sand driven across the screen. Each keeps its own particles, which only come while its kind is wanted and go their own way
//! once it isn't: fog and fireflies fade, the rest falls out or burns down.

use std::f32::consts::{PI, TAU};

use super::{Mode, Random, SnowSimulation, Sprite, drawn_size, landing, spin, spin_rate, tumble, tumble_speed};
use crate::geometry::{V2, smoothstep};

/// How much of a screen a megapoint is, to scale how many of each there are.
const MEGAPOINT: f32 = 1_000_000.0;
const CONFETTI_COLOURS: [[f32; 3]; 7] =
    [[1.0, 0.33, 0.38], [1.0, 0.72, 0.18], [0.36, 0.84, 0.44], [0.28, 0.62, 1.0], [0.72, 0.42, 1.0], [1.0, 0.5, 0.82], [0.2, 0.86, 0.86]];
const FIREWORK_HUES: [[f32; 3]; 6] = [[1.0, 0.36, 0.3], [1.0, 0.8, 0.3], [0.4, 0.9, 0.45], [0.35, 0.65, 1.0], [0.85, 0.45, 1.0], [1.0, 1.0, 1.0]];
/// Confetti and hail lying on the ground at once, at most.
const MAX_LYING: usize = 500;
const MAX_SPARKS: usize = 2500;
/// How far apart the rays of a curtain of northern lights stand, in points.
const RAY_SPACING: f32 = 20.0;

/// A curtain of northern lights: where along the top of the screen it hangs and how it ripples.
#[derive(Clone, Copy)]
struct Curtain {
    /// How high its lower edge hangs, as a share of the screen's height.
    base: f32,
    /// Where it starts and ends across the screen, as shares of its width.
    from: f32,
    to: f32,
    amplitude: f32,
    waves: [f32; 2],
    speeds: [f32; 2],
    phase: f32,
    color: [f32; 3],
}

/// The northern lights: a few curtains, faded in while they are wanted.
#[derive(Default)]
struct Aurora {
    curtains: Vec<Curtain>,
    shown: f32,
}

/// A grain of sand or dust driven along by the storm.
#[derive(Clone, Copy)]
struct Grain {
    position: V2,
    velocity: V2,
    size: f32,
    shade: f32,
    phase: f32,
    shown: f32,
}

/// A leaf caught by a sandstorm, tumbling along with it.
#[derive(Clone, Copy)]
struct Debris {
    position: V2,
    velocity: V2,
    seed: f32,
    phase: f32,
    size: f32,
}

/// A puff of fog: big, faint and soft, drifting sideways near the bottom of the screen.
#[derive(Clone, Copy)]
struct Puff {
    position: V2,
    velocity: f32,
    radius: f32,
    alpha: f32,
    /// How far it has faded in, 0 to 1; it fades out the same way once fog isn't wanted.
    shown: f32,
    phase: f32,
}

/// A firefly: a small light wandering in the lower part of the screen, glowing up and dimming in its own rhythm.
#[derive(Clone, Copy)]
struct Glow {
    position: V2,
    velocity: V2,
    phase: f32,
    rate: f32,
    shown: f32,
}

/// A piece of confetti or a hailstone: falling, or lying on the ground until it fades.
#[derive(Clone, Copy)]
struct Bit {
    position: V2,
    velocity: V2,
    size: f32,
    color: [f32; 3],
    seed: f32,
    phase: f32,
    /// Seconds since it came to rest, while it lies.
    lying: Option<f32>,
    /// A hailstone's bounces so far.
    bounces: u8,
}

/// A spark of a firework, a rocket climbing, or a burst's glow.
#[derive(Clone, Copy)]
struct Spark {
    position: V2,
    velocity: V2,
    color: [f32; 3],
    size: f32,
    age: f32,
    life: f32,
    kind: SparkKind,
}

#[derive(Clone, Copy, PartialEq)]
enum SparkKind {
    /// Climbs to `target`'s height and bursts there.
    Rocket { burst_at: f32 },
    /// Left behind a rocket, fading fast.
    Trail,
    Spark,
    /// The light a burst throws, for a moment.
    Flash,
}

/// A stroke of lightning: its path down the sky, how bright it is now, and when it flickers again.
#[derive(Default)]
struct Lightning {
    path: Vec<V2>,
    brightness: f32,
    flicker_at: Option<f64>,
    x: f32,
}

pub(super) struct Extras {
    random: Random,
    fog: Vec<Puff>,
    glows: Vec<Glow>,
    confetti: Vec<Bit>,
    hail: Vec<Bit>,
    sparks: Vec<Spark>,
    lightning: Lightning,
    next_strike: f64,
    next_burst: f64,
    aurora: Aurora,
    haze: Vec<Puff>,
    grains: Vec<Grain>,
    debris: Vec<Debris>,
}

impl Default for Extras {
    fn default() -> Extras {
        Extras {
            random: Random::new(),
            fog: Vec::new(),
            glows: Vec::new(),
            confetti: Vec::new(),
            hail: Vec::new(),
            sparks: Vec::new(),
            lightning: Lightning::default(),
            next_strike: 2.0,
            next_burst: 0.5,
            aurora: Aurora::default(),
            haze: Vec::new(),
            grains: Vec::new(),
            debris: Vec::new(),
        }
    }
}

impl Extras {
    pub(super) fn sprite_count(&self) -> usize {
        self.fog.len()
            + self.glows.len() * 2
            + self.confetti.len()
            + self.hail.len()
            + self.sparks.len()
            + if self.lightning.brightness > 0.01 { self.lightning.path.len() + 1 } else { 0 }
            + if self.aurora.shown > 0.0 { self.aurora.curtains.len() * 120 } else { 0 }
            + self.haze.len()
            + self.grains.len()
            + self.debris.len()
    }

    /// Whether anything moves fast enough to be drawn at more than the calm rate.
    pub(super) fn lively(&self) -> bool {
        !self.sparks.is_empty() || self.lightning.brightness > 0.01 || !self.grains.is_empty() || !self.debris.is_empty()
    }
}

impl SnowSimulation {
    pub(super) fn advance_extras(&mut self, dt: f32) {
        let modes = self.air.modes.clone();
        let wants = |mode: Mode| modes.contains(&mode) && !self.snowfall_held;
        let area = (self.width * self.height / MEGAPOINT).max(0.2);
        let amount = self.intensity.clamp(0.0, 1.0);
        let (width, height) = (self.width, self.height);
        let wind = self.air.breeze() * 0.6;
        let clock = self.air.clock;
        let cursor = self.air.cursor;
        let ground = &self.layers[0];
        let floor = |x: f32| ground.surface(ground.clamped_index(x.clamp(0.0, width - 1.0)));
        let palette = self.air.look.palette.clone();
        let e = &mut self.extras;

        // Fog: puffs keep their number while fog is wanted, and all fade away once it isn't.
        let fog_target = if wants(Mode::Fog) { ((10.0 + 26.0 * amount) * area).min(90.0) as usize } else { 0 };
        while e.fog.len() < fog_target {
            let random = &mut e.random;
            let radius = random.range(120.0, 300.0);
            e.fog.push(Puff {
                position: V2::new(random.range(-radius, width + radius), random.next().powf(1.8) * height * 0.42),
                velocity: random.range(5.0, 14.0) * if random.next() < 0.5 { -1.0 } else { 1.0 },
                radius,
                alpha: random.range(0.05, 0.11) * (0.55 + 0.45 * amount),
                shown: 0.0,
                phase: random.range(0.0, TAU),
            });
        }
        let fog_wanted = fog_target > 0;
        for (i, puff) in e.fog.iter_mut().enumerate() {
            let keep = fog_wanted && i < fog_target;
            puff.shown = (puff.shown + dt / if keep { 4.0 } else { -2.5 }).clamp(0.0, 1.0);
            puff.phase += dt * 0.15;
            puff.position.x += (puff.velocity + wind * 0.25) * dt;
            puff.position.y += puff.phase.sin() * 2.0 * dt;
            // Off one side, it comes back in on the other.
            if puff.position.x < -puff.radius * 1.2 {
                puff.position.x = width + puff.radius;
            } else if puff.position.x > width + puff.radius * 1.2 {
                puff.position.x = -puff.radius;
            }
        }
        e.fog.retain(|puff| puff.shown > 0.0 || fog_wanted);

        // Fireflies: wandering, shying from the cursor, kept to the lower part of the screen.
        let glow_target = if wants(Mode::Fireflies) { ((8.0 + 24.0 * amount) * area).min(80.0) as usize } else { 0 };
        while e.glows.len() < glow_target {
            let random = &mut e.random;
            e.glows.push(Glow {
                position: V2::new(random.range(0.0, width), random.range(height * 0.04, height * 0.55)),
                velocity: V2::new(random.range(-10.0, 10.0), random.range(-6.0, 6.0)),
                phase: random.range(0.0, TAU),
                rate: random.range(0.7, 1.6),
                shown: 0.0,
            });
        }
        for (i, glow) in e.glows.iter_mut().enumerate() {
            let keep = i < glow_target;
            glow.shown = (glow.shown + dt / if keep { 2.0 } else { -1.5 }).clamp(0.0, 1.0);
            glow.phase += glow.rate * dt;
            let random = &mut e.random;
            glow.velocity += V2::new(random.range(-40.0, 40.0), random.range(-30.0, 30.0)) * dt;
            if let Some(cursor) = cursor {
                let away = glow.position - cursor;
                let distance = away.length();
                if distance < 120.0 && distance > 0.01 {
                    glow.velocity += away * (1.0 / distance) * (120.0 - distance) * 2.0 * dt;
                }
            }
            // Drawn back into its band, and never faster than a drift.
            let low = floor(glow.position.x) + 20.0;
            if glow.position.y < low {
                glow.velocity.y += 30.0 * dt;
            } else if glow.position.y > height * 0.6 {
                glow.velocity.y -= 30.0 * dt;
            }
            let speed = glow.velocity.length();
            if speed > 26.0 {
                glow.velocity *= 26.0 / speed;
            }
            glow.position += glow.velocity * dt;
            glow.position.x = glow.position.x.rem_euclid(width.max(1.0));
        }
        e.glows.retain(|glow| glow.shown > 0.0 || glow_target > 0);

        // Confetti: tumbling down at its own pace, lying on the ground a while, then fading.
        let confetti_target = if wants(Mode::Confetti) { ((60.0 + 360.0 * amount) * area).min(1400.0) as usize } else { 0 };
        let falling = e.confetti.iter().filter(|bit| bit.lying.is_none()).count();
        for _ in falling..confetti_target {
            let random = &mut e.random;
            let color = if palette.is_empty() {
                CONFETTI_COLOURS[(random.next() * CONFETTI_COLOURS.len() as f32) as usize % CONFETTI_COLOURS.len()]
            } else {
                palette[(random.next() * palette.len() as f32) as usize % palette.len()]
            };
            e.confetti.push(Bit {
                position: V2::new(random.range(-20.0, width + 20.0), height + random.range(10.0, height * 0.9)),
                velocity: V2::new(0.0, -random.range(45.0, 85.0)),
                size: random.range(5.0, 9.0),
                color,
                seed: random.range(0.05, 1.0),
                phase: random.range(0.0, TAU),
                lying: None,
                bounces: 0,
            });
        }
        for bit in e.confetti.iter_mut() {
            if let Some(age) = bit.lying.as_mut() {
                *age += dt;
                continue;
            }
            bit.phase += dt * (1.5 + bit.seed * 2.0);
            let sway = bit.phase.sin() * 28.0 * (0.5 + bit.seed);
            let fall = bit.velocity.y;
            let before = bit.position.y;
            bit.position += V2::new(wind + sway, fall) * dt;
            if bit.position.x < -30.0 {
                bit.position.x += width + 60.0;
            } else if bit.position.x > width + 30.0 {
                bit.position.x -= width + 60.0;
            }
            let rest = floor(bit.position.x);
            if before >= rest && bit.position.y <= rest {
                bit.position.y = rest + bit.size * 0.15;
                bit.lying = Some(0.0);
            }
        }
        e.confetti.retain(|bit| bit.lying.is_none_or(|age| age < 24.0) && bit.position.y > -20.0);
        trim_lying(&mut e.confetti);

        // Hail: hard and fast, bouncing off window tops and the ground, then lying a moment before it melts.
        let hail_target = if wants(Mode::Hail) { ((40.0 + 280.0 * amount) * area).min(800.0) as usize } else { 0 };
        let falling = e.hail.iter().filter(|bit| bit.lying.is_none() && bit.bounces == 0).count();
        for _ in falling..hail_target {
            let random = &mut e.random;
            let fall = random.range(520.0, 780.0);
            e.hail.push(Bit {
                position: V2::new(random.range(-40.0, width + 40.0), height + random.range(10.0, height)),
                velocity: V2::new(wind * 2.5, -fall),
                size: random.range(2.2, 4.2),
                color: [0.93, 0.97, 1.0],
                seed: random.next(),
                phase: 0.0,
                lying: None,
                bounces: 0,
            });
        }
        let layers = &self.layers;
        for bit in e.hail.iter_mut() {
            if let Some(age) = bit.lying.as_mut() {
                *age += dt;
                continue;
            }
            let before = bit.position.y;
            bit.velocity.y -= 1400.0 * dt;
            bit.position += bit.velocity * dt;
            if let Some((li, i)) = landing(layers, bit.position.x, before, bit.position.y, true, true) {
                let surface = layers[li].surface(i);
                bit.position.y = surface + bit.size * 0.5;
                let random = &mut e.random;
                // Each bounce takes most of its speed; slow enough, it stays where it is.
                if bit.bounces < 2 && bit.velocity.y < -180.0 {
                    bit.velocity.y = -bit.velocity.y * random.range(0.22, 0.38);
                    bit.velocity.x = bit.velocity.x * 0.5 + random.range(-90.0, 90.0);
                    bit.bounces += 1;
                } else if li == 0 {
                    bit.lying = Some(0.0);
                } else {
                    // Off a window's top edge it rolls over and falls on.
                    bit.velocity = V2::new(random.range(-60.0, 60.0), -40.0);
                    bit.position.y -= 1.0;
                    bit.bounces = 3;
                }
            }
        }
        e.hail.retain(|bit| bit.lying.is_none_or(|age| age < 2.5) && bit.position.y > -20.0);
        trim_lying(&mut e.hail);

        // Fireworks: a rocket now and then, climbing on a trail of sparks and bursting into a ball of them.
        if wants(Mode::Fireworks) && clock >= e.next_burst && e.sparks.len() < MAX_SPARKS {
            let random = &mut e.random;
            let x = random.range(width * 0.12, width * 0.88);
            let burst_at = random.range(height * 0.5, height * 0.88);
            let color = FIREWORK_HUES[(random.next() * FIREWORK_HUES.len() as f32) as usize % FIREWORK_HUES.len()];
            let base = floor(x);
            e.sparks.push(Spark {
                position: V2::new(x, base),
                velocity: V2::new(random.range(-30.0, 30.0), random.range(520.0, 640.0)),
                color,
                size: 4.0,
                age: 0.0,
                life: 3.0,
                kind: SparkKind::Rocket { burst_at },
            });
            e.next_burst = clock + (random.range(0.8, 2.6) * (1.6 - amount)) as f64;
        }
        let mut born: Vec<Spark> = Vec::new();
        for spark in e.sparks.iter_mut() {
            spark.age += dt;
            match spark.kind {
                SparkKind::Rocket { burst_at } => {
                    spark.velocity.y -= 160.0 * dt;
                    spark.position += spark.velocity * dt;
                    let random = &mut e.random;
                    born.push(Spark {
                        position: spark.position,
                        velocity: V2::new(random.range(-12.0, 12.0), random.range(-40.0, -10.0)),
                        color: [1.0, 0.82, 0.55],
                        size: random.range(2.0, 3.0),
                        age: 0.0,
                        life: random.range(0.3, 0.6),
                        kind: SparkKind::Trail,
                    });
                    if spark.position.y >= burst_at || spark.velocity.y < 60.0 {
                        spark.age = spark.life;
                        burst(&mut born, random, spark.position, spark.color);
                    }
                }
                SparkKind::Spark | SparkKind::Trail => {
                    spark.velocity *= (-1.6 * dt).exp();
                    spark.velocity.y -= 70.0 * dt;
                    spark.position += spark.velocity * dt;
                }
                SparkKind::Flash => {}
            }
        }
        e.sparks.retain(|spark| spark.age < spark.life);
        let room = MAX_SPARKS.saturating_sub(e.sparks.len());
        e.sparks.extend(born.into_iter().take(room));

        // Lightning: a stroke now and then while it storms, lighting the sky, sometimes flickering once more.
        let lightning = &mut e.lightning;
        lightning.brightness *= (-7.0 * dt).exp();
        if let Some(at) = lightning.flicker_at
            && clock >= at
        {
            lightning.brightness = 0.8;
            lightning.flicker_at = None;
        }
        if wants(Mode::Storm) && clock >= e.next_strike {
            let random = &mut e.random;
            lightning.x = random.range(width * 0.1, width * 0.9);
            let bottom = floor(lightning.x).max(height * random.range(0.25, 0.6));
            lightning.path = bolt(random, lightning.x, height, bottom);
            lightning.brightness = 1.0;
            lightning.flicker_at = (random.next() < 0.55).then(|| clock + random.range(0.08, 0.2) as f64);
            e.next_strike = clock + (random.range(3.0, 11.0) * (1.4 - amount * 0.8)) as f64;
        }

        // Northern lights: a few curtains hung across the top of the screen, fading in slowly and out again.
        let aurora = &mut e.aurora;
        let aurora_wanted = wants(Mode::Aurora);
        if aurora_wanted && aurora.curtains.is_empty() {
            let random = &mut e.random;
            const GREENS: [[f32; 3]; 3] = [[0.3, 1.0, 0.55], [0.25, 0.95, 0.75], [0.45, 1.0, 0.45]];
            for k in 0..3 {
                let from = random.range(-0.15, 0.35);
                aurora.curtains.push(Curtain {
                    base: random.range(0.6, 0.76) + k as f32 * 0.03,
                    from,
                    to: (from + random.range(0.6, 1.0)).min(1.15),
                    amplitude: random.range(30.0, 70.0),
                    waves: [random.range(0.002, 0.005), random.range(0.007, 0.013)],
                    speeds: [random.range(0.08, 0.2), random.range(0.15, 0.35)],
                    phase: random.range(0.0, TAU),
                    // Now and then one glows violet and pink instead of green.
                    color: if k == 2 && random.next() < 0.6 { [0.78, 0.45, 1.0] } else { GREENS[k] },
                });
            }
        }
        aurora.shown = (aurora.shown + dt / if aurora_wanted { 6.0 } else { -3.0 }).clamp(0.0, 1.0);
        if aurora.shown <= 0.0 && !aurora_wanted {
            aurora.curtains.clear();
        }

        // Sandstorm: a sandy haze driven across the screen, grains streaking through it, leaves tumbling along.
        let sand = wants(Mode::Sandstorm);
        let direction = if wind < 0.0 { -1.0 } else { 1.0 };
        let gust = |t: f64, y: f32| (0.75 + 0.35 * ((t * 0.6) as f32 + y * 0.003).sin() + 0.2 * ((t * 1.7) as f32 - y * 0.01).sin()).max(0.2);
        let haze_target = if sand { ((8.0 + 18.0 * amount) * area).min(70.0) as usize } else { 0 };
        while e.haze.len() < haze_target {
            let random = &mut e.random;
            let radius = random.range(160.0, 360.0);
            e.haze.push(Puff {
                position: V2::new(random.range(-radius, width + radius), random.next().powf(1.4) * height * 0.8),
                velocity: random.range(90.0, 180.0),
                radius,
                alpha: random.range(0.06, 0.12) * (0.5 + 0.5 * amount),
                shown: 0.0,
                phase: random.range(0.0, TAU),
            });
        }
        for (i, puff) in e.haze.iter_mut().enumerate() {
            let keep = i < haze_target;
            puff.shown = (puff.shown + dt / if keep { 3.0 } else { -2.0 }).clamp(0.0, 1.0);
            puff.phase += dt * 0.3;
            puff.position.x += direction * puff.velocity * gust(clock, puff.position.y) * dt;
            puff.position.y += puff.phase.sin() * 6.0 * dt;
            if direction > 0.0 && puff.position.x > width + puff.radius * 1.2 {
                puff.position.x = -puff.radius;
            } else if direction < 0.0 && puff.position.x < -puff.radius * 1.2 {
                puff.position.x = width + puff.radius;
            }
        }
        e.haze.retain(|puff| puff.shown > 0.0 || haze_target > 0);

        let grain_target = if sand { ((250.0 + 1400.0 * amount) * area).min(3500.0) as usize } else { 0 };
        while e.grains.len() < grain_target {
            let random = &mut e.random;
            e.grains.push(Grain {
                position: V2::new(random.range(0.0, width), random.next().powf(1.6) * height),
                velocity: V2::new(direction * random.range(380.0, 820.0), random.range(-30.0, 30.0)),
                size: random.range(1.1, 2.6),
                shade: random.next(),
                phase: random.range(0.0, TAU),
                shown: 0.0,
            });
        }
        for grain in e.grains.iter_mut() {
            grain.shown = (grain.shown + dt * 3.0).min(1.0);
            grain.phase += dt * (2.0 + grain.shade * 3.0);
            let push = gust(clock, grain.position.y);
            let lift = grain.phase.sin() * 60.0 + (grain.position.x * 0.01 + clock as f32 * 2.0).sin() * 40.0;
            grain.position += V2::new(grain.velocity.x * push, grain.velocity.y + lift) * dt;
            let floor_here = floor(grain.position.x);
            if grain.position.y < floor_here + 1.0 {
                grain.position.y = floor_here + 1.0;
                grain.velocity.y = grain.velocity.y.abs() + 20.0;
            }
            grain.velocity.y *= (-0.8 * dt).exp();
        }
        // Blown off the leeward side, a grain comes in again on the windward one while the storm lasts.
        for grain in e.grains.iter_mut() {
            let off = if direction > 0.0 { grain.position.x > width + 20.0 } else { grain.position.x < -20.0 };
            if off && sand {
                let random = &mut e.random;
                grain.position = V2::new(if direction > 0.0 { -10.0 } else { width + 10.0 }, random.next().powf(1.6) * height);
                grain.velocity = V2::new(direction * random.range(380.0, 820.0), random.range(-30.0, 30.0));
            } else if off {
                grain.shown = -1.0;
            }
        }
        e.grains.retain(|grain| grain.shown >= 0.0);
        e.grains.truncate(grain_target.max(if sand { 0 } else { e.grains.len() }));

        let debris_target = if sand { ((4.0 + 16.0 * amount) * area).min(40.0) as usize } else { 0 };
        while e.debris.len() < debris_target {
            let random = &mut e.random;
            e.debris.push(Debris {
                position: V2::new(if direction > 0.0 { -30.0 } else { width + 30.0 }, random.range(height * 0.03, height * 0.7)),
                velocity: V2::new(direction * random.range(260.0, 520.0), random.range(-20.0, 60.0)),
                seed: random.range(0.001, 1.0),
                phase: random.range(0.0, TAU),
                size: random.range(14.0, 22.0),
            });
        }
        for leaf in e.debris.iter_mut() {
            leaf.phase += dt * 2.2;
            let push = gust(clock, leaf.position.y);
            // Bobbing and skipping along: lifted by eddies, dropped again, bounced off the ground.
            leaf.velocity.y += (leaf.phase.sin() * 260.0 - 120.0) * dt;
            leaf.position += V2::new(leaf.velocity.x * push, leaf.velocity.y) * dt;
            let floor_here = floor(leaf.position.x);
            if leaf.position.y < floor_here + leaf.size * 0.4 {
                leaf.position.y = floor_here + leaf.size * 0.4;
                leaf.velocity.y = leaf.velocity.y.abs() * 0.6 + 60.0;
            }
            if leaf.position.y > height * 0.85 {
                leaf.velocity.y = -leaf.velocity.y.abs();
            }
        }
        e.debris.retain(|leaf| if direction > 0.0 { leaf.position.x < width + 40.0 } else { leaf.position.x > -40.0 });
    }

    /// The northern lights, fog and a sandstorm's haze, drawn first, under everything else.
    pub(super) fn fog_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let opacity = self.falling_opacity;
        let puff = move |tint: [f32; 3]| {
            move |puff: &Puff| Sprite {
                position: puff.position,
                size: puff.radius * 2.0,
                alpha: puff.alpha * smoothstep(0.0, 1.0, puff.shown) * (0.85 + 0.15 * puff.phase.cos()) * opacity,
                soft: 1.0,
                tint_amount: 1.0,
                tint,
                ..Sprite::default()
            }
        };
        let fog = self.extras.fog.iter().map(puff([0.86, 0.88, 0.92]));
        let haze = self.extras.haze.iter().map(puff([0.84, 0.68, 0.46]));
        self.aurora_sprites().chain(fog).chain(haze)
    }

    /// Each curtain of northern lights as a row of soft rays standing on its rippling lower edge, brighter here and
    /// fainter there, the brightness wandering along it.
    fn aurora_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let aurora = &self.extras.aurora;
        let (width, height) = (self.width, self.height);
        let t = self.air.clock as f32;
        let strength = smoothstep(0.0, 1.0, aurora.shown) * self.falling_opacity * (0.6 + 0.4 * self.intensity.clamp(0.0, 1.0));
        aurora.curtains.iter().enumerate().filter(move |_| strength > 0.0).flat_map(move |(k, curtain)| {
            let start = curtain.from * width;
            let end = curtain.to * width;
            let count = ((end - start) / RAY_SPACING).max(0.0) as usize;
            (0..count).map(move |n| {
                let x = start + n as f32 * RAY_SPACING;
                let along = (x - start) / (end - start).max(1.0);
                let ends = (along * PI).sin().max(0.0).powf(0.7);
                let edge = height * curtain.base
                    + curtain.amplitude * ((x * curtain.waves[0] + t * curtain.speeds[0] + curtain.phase).sin()
                        + 0.5 * (x * curtain.waves[1] - t * curtain.speeds[1] + curtain.phase * 2.0).sin());
                let pulse = 0.5 + 0.5 * (x * 0.009 + t * 0.45 + curtain.phase * 3.0).sin();
                let bright = ends * (0.25 + 0.75 * pulse * pulse);
                let tall = height * (0.16 + 0.1 * (x * 0.006 + t * 0.25 + k as f32).sin().abs());
                Sprite {
                    // The ray's foot is the bottom of its sprite, on the curtain's edge.
                    position: V2::new(x, edge + tall * 0.5),
                    size: tall,
                    alpha: 0.16 * bright * strength,
                    soft: 1.0,
                    tint_amount: 1.0,
                    tint: curtain.color,
                    lump: ((x * 0.0131 + k as f32 * 0.37).fract()).max(0.01),
                    // Four marks a ray of northern lights in the shader.
                    petal: 4.0,
                    ..Sprite::default()
                }
            })
        })
    }

    pub(super) fn extra_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let e = &self.extras;
        let opacity = self.falling_opacity;
        let glows = e.glows.iter().flat_map(move |glow| {
            // A firefly lights up and dims again in a soft pulse, never quite out.
            let pulse = glow.phase.sin().max(0.0).powi(3);
            let lit = (0.12 + 0.88 * pulse) * smoothstep(0.0, 1.0, glow.shown) * opacity;
            [
                Sprite { position: glow.position, size: 22.0, alpha: 0.28 * lit, soft: 1.0, tint_amount: 1.0, tint: [0.72, 1.0, 0.32], ..Sprite::default() },
                Sprite { position: glow.position, size: 3.6, alpha: 0.95 * lit, soft: 0.3, tint_amount: 1.0, tint: [0.96, 1.0, 0.62], ..Sprite::default() },
            ]
        });
        let confetti = e.confetti.iter().map(move |bit| {
            let fade = bit.lying.map_or(1.0, |age| 1.0 - smoothstep(20.0, 24.0, age));
            Sprite {
                position: bit.position,
                velocity: if bit.lying.is_some() { V2::ZERO } else { V2::new(0.0, bit.velocity.y) },
                size: bit.size,
                alpha: 0.95 * fade * opacity,
                soft: 0.1,
                tint_amount: 1.0,
                tint: bit.color,
                // Three marks confetti in the shader; its seed turns and flips it, stopped once it lies.
                lump: if bit.lying.is_some() { 0.5 } else { bit.seed },
                petal: 3.0,
                ..Sprite::default()
            }
        });
        let hail = e.hail.iter().map(move |bit| {
            let fade = bit.lying.map_or(1.0, |age| 1.0 - smoothstep(0.8, 2.5, age));
            Sprite {
                position: bit.position,
                velocity: if bit.lying.is_some() { V2::ZERO } else { bit.velocity },
                size: bit.size,
                alpha: 0.92 * fade * opacity,
                soft: 0.35,
                tint_amount: 0.5,
                tint: bit.color,
                ..Sprite::default()
            }
        });
        let sparks = e.sparks.iter().map(move |spark| {
            let t = spark.age / spark.life;
            let (alpha, size) = match spark.kind {
                SparkKind::Rocket { .. } => (1.0, spark.size),
                SparkKind::Trail => (0.7 * (1.0 - t), spark.size * (1.0 - t * 0.5)),
                // Sparks twinkle as they burn down.
                SparkKind::Spark => ((1.0 - t).powf(1.4) * (0.75 + 0.25 * (spark.age * 31.0 + spark.size * 7.0).sin()), spark.size * (1.0 - t * 0.4)),
                SparkKind::Flash => (0.35 * (1.0 - t).powi(2), spark.size),
            };
            let flash = spark.kind == SparkKind::Flash;
            Sprite {
                position: spark.position,
                velocity: spark.velocity,
                size,
                alpha: alpha * opacity,
                soft: if flash { 1.0 } else { 0.4 },
                tint_amount: 0.9,
                tint: spark.color,
                // Sparks draw with rays, as stars do; a burst's light is a plain soft glow.
                petal: if flash { 0.0 } else { 2.0 },
                ..Sprite::default()
            }
        });
        let lit = e.lightning.brightness;
        let shown = lit > 0.01;
        let sky = shown.then(|| Sprite {
            position: V2::new(e.lightning.x, self.height * 1.05),
            size: self.width.max(self.height) * 1.6,
            alpha: 0.32 * lit,
            soft: 1.0,
            tint_amount: 1.0,
            tint: [0.82, 0.88, 1.0],
            ..Sprite::default()
        });
        let bolt = e.lightning.path.iter().filter(move |_| shown).map(move |&point| Sprite {
            position: point,
            size: 3.2,
            alpha: lit,
            soft: 0.25,
            tint_amount: 0.4,
            tint: [0.85, 0.9, 1.0],
            ..Sprite::default()
        });
        let clock = self.air.clock;
        let grains = e.grains.iter().filter(|grain| grain.shown > 0.0).map(move |grain| Sprite {
            position: grain.position,
            velocity: grain.velocity,
            size: grain.size,
            alpha: (0.45 + 0.4 * grain.shade) * grain.shown * opacity,
            soft: 0.5,
            tint_amount: 1.0,
            tint: [0.78 + 0.14 * grain.shade, 0.6 + 0.12 * grain.shade, 0.38 + 0.1 * grain.shade],
            ..Sprite::default()
        });
        let debris = e.debris.iter().map(move |leaf| Sprite {
            position: leaf.position,
            velocity: leaf.velocity,
            size: drawn_size(false, leaf.size, leaf.seed),
            alpha: opacity,
            leaf: [leaf.seed, leaf.seed * 40.0 + spin(clock, 3.0 + leaf.seed * 4.0), tumble(clock * 3.0, leaf.seed)],
            spin: [3.0 + leaf.seed * 4.0 + spin_rate(false, leaf.seed), tumble_speed(clock * 3.0, leaf.seed) * 3.0],
            ..Sprite::default()
        });
        sky.into_iter().chain(glows).chain(confetti).chain(hail).chain(sparks).chain(bolt).chain(grains).chain(debris)
    }
}

/// A firework bursting: a ball of sparks in one or two colours, and its light for a moment.
fn burst(into: &mut Vec<Spark>, random: &mut Random, at: V2, color: [f32; 3]) {
    let second = if random.next() < 0.35 { FIREWORK_HUES[(random.next() * 6.0) as usize % 6] } else { color };
    let count = random.range(70.0, 120.0) as usize;
    let speed = random.range(150.0, 240.0);
    for i in 0..count {
        let angle = random.range(0.0, TAU);
        // Most sparks fly near the full speed, so the ball reads as a sphere rather than a cloud.
        let reach = speed * (0.75 + 0.25 * random.next().sqrt());
        into.push(Spark {
            position: at,
            velocity: V2::new(angle.cos(), angle.sin()) * reach,
            color: if i % 3 == 0 { second } else { color },
            size: random.range(3.5, 5.5),
            age: 0.0,
            life: random.range(1.2, 2.2),
            kind: SparkKind::Spark,
        });
    }
    into.push(Spark { position: at, velocity: V2::ZERO, color, size: 260.0, age: 0.0, life: 0.45, kind: SparkKind::Flash });
}

/// A jagged stroke of lightning from above the screen down to `bottom`, with a branch or two, as points close enough
/// together to read as a line.
fn bolt(random: &mut Random, x: f32, top: f32, bottom: f32) -> Vec<V2> {
    let mut path = Vec::new();
    let mut point = V2::new(x, top + 10.0);
    let mut drift = 0.0;
    let mut branches = 0;
    while point.y > bottom {
        drift = (drift + random.range(-1.6, 1.6)).clamp(-2.8, 2.8);
        let next = point + V2::new(drift + random.range(-1.2, 1.2), -random.range(2.5, 3.6));
        path.push(next);
        if branches < 2 && random.next() < 0.012 {
            branches += 1;
            let side = if random.next() < 0.5 { -1.0 } else { 1.0 };
            let mut fork = next;
            for _ in 0..random.range(20.0, 60.0) as usize {
                fork += V2::new(side * random.range(0.8, 2.4), -random.range(2.0, 3.2));
                path.push(fork);
            }
        }
        point = next;
    }
    path
}

/// Keeps no more than `MAX_LYING` lying, the oldest going first.
fn trim_lying(bits: &mut Vec<Bit>) {
    let lying = bits.iter().filter(|bit| bit.lying.is_some()).count();
    if lying > MAX_LYING {
        let mut extra = lying - MAX_LYING;
        bits.retain(|bit| {
            if extra > 0 && bit.lying.is_some() {
                extra -= 1;
                return false;
            }
            true
        });
    }
}
