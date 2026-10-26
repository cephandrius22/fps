//! Builds the test prop: a wooden barrel, made in code so it needs no
//! modeling tool, written as .glb.
//!
//! ```text
//!   cargo run -p props --example barrel -- assets/models/barrel.glb
//! ```
//!
//! Two meshes (staves and lids), each with its own pixel-art texture;
//! the origin is the middle of the bottom, so it stands where placed.

use std::f32::consts::TAU;

use base::Image;
use glam::{Vec2, Vec3};
use props::{Mesh, Prop};

const SIDES: u32 = 16;
/// Meters.
const RADIUS: f32 = 0.35;
const HEIGHT: f32 = 0.9;
/// The stave texture wraps this many times around.
const WRAPS: f32 = 2.0;
const TEXELS: u32 = 32;
const WHITE: [f32; 4] = [1.0; 4];
const WOOD: [u8; 3] = [120, 78, 40];
const SEAM: [u8; 3] = [70, 44, 22];
const HOOP: [u8; 3] = [90, 90, 96];
/// Texel columns between stave seams, and rows the hoops cover.
const STAVE_TEXELS: u32 = 8;
const HOOP_ROWS: [std::ops::Range<u32>; 2] = [5..8, 24..27];

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: barrel <output.glb>"))?;

    let prop = Prop {
        meshes: vec![staves(), lids()],
        textures: vec![stave_texture(), lid_texture()],
    };
    std::fs::write(&path, props::write(&prop)?)?;
    println!("wrote {path}");
    Ok(())
}

/// The point on the rim at `angle` and height `y`.
fn rim(angle: f32, y: f32) -> Vec3 {
    Vec3::new(RADIUS * angle.cos(), y, RADIUS * angle.sin())
}

/// The sides: one quad per side, normals pointing out.
fn staves() -> Mesh {
    let mut mesh = empty(Some(0));
    for i in 0..=SIDES {
        let angle = TAU * i as f32 / SIDES as f32;
        let u = WRAPS * i as f32 / SIDES as f32;
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        for (y, v) in [(0.0, 1.0), (HEIGHT, 0.0)] {
            mesh.positions.push(rim(angle, y));
            mesh.normals.push(out);
            mesh.uvs.push(Vec2::new(u, v));
        }
    }

    // Vertices 2i (bottom) and 2i + 1 (top) at side i's edge.
    for i in 0..SIDES {
        let [b0, t0, b1, t1] = [2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3];
        mesh.indices.extend([b0, t0, b1, b1, t0, t1]);
    }
    mesh
}

/// Top and bottom: fans around their centers.
fn lids() -> Mesh {
    let mut mesh = empty(Some(1));
    for (y, normal) in [(HEIGHT, Vec3::Y), (0.0, Vec3::NEG_Y)] {
        let center = mesh.positions.len() as u32;
        mesh.positions.push(Vec3::new(0.0, y, 0.0));
        mesh.normals.push(normal);
        mesh.uvs.push(Vec2::splat(0.5));

        for i in 0..SIDES {
            let angle = TAU * i as f32 / SIDES as f32;
            mesh.positions.push(rim(angle, y));
            mesh.normals.push(normal);
            mesh.uvs
                .push(Vec2::new(0.5 + 0.5 * angle.cos(), 0.5 + 0.5 * angle.sin()));
        }

        // Counter-clockwise seen from outside: reversed underneath.
        for i in 0..SIDES {
            let (a, b) = (center + 1 + i, center + 1 + (i + 1) % SIDES);
            match normal == Vec3::Y {
                true => mesh.indices.extend([center, b, a]),
                false => mesh.indices.extend([center, a, b]),
            }
        }
    }
    mesh
}

fn empty(texture: Option<usize>) -> Mesh {
    Mesh {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
        texture,
        color: WHITE,
    }
}

/// Wood with dark seams between staves and two metal hoops.
fn stave_texture() -> Image {
    texture(|x, y| {
        if HOOP_ROWS.iter().any(|rows| rows.contains(&y)) {
            return HOOP;
        }
        if x % STAVE_TEXELS == 0 {
            return SEAM;
        }
        WOOD
    })
}

/// Wood with a dark ring at the rim.
fn lid_texture() -> Image {
    let center = TEXELS as f32 / 2.0;
    texture(|x, y| {
        let r = Vec2::new(x as f32 + 0.5 - center, y as f32 + 0.5 - center).length();
        match r > center - 3.0 {
            true => SEAM,
            false => WOOD,
        }
    })
}

fn texture(color: impl Fn(u32, u32) -> [u8; 3]) -> Image {
    let rgba = (0..TEXELS)
        .flat_map(|y| (0..TEXELS).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let [r, g, b] = color(x, y);
            [r, g, b, u8::MAX]
        })
        .collect();
    Image::new(TEXELS, TEXELS, rgba).expect("size matches")
}
