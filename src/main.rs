use std::env;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::prelude::*;
use rand_pcg::Pcg32;

use simdnoise::NoiseBuilder;
use structopt::StructOpt;

/// Generate random terrain-like meshes using various types of noise functions.
#[derive(StructOpt)]
pub struct App {
    /// Output obj filename template.
    #[structopt(short, long, parse(from_os_str), default_value = "terrain.obj")]
    output: PathBuf,

    /// The width of the final terrain as in number of vertices.
    #[structopt(short, long, default_value = "51")]
    width: u16,

    /// The depth of the final terrain as in number of vertices.
    #[structopt(short, long, default_value = "51")]
    depth: u16,

    /// The maximum height of the terrain.
    #[structopt(short, long, default_value = "20")]
    amplitude: f32,

    /// The seed to use to generate the terrain. You can find the seed of a given terrain by
    /// inspecting the obj file.
    #[structopt(short, long)]
    seed: Option<u64>,

    #[structopt(long, default_value = "0.5")]
    lacunarity: f32,

    #[structopt(long, default_value = "4")]
    octaves: u8,

    #[structopt(long, default_value = "2.0")]
    gain: f32,

    #[structopt(long, default_value = "0.2")]
    frequency: f32,

    /// Generate the dual of terrain too.
    #[structopt(long)]
    dual: bool,
}

#[derive(Debug)]
pub struct Terrain {
    heights: Vec<f32>,
    width: usize,
    depth: usize,
    amplitude: f32,
    generator: TerrainGenerator,
}

#[derive(Debug, Clone)]
pub enum TerrainGenerator {
    Noise { seed: u64 },
    Dual { parent_seed: u64 },
}

impl Terrain {
    pub fn generate(
        App {
            amplitude,
            depth,
            frequency,
            gain,
            lacunarity,
            octaves,
            seed,
            width,
            ..
        }: &App,
    ) -> Self {
        // it seems there isn't a way to automatically randomize the noise functions, revert to
        // simply looking at different areas in the noise space.
        let seed = seed.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time drift detected, aborting")
                .as_secs()
        });

        let mut rng = Pcg32::seed_from_u64(seed);
        let width_offset = rng.gen_range(-f32::from(width / 2), f32::from(width / 2));
        let depth_offset = rng.gen_range(-f32::from(depth / 2), f32::from(depth / 2));

        let width = usize::from(*width);
        let depth = usize::from(*depth);

        let mut noise_config =
            NoiseBuilder::fbm_2d_offset(width_offset, width, depth_offset, depth);
        noise_config
            .with_octaves(*octaves)
            .with_freq(*frequency)
            .with_gain(*gain)
            .with_lacunarity(*lacunarity);

        let heights = noise_config.generate_scaled(0.0, *amplitude);

        Terrain {
            depth,
            heights,
            width,
            amplitude: *amplitude,
            generator: TerrainGenerator::Noise { seed },
        }
    }

    pub fn dual(&self) -> Terrain {
        let heights = self
            .positions_by_depth()
            .map(|(y, x)| self.amplitude - self.height_at(self.width - 1 - x, y))
            .collect::<Vec<_>>();

        let generator = match self.generator {
            TerrainGenerator::Noise { seed } => TerrainGenerator::Dual { parent_seed: seed },
            TerrainGenerator::Dual { parent_seed } => TerrainGenerator::Noise { seed: parent_seed },
        };

        Terrain {
            heights,
            generator,
            ..*self
        }
    }

    pub fn height_at(&self, x: usize, y: usize) -> f32 {
        self.heights[y * self.width + x]
    }

    pub fn iter_by_depth(&self) -> impl Iterator<Item = (usize, usize, f32)> + '_ {
        self.heights
            .iter()
            .enumerate()
            .map(move |(i, z)| (i / self.width, i % self.width, *z))
    }

    pub fn positions_by_depth(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..self.depth).flat_map(move |y| (0..self.width).map(move |x| (y, x)))
    }

    pub fn index_of(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn amplitude(&self) -> f32 {
        self.amplitude
    }

    pub fn generator(&self) -> &TerrainGenerator {
        &self.generator
    }
}

fn main() -> io::Result<()> {
    let opt = App::from_args();

    let terrain = Terrain::generate(&opt);

    let mut f = BufWriter::new(File::create(&opt.output)?);
    dump(&mut f, &terrain, true)?;

    if opt.dual {
        let dual = terrain.dual();

        let dual_output = opt.output.with_file_name(format!(
            "{}-dual.{}",
            opt.output
                .file_stem()
                .map_or_else(|| "terrain".into(), |oss| oss.to_string_lossy()),
            opt.output
                .extension()
                .map_or_else(|| "obj".into(), |oss| oss.to_string_lossy()),
        ));

        let mut f = BufWriter::new(File::create(dual_output)?);
        dump(&mut f, &dual, true)?;
    }

    Ok(())
}

pub fn dump(w: &mut impl Write, terrain: &Terrain, support: bool) -> io::Result<()> {
    writeln!(
        w,
        r#"# generated by terrain-mesh <https://github.com/d-dorazio/terrain-mesh>
# {}
o terrain"#,
        env::args().collect::<Vec<_>>().join(" ")
    )?;

    for (y, x, z) in terrain.iter_by_depth() {
        writeln!(w, "v {} {} {}", x, y, z)?;
    }

    if support {
        for (y, x) in terrain.positions_by_depth() {
            writeln!(w, "v {} {} 0", x, y)?;
        }
    }

    let depth = terrain.depth();
    let width = terrain.width();
    for y in 0..depth.saturating_sub(1) {
        for x in 0..width.saturating_sub(1) {
            let i = 1 + terrain.index_of(x, y);
            let j = 1 + terrain.index_of(x, y + 1);
            writeln!(w, "f {} {} {} {}", i, i + 1, j + 1, j)?;
        }
    }

    if support {
        let oi = width * depth + 1;
        writeln!(
            w,
            "f {} {} {} {}",
            oi,
            oi + terrain.index_of(0, depth - 1),
            oi + terrain.index_of(width - 1, depth - 1),
            oi + terrain.index_of(width - 1, 0),
        )?;

        for y in 0..depth.saturating_sub(1) {
            writeln!(
                w,
                "f {} {} {} {}",
                oi + terrain.index_of(0, y + 1),
                oi + terrain.index_of(0, y),
                1 + terrain.index_of(0, y),
                1 + terrain.index_of(0, y + 1),
            )?;

            writeln!(
                w,
                "f {} {} {} {}",
                oi + terrain.index_of(width - 1, y),
                oi + terrain.index_of(width - 1, y + 1),
                1 + terrain.index_of(width - 1, y + 1),
                1 + terrain.index_of(width - 1, y),
            )?;
        }

        for x in 0..width.saturating_sub(1) {
            writeln!(
                w,
                "f {} {} {} {}",
                oi + terrain.index_of(x, 0),
                oi + terrain.index_of(x + 1, 0),
                1 + terrain.index_of(x + 1, 0),
                1 + terrain.index_of(x, 0),
            )?;

            writeln!(
                w,
                "f {} {} {} {}",
                oi + terrain.index_of(x + 1, depth - 1),
                oi + terrain.index_of(x, depth - 1),
                1 + terrain.index_of(x, depth - 1),
                1 + terrain.index_of(x + 1, depth - 1),
            )?;
        }
    }

    Ok(())
}
