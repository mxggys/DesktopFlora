//! DesktopFlora's own weathers beside Snowhush's: a thunderstorm's lightning and gathering cloud over its rain, the mist
//! heavy rain raises, fog drifting in banks, and hail bouncing off the windows and the ground. Each keeps its own
//! particles, which only come while its kind is wanted and go their own way once it isn't: fog and mist fade, hail
//! falls out and melts.

use std::f32::consts::TAU;

use super::{Mode, Random, SnowSimulation, Sprite, landing};
use crate::geometry::{V2, smoothstep};

/// How much of a screen a megapoint is, to scale how many of each there are.
const MEGAPOINT: f32 = 1_000_000.0;
/// Hailstones lying on the ground at once, at most.
const MAX_LYING: usize = 500;

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


/// A hailstone: falling, or lying on the ground until it melts.
#[derive(Clone, Copy)]
struct Bit {
    position: V2,
    velocity: V2,
    size: f32,
    color: [f32; 3],
    /// Seconds since it came to rest, while it lies.
    lying: Option<f32>,
    /// A hailstone's bounces so far.
    bounces: u8,
}


/// A stroke of lightning: its path down the sky, how bright it is now, and when it flickers again.
#[derive(Default)]
struct Lightning {
    /// Points along the stroke, close enough to read as a line, each with how thick it is there: the main channel
    /// thick and tapering, its branches thin.
    path: Vec<(V2, f32)>,
    brightness: f32,
    flicker_at: Option<f64>,
    x: f32,
}

/// A splinter of ice knocked off a hailstone as it strikes.
#[derive(Clone, Copy)]
struct Shard {
    position: V2,
    velocity: V2,
    age: f32,
}

pub(super) struct Extras {
    random: Random,
    shards: Vec<Shard>,
    /// How many strokes of lightning there have been, for the thunder after each.
    strikes: u32,
    fog: Vec<Puff>,
    hail: Vec<Bit>,
    graupel: Vec<Bit>,
    lightning: Lightning,
    next_strike: f64,
    /// The fine spray heavy rain hangs low over the ground in, and how far a storm has darkened the sky.
    mist: Vec<Puff>,
    gloom: f32,
}

impl Default for Extras {
    fn default() -> Extras {
        Extras {
            random: Random::new(),
            shards: Vec::new(),
            strikes: 0,
            fog: Vec::new(),
            hail: Vec::new(),
            graupel: Vec::new(),
            lightning: Lightning::default(),
            next_strike: 2.0,
            mist: Vec::new(),
            gloom: 0.0,
        }
    }
}

impl Extras {
    pub(super) fn sprite_count(&self) -> usize {
        self.fog.len()
            + self.hail.len()
            + self.shards.len()
            + self.graupel.len()
            + if self.lightning.brightness > 0.01 { self.lightning.path.len() * 2 + 2 } else { 0 }
            + self.mist.len()
            + if self.gloom > 0.0 { 3 } else { 0 }
    }

    pub(super) fn flash(&self) -> f32 {
        self.lightning.brightness
    }

    pub(super) fn strikes(&self) -> u32 {
        self.strikes
    }

    /// Whether anything moves fast enough to be drawn at more than the calm rate.
    pub(super) fn lively(&self) -> bool {
        self.lightning.brightness > 0.01 || !self.shards.is_empty()
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
        let ground = &self.layers[0];
        let floor = |x: f32| ground.surface(ground.clamped_index(x.clamp(0.0, width - 1.0)));
        // The air a moving cursor drags along stirs up what it passes, while it leaves a trail.
        let wake = if self.air.trail { self.air.cursor_wake() } else { None };
        let stir = move |at: V2, reach: f32| -> V2 {
            let Some((cursor, velocity)) = wake else { return V2::ZERO };
            let distance = (at - cursor).length();
            if distance >= reach {
                return V2::ZERO;
            }
            let near = 1.0 - distance / reach;
            velocity * (near * near)
        };
        let e = &mut self.extras;

        // Fog: puffs keep their number while fog is wanted, and all fade away once it isn't.
        // Wide, flat banks, layered: the lowest densest, the higher ones thinner and slower, drifting the same way.
        let fog_target = if wants(Mode::Fog) { ((5.0 + 11.0 * amount) * area).min(32.0) as usize } else { 0 };
        let fog_way = if wind < 0.0 { -1.0 } else { 1.0 };
        while e.fog.len() < fog_target {
            let random = &mut e.random;
            let radius = random.range(260.0, 520.0);
            let height_share = random.next().powf(1.5);
            e.fog.push(Puff {
                position: V2::new(random.range(-radius, width + radius), height_share * height * 0.45 - radius * 0.08),
                velocity: fog_way * random.range(6.0, 16.0) * (1.0 - 0.5 * height_share),
                radius,
                alpha: random.range(0.2, 0.34) * (1.0 - 0.45 * height_share) * (0.55 + 0.45 * amount),
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
            puff.position += stir(puff.position, puff.radius * 0.9) * 0.35 * dt;
            // Off one side, it comes back in on the other.
            if puff.position.x < -puff.radius * 1.2 {
                puff.position.x = width + puff.radius;
            } else if puff.position.x > width + puff.radius * 1.2 {
                puff.position.x = -puff.radius;
            }
        }
        e.fog.retain(|puff| puff.shown > 0.0 || fog_wanted);

        // Rain: the spray it raises hangs low over the ground as a faint mist, thicker the harder it rains; a storm
        // darkens the sky as it gathers and lets it lighten again as it passes.
        let rain_wanted = wants(Mode::Rain) || wants(Mode::Storm);
        let mist_target = if rain_wanted { ((3.0 + 9.0 * amount) * area).min(30.0) as usize } else { 0 };
        while e.mist.len() < mist_target {
            let random = &mut e.random;
            let radius = random.range(140.0, 260.0);
            e.mist.push(Puff {
                position: V2::new(random.range(-radius, width + radius), random.range(-radius * 0.35, radius * 0.25)),
                velocity: random.range(4.0, 10.0) * if random.next() < 0.5 { -1.0 } else { 1.0 },
                radius,
                alpha: random.range(0.035, 0.07) * (0.4 + 0.6 * amount) * if wants(Mode::Storm) { 1.4 } else { 1.0 },
                shown: 0.0,
                phase: random.range(0.0, TAU),
            });
        }
        for (i, puff) in e.mist.iter_mut().enumerate() {
            let keep = i < mist_target;
            puff.shown = (puff.shown + dt / if keep { 5.0 } else { -2.5 }).clamp(0.0, 1.0);
            puff.phase += dt * 0.2;
            puff.position.x += (puff.velocity + wind * 0.4) * dt;
            if puff.position.x < -puff.radius * 1.2 {
                puff.position.x = width + puff.radius;
            } else if puff.position.x > width + puff.radius * 1.2 {
                puff.position.x = -puff.radius;
            }
        }
        e.mist.retain(|puff| puff.shown > 0.0 || mist_target > 0);
        let gloom_target = if wants(Mode::Storm) { 0.5 + 0.5 * amount } else { 0.0 };
        e.gloom += (gloom_target - e.gloom) * (dt / 6.0).min(1.0);
        if e.gloom < 0.005 && gloom_target == 0.0 {
            e.gloom = 0.0;
        }


        // Hail, hard and fast, and graupel, soft and slow: bouncing off window tops and the ground, then lying a
        // while before they melt.
        let layers = &self.layers;
        for (spec, bits) in [(&HAIL, &mut e.hail), (&GRAUPEL, &mut e.graupel)] {
            let target = if wants(spec.mode) { ((spec.count.0 + spec.count.1 * amount) * area).min(spec.count.2) as usize } else { 0 };
            pelt(bits, &mut e.shards, &mut e.random, layers, &stir, spec, target, wind, width, height, dt);
        }
        for shard in e.shards.iter_mut() {
            shard.age += dt;
            shard.velocity.y -= 900.0 * dt;
            shard.position += shard.velocity * dt;
        }
        e.shards.retain(|shard| shard.age < 0.35);

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
            e.strikes += 1;
            lightning.flicker_at = (random.next() < 0.55).then(|| clock + random.range(0.08, 0.2) as f64);
            e.next_strike = clock + (random.range(3.0, 11.0) * (1.4 - amount * 0.8)) as f64;
        }

    }

    /// A storm's cloud, fog and the rain's mist, drawn first, under everything else.
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
        // Fog draws as banks of wisps in the shader, each its own pattern by its seed.
        let bank = move |tint: [f32; 3]| {
            move |puff: &Puff| Sprite {
                alpha: puff.alpha * smoothstep(0.0, 1.0, puff.shown) * opacity,
                // Steady for the bank, so its wisps only drift: by its size, which never changes.
                lump: (puff.radius * 0.0137).fract().max(0.01),
                petal: 7.0,
                ..puff_sprite(puff, tint)
            }
        };
        let fog = self.extras.fog.iter().map(bank([0.88, 0.9, 0.93]));
        let mist = self.extras.mist.iter().map(puff([0.78, 0.82, 0.88]));
        // A storm's heavy cloud: three broad dark shadows over the top of the screen, faint enough to keep the
        // desktop readable, drifting slowly. Lightning lights the sky over them.
        let gloom = self.extras.gloom;
        let (width, height) = (self.width, self.height);
        let t = self.air.clock as f32;
        let clouds = (0..3).filter(move |_| gloom > 0.0).map(move |k| {
            let k = k as f32;
            Sprite {
                position: V2::new(width * (0.2 + 0.3 * k) + (t * 0.01 + k * 2.1).sin() * width * 0.08, height * (1.02 - 0.03 * k)),
                size: width.max(height) * 1.1,
                alpha: 0.2 * gloom * opacity,
                soft: 1.0,
                tint_amount: 1.0,
                tint: [0.06, 0.07, 0.1],
                ..Sprite::default()
            }
        });
        clouds.chain(fog).chain(mist)
    }


    pub(super) fn extra_sprites(&self) -> impl Iterator<Item = Sprite> + '_ {
        let e = &self.extras;
        let opacity = self.falling_opacity;
        let pellet = move |spec: &'static Pellets| {
            move |bit: &Bit| {
                let fade = bit.lying.map_or(1.0, |age| 1.0 - smoothstep(spec.lie * 0.3, spec.lie, age));
                Sprite {
                    position: bit.position,
                    velocity: if bit.lying.is_some() { V2::ZERO } else { bit.velocity },
                    size: bit.size,
                    alpha: spec.alpha * fade * opacity,
                    soft: spec.soft,
                    tint_amount: spec.tint_amount,
                    tint: bit.color,
                    // Nine marks a pellet in the shader; its look tells hail from graupel.
                    lump: spec.look,
                    petal: 9.0,
                    ..Sprite::default()
                }
            }
        };
        let shards = e.shards.iter().map(move |shard| Sprite {
            position: shard.position,
            velocity: shard.velocity,
            size: 1.0,
            alpha: 0.8 * (1.0 - shard.age / 0.35) * opacity,
            soft: 0.3,
            tint_amount: 0.5,
            tint: [0.93, 0.97, 1.0],
            ..Sprite::default()
        });
        let hail = e.hail.iter().map(pellet(&HAIL)).chain(e.graupel.iter().map(pellet(&GRAUPEL))).chain(shards);
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
        // A white-hot core along the stroke, in a violet-blue glow that is wider and fainter; a glow too where it
        // strikes.
        let bolt = e.lightning.path.iter().enumerate().filter(move |_| shown).flat_map(move |(i, &(point, thick))| {
            let glow = (i % 3 == 0).then(|| Sprite {
                position: point,
                size: 10.0 + thick * 7.0,
                alpha: 0.13 * lit * thick.min(1.0),
                soft: 1.0,
                tint_amount: 1.0,
                tint: [0.62, 0.66, 1.0],
                ..Sprite::default()
            });
            let core = Sprite {
                position: point,
                size: 1.2 + 2.2 * thick,
                alpha: lit * (0.55 + 0.45 * thick.min(1.0)),
                soft: 0.3,
                tint_amount: 0.25,
                tint: [0.9, 0.93, 1.0],
                ..Sprite::default()
            };
            glow.into_iter().chain(std::iter::once(core))
        });
        let strike = e.lightning.path.iter().filter(|&&(_, thick)| thick > 0.5).last().copied().filter(|_| shown).map(|(point, _)| Sprite {
            position: point,
            size: 120.0,
            alpha: 0.22 * lit,
            soft: 1.0,
            tint_amount: 1.0,
            tint: [0.75, 0.8, 1.0],
            ..Sprite::default()
        });
        sky.into_iter().chain(strike).chain(hail).chain(bolt)
    }
}

fn puff_sprite(puff: &Puff, tint: [f32; 3]) -> Sprite {
    Sprite { position: puff.position, size: puff.radius * 2.0, soft: 1.0, tint_amount: 1.0, tint, ..Sprite::default() }
}


/// A jagged stroke of lightning from above the screen down to `bottom`, with a branch or two, as points close enough
/// together to read as a line.
fn bolt(random: &mut Random, x: f32, top: f32, bottom: f32) -> Vec<(V2, f32)> {
    let mut path = Vec::new();
    let mut point = V2::new(x, top + 10.0);
    let mut drift = 0.0;
    let mut branches = 0;
    let span = (top + 10.0 - bottom).max(1.0);
    while point.y > bottom {
        drift = (drift + random.range(-1.6, 1.6)).clamp(-2.8, 2.8);
        let next = point + V2::new(drift + random.range(-1.2, 1.2), -random.range(2.5, 3.6));
        // The channel thins a little on its way down.
        let thick = 1.0 - 0.35 * ((top + 10.0 - next.y) / span).clamp(0.0, 1.0);
        path.push((next, thick));
        if branches < 3 && random.next() < 0.014 {
            branches += 1;
            let side = if random.next() < 0.5 { -1.0 } else { 1.0 };
            let mut fork = next;
            let length = random.range(20.0, 70.0) as usize;
            for n in 0..length {
                fork += V2::new(side * random.range(0.8, 2.4), -random.range(2.0, 3.2));
                // A branch starts thinner than the channel and fades out to nothing.
                path.push((fork, 0.45 * (1.0 - n as f32 / length as f32) + 0.05));
            }
        }
        point = next;
    }
    path
}

/// How a kind of pellet falls, bounces, lies and looks.
struct Pellets {
    mode: Mode,
    /// How many fall at least, how many more at full intensity, per megapoint, and how many at most.
    count: (f32, f32, f32),
    fall: (f32, f32),
    size: (f32, f32),
    gravity: f32,
    bounces: u8,
    /// Slower than this, it doesn't bounce again.
    bounce_speed: f32,
    restitution: (f32, f32),
    /// How long it lies before it has melted, in seconds.
    lie: f32,
    color: [f32; 3],
    alpha: f32,
    soft: f32,
    tint_amount: f32,
    /// How the shader draws it: as clear ice with a white core, or as soft matte snow.
    look: f32,
    shatters: bool,
}

/// Hard, clear ice, fast, bouncing high and melting soon.
const HAIL: Pellets = Pellets {
    mode: Mode::Hail,
    count: (40.0, 280.0, 800.0),
    fall: (520.0, 780.0),
    size: (2.2, 4.2),
    gravity: 1400.0,
    bounces: 2,
    bounce_speed: 180.0,
    restitution: (0.22, 0.38),
    lie: 2.5,
    color: [0.93, 0.97, 1.0],
    alpha: 0.92,
    soft: 0.35,
    tint_amount: 0.5,
    look: 0.25,
    shatters: true,
};

/// Soft, white, opaque snow pellets: slower, hardly bouncing, lying a good while.
const GRAUPEL: Pellets = Pellets {
    mode: Mode::Graupel,
    count: (60.0, 360.0, 1000.0),
    fall: (260.0, 380.0),
    size: (1.8, 3.2),
    gravity: 900.0,
    bounces: 1,
    bounce_speed: 140.0,
    restitution: (0.1, 0.22),
    lie: 9.0,
    color: [0.97, 0.97, 0.99],
    alpha: 0.95,
    soft: 0.55,
    tint_amount: 0.8,
    look: 0.75,
    shatters: false,
};

/// Keeps so many pellets falling, and moves them along: falling, bouncing, rolling off windows, lying, melting.
#[allow(clippy::too_many_arguments)]
fn pelt(
    bits: &mut Vec<Bit>,
    shards: &mut Vec<Shard>,
    random: &mut Random,
    layers: &[crate::layer::SnowLayer],
    stir: &dyn Fn(V2, f32) -> V2,
    spec: &Pellets,
    target: usize,
    wind: f32,
    width: f32,
    height: f32,
    dt: f32,
) {
    let falling = bits.iter().filter(|bit| bit.lying.is_none() && bit.bounces == 0).count();
    for _ in falling..target {
        let fall = random.range(spec.fall.0, spec.fall.1);
        bits.push(Bit {
            position: V2::new(random.range(-40.0, width + 40.0), height + random.range(10.0, height)),
            velocity: V2::new(wind * 2.5 * (spec.fall.0 / 520.0), -fall),
            size: random.range(spec.size.0, spec.size.1),
            color: spec.color,
            lying: None,
            bounces: 0,
        });
    }
    for bit in bits.iter_mut() {
        let push = stir(bit.position, 50.0);
        if let Some(age) = bit.lying.as_mut() {
            *age += dt;
            if push.length() > 150.0 {
                // Kicked by the cursor, it skitters off across the ground.
                bit.lying = None;
                bit.position.y += 1.0;
                bit.velocity = V2::new(push.x * 0.9, push.length() * 0.4 + 120.0);
                bit.bounces = 1;
            }
            continue;
        }
        bit.velocity += push * 2.0 * dt;
        let before = bit.position.y;
        bit.velocity.y -= spec.gravity * dt;
        bit.position += bit.velocity * dt;
        if let Some((li, i)) = landing(layers, bit.position.x, before, bit.position.y, true, true) {
            let surface = layers[li].surface(i);
            bit.position.y = surface + bit.size * 0.5;
            // Each bounce takes most of its speed; slow enough, it stays where it is.
            // Hail striking hard knocks a few splinters off.
            if spec.shatters && bit.bounces == 0 && bit.velocity.y < -400.0 && shards.len() < 600 {
                for _ in 0..3 {
                    shards.push(Shard {
                        position: bit.position,
                        velocity: V2::new(random.range(-120.0, 120.0), random.range(60.0, 200.0)),
                        age: 0.0,
                    });
                }
            }
            if bit.bounces < spec.bounces && bit.velocity.y < -spec.bounce_speed {
                bit.velocity.y = -bit.velocity.y * random.range(spec.restitution.0, spec.restitution.1);
                bit.velocity.x = bit.velocity.x * 0.5 + random.range(-90.0, 90.0) * spec.restitution.1;
                bit.bounces += 1;
            } else if li == 0 {
                bit.lying = Some(0.0);
            } else {
                // Off a window's top edge it rolls over and falls on.
                bit.velocity = V2::new(random.range(-60.0, 60.0), -40.0);
                bit.position.y -= 1.0;
                bit.bounces = spec.bounces + 1;
            }
        }
    }
    bits.retain(|bit| bit.lying.is_none_or(|age| age < spec.lie) && bit.position.y > -20.0);
    trim_lying(bits);
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
