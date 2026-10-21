//! Rastériseur logiciel avec tampon de profondeur (z-buffer), multi-thread.
//!
//! Remplace l'algorithme du peintre, faux dès que des faces se chevauchent en profondeur
//! (grands n-gones, objets imbriqués). Produit aussi un tampon d'identifiants d'objets,
//! utilisé pour la sélection au clic et le contour de sélection façon Blender.

use easyuv::math::V3;

/// Pyramide de mipmaps de la texture damier (RGBA8, lignes de haut en bas).
pub struct Mips {
    pub levels: Vec<(usize, Vec<[u8; 4]>)>,
}

impl Mips {
    pub fn new(size: usize, rgba: &[u8]) -> Self {
        let base: Vec<[u8; 4]> = rgba.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        let mut levels = vec![(size, base)];
        while levels.last().unwrap().0 > 1 {
            let (s, prev) = levels.last().unwrap();
            let n = s / 2;
            let mut next = Vec::with_capacity(n * n);
            for y in 0..n {
                for x in 0..n {
                    let mut acc = [0u32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let p = prev[(2 * y + dy) * s + 2 * x + dx];
                        for k in 0..4 {
                            acc[k] += p[k] as u32;
                        }
                    }
                    next.push([(acc[0] / 4) as u8, (acc[1] / 4) as u8, (acc[2] / 4) as u8, (acc[3] / 4) as u8]);
                }
            }
            levels.push((n, next));
        }
        Mips { levels }
    }

    #[inline]
    fn sample(&self, lod: usize, u: f32, v: f32) -> [u8; 4] {
        let (s, px) = &self.levels[lod.min(self.levels.len() - 1)];
        let fu = u - u.floor();
        let fv = 1.0 - (v - v.floor());
        let x = ((fu * *s as f32) as usize).min(s - 1);
        let y = ((fv * *s as f32) as usize).min(s - 1);
        px[y * s + x]
    }
}

/// Face à dessiner, préparée par l'appelant.
pub struct FaceStyle {
    pub color: [u8; 3],
    pub textured: bool,
    /// Identifiant d'objet + 1 (0 = fond).
    pub id: u32,
}

pub struct View {
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    pub fwd: V3,
    /// Focale en pixels physiques.
    pub focal: f64,
    pub near: f64,
    pub width: usize,
    pub height: usize,
}

pub struct Line {
    pub a: V3,
    pub b: V3,
    pub color: [u8; 4],
    pub width: u8,
}

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub ids: Vec<u32>,
}

#[derive(Clone, Copy)]
struct SV {
    x: f32,
    y: f32,
    iz: f32,
    u: f32,
    v: f32,
}

struct Tri {
    v: [SV; 3],
    face: u32,
    lod: u8,
}

/// Sommet en espace vue (x droite, y haut, z profondeur).
#[derive(Clone, Copy)]
struct VV {
    x: f64,
    y: f64,
    z: f64,
    u: f32,
    v: f32,
}

fn to_view(view: &View, p: V3) -> (f64, f64, f64) {
    let d = p - view.eye;
    (d.dot(view.right), d.dot(view.up), d.dot(view.fwd))
}

fn project(view: &View, p: VV) -> SV {
    let iz = 1.0 / p.z;
    SV {
        x: (view.width as f64 * 0.5 + p.x * iz * view.focal) as f32,
        y: (view.height as f64 * 0.5 - p.y * iz * view.focal) as f32,
        iz: iz as f32,
        u: p.u * iz as f32,
        v: p.v * iz as f32,
    }
}

/// Découpe un polygone contre le plan proche z >= near (Sutherland-Hodgman).
fn clip_near(poly: &[VV], near: f64) -> Vec<VV> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let ina = a.z >= near;
        let inb = b.z >= near;
        if ina {
            out.push(a);
        }
        if ina != inb {
            let t = (near - a.z) / (b.z - a.z);
            let lerp = |x: f64, y: f64| x + (y - x) * t;
            out.push(VV {
                x: lerp(a.x, b.x),
                y: lerp(a.y, b.y),
                z: near,
                u: a.u + (b.u - a.u) * t as f32,
                v: a.v + (b.v - a.v) * t as f32,
            });
        }
    }
    out
}

/// Données de maillage à rendre.
pub struct Geometry<'a> {
    pub positions: &'a [V3],
    pub corners: &'a [u32],
    pub face_start: &'a [usize],
    pub face_normals: &'a [V3],
    pub corner_uv: Option<&'a [[f32; 2]]>,
    pub styles: &'a [Option<FaceStyle>],
}

pub fn render(view: &View, geo: &Geometry, lines: &[Line], tex: &Mips, tiling: f32, selected: &[bool], active: Option<usize>, outline_px: usize) -> Frame {
    let (w, h) = (view.width.max(1), view.height.max(1));
    let tex_size = tex.levels[0].0 as f64;
    let max_lod = tex.levels.len() - 1;

    // Préparation des triangles (espace écran) et de l'ombrage par face
    let nf = geo.face_start.len() - 1;
    let vv: Vec<(f64, f64, f64)> = geo.positions.iter().map(|&p| to_view(view, p)).collect();
    let mut shade = vec![0f32; nf];
    let mut tris: Vec<Tri> = Vec::with_capacity(nf * 2);
    for f in 0..nf {
        let Some(st) = &geo.styles[f] else { continue };
        let r = geo.face_start[f]..geo.face_start[f + 1];
        if r.len() < 3 {
            continue;
        }
        // centre de la face vu depuis l'œil : ombrage « phare » comme le mode Solide de Blender
        let mut c = V3::ZERO;
        for ci in r.clone() {
            c = c + geo.positions[geo.corners[ci] as usize];
        }
        c = c / r.len() as f64;
        let to_eye = (view.eye - c).normalized();
        let ndv = geo.face_normals[f].dot(to_eye);
        let mut k = 0.30 + 0.70 * ndv.abs() as f32;
        if ndv < 0.0 {
            k *= 0.55; // faces arrière plus sombres
        }
        shade[f] = k;
        let poly: Vec<VV> = r
            .clone()
            .map(|ci| {
                let (x, y, z) = vv[geo.corners[ci] as usize];
                let uv = if st.textured { geo.corner_uv.map(|u| u[ci]).unwrap_or([0.0; 2]) } else { [0.0; 2] };
                VV { x, y, z, u: uv[0] * tiling, v: uv[1] * tiling }
            })
            .collect();
        let poly = if poly.iter().all(|p| p.z >= view.near) { poly } else { clip_near(&poly, view.near) };
        if poly.len() < 3 {
            continue;
        }
        let sv: Vec<SV> = poly.iter().map(|&p| project(view, p)).collect();
        // niveau de mipmap : texels par pixel sur la face
        let lod = if st.textured {
            let (mut a_uv, mut a_px) = (0.0f64, 0.0f64);
            for i in 1..poly.len() - 1 {
                let (p0, p1, p2) = (poly[0], poly[i], poly[i + 1]);
                a_uv += (((p1.u - p0.u) * (p2.v - p0.v) - (p2.u - p0.u) * (p1.v - p0.v)) as f64).abs() * 0.5;
                let (s0, s1, s2) = (sv[0], sv[i], sv[i + 1]);
                a_px += (((s1.x - s0.x) * (s2.y - s0.y) - (s2.x - s0.x) * (s1.y - s0.y)) as f64).abs() * 0.5;
            }
            let ratio = a_uv * tex_size * tex_size / a_px.max(1e-6);
            (0.5 * ratio.max(1.0).log2()).round().clamp(0.0, max_lod as f64) as u8
        } else {
            0
        };
        for i in 1..sv.len() - 1 {
            tris.push(Tri { v: [sv[0], sv[i], sv[i + 1]], face: f as u32, lod });
        }
    }

    let mut rgba = vec![0u8; w * h * 4];
    let mut depth = vec![0f32; w * h];
    let mut ids = vec![0u32; w * h];

    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16);
    let band = h.div_ceil(threads);
    std::thread::scope(|s| {
        for (((ti, rgba_b), depth_b), ids_b) in rgba.chunks_mut(band * w * 4).enumerate().zip(depth.chunks_mut(band * w)).zip(ids.chunks_mut(band * w)) {
            let y0 = ti * band;
            let y1 = (y0 + band).min(h);
            let tris = &tris;
            let shade = &shade;
            s.spawn(move || {
                raster_band(tris, geo, shade, tex, w, y0, y1, rgba_b, depth_b, ids_b);
                lines_band(view, lines, w, y0, y1, rgba_b, depth_b);
            });
        }
    });

    // Contour de sélection (sur les pixels de l'objet, là où l'identifiant change)
    if selected.iter().any(|&s| s) && outline_px > 0 {
        let ids_ref = &ids;
        let r = outline_px as isize;
        std::thread::scope(|s| {
            for (ti, rgba_b) in rgba.chunks_mut(band * w * 4).enumerate() {
                let y0 = ti * band;
                s.spawn(move || {
                    let rows = rgba_b.len() / (w * 4);
                    for yy in 0..rows {
                        let y = y0 + yy;
                        for x in 0..w {
                            let id = ids_ref[y * w + x];
                            if id == 0 || !selected.get(id as usize - 1).copied().unwrap_or(false) {
                                continue;
                            }
                            let mut edge = false;
                            'n: for dy in -r..=r {
                                for dx in -r..=r {
                                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                                        edge = true;
                                        break 'n;
                                    }
                                    if ids_ref[ny as usize * w + nx as usize] != id {
                                        edge = true;
                                        break 'n;
                                    }
                                }
                            }
                            if edge {
                                let col = if active == Some(id as usize - 1) { [0xff, 0xaa, 0x40, 255] } else { [0xf1, 0x58, 0x00, 255] };
                                rgba_b[(yy * w + x) * 4..(yy * w + x) * 4 + 4].copy_from_slice(&col);
                            }
                        }
                    }
                });
            }
        });
    }
    Frame { width: w, height: h, rgba, ids }
}

#[allow(clippy::too_many_arguments)]
fn raster_band(tris: &[Tri], geo: &Geometry, shade: &[f32], tex: &Mips, w: usize, y0: usize, y1: usize, rgba: &mut [u8], depth: &mut [f32], ids: &mut [u32]) {
    for t in tris {
        let [a, b, c] = t.v;
        let miny = a.y.min(b.y).min(c.y).floor().max(y0 as f32) as isize;
        let maxy = a.y.max(b.y).max(c.y).ceil().min(y1 as f32 - 1.0) as isize;
        if miny > maxy {
            continue;
        }
        let minx = a.x.min(b.x).min(c.x).floor().max(0.0) as isize;
        let maxx = a.x.max(b.x).max(c.x).ceil().min(w as f32 - 1.0) as isize;
        if minx > maxx {
            continue;
        }
        let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if area.abs() < 1e-8 {
            continue;
        }
        let inv = 1.0 / area;
        let st = geo.styles[t.face as usize].as_ref().unwrap();
        let k = shade[t.face as usize];
        let flat = [(st.color[0] as f32 * k) as u8, (st.color[1] as f32 * k) as u8, (st.color[2] as f32 * k) as u8];
        for y in miny..=maxy {
            let py = y as f32 + 0.5;
            let row = (y as usize - y0) * w;
            for x in minx..=maxx {
                let px = x as f32 + 0.5;
                let w0 = ((b.x - px) * (c.y - py) - (c.x - px) * (b.y - py)) * inv;
                let w1 = ((c.x - px) * (a.y - py) - (a.x - px) * (c.y - py)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6 {
                    continue;
                }
                let iz = w0 * a.iz + w1 * b.iz + w2 * c.iz;
                let i = row + x as usize;
                if iz <= depth[i] {
                    continue;
                }
                depth[i] = iz;
                ids[i] = st.id;
                let col = if st.textured {
                    let u = (w0 * a.u + w1 * b.u + w2 * c.u) / iz;
                    let v = (w0 * a.v + w1 * b.v + w2 * c.v) / iz;
                    let s = tex.sample(t.lod as usize, u, v);
                    [(s[0] as f32 * k) as u8, (s[1] as f32 * k) as u8, (s[2] as f32 * k) as u8]
                } else {
                    flat
                };
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[col[0], col[1], col[2], 255]);
            }
        }
    }
}

fn lines_band(view: &View, lines: &[Line], w: usize, y0: usize, y1: usize, rgba: &mut [u8], depth: &[f32]) {
    for l in lines {
        let (ax, ay, az) = to_view(view, l.a);
        let (bx, by, bz) = to_view(view, l.b);
        let mut pa = VV { x: ax, y: ay, z: az, u: 0.0, v: 0.0 };
        let mut pb = VV { x: bx, y: by, z: bz, u: 0.0, v: 0.0 };
        if pa.z < view.near && pb.z < view.near {
            continue;
        }
        if pa.z < view.near || pb.z < view.near {
            let t = (view.near - pa.z) / (pb.z - pa.z);
            let cut = VV { x: pa.x + (pb.x - pa.x) * t, y: pa.y + (pb.y - pa.y) * t, z: view.near, u: 0.0, v: 0.0 };
            if pa.z < view.near {
                pa = cut;
            } else {
                pb = cut;
            }
        }
        let (sa, sb) = (project(view, pa), project(view, pb));
        if (sa.y < y0 as f32 - 2.0 && sb.y < y0 as f32 - 2.0) || (sa.y > y1 as f32 + 2.0 && sb.y > y1 as f32 + 2.0) {
            continue;
        }
        let (dx, dy) = (sb.x - sa.x, sb.y - sa.y);
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as usize;
        if steps > 20_000 {
            continue;
        }
        // épaisseur perpendiculaire à l'axe dominant
        let horiz = dx.abs() >= dy.abs();
        let half = l.width as isize / 2;
        let alpha = l.color[3] as f32 / 255.0;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let x = sa.x + dx * t;
            let y = sa.y + dy * t;
            let iz = sa.iz + (sb.iz - sa.iz) * t;
            for o in -half..=(l.width as isize - 1 - half) {
                let (xi, yi) = if horiz { (x as isize, y as isize + o) } else { (x as isize + o, y as isize) };
                if xi < 0 || xi >= w as isize || yi < y0 as isize || yi >= y1 as isize {
                    continue;
                }
                let i = (yi as usize - y0) * w + xi as usize;
                // biais de profondeur : l'arête reste visible sur sa propre face
                if iz * 1.004 + 1e-7 < depth[i] {
                    continue;
                }
                let p = &mut rgba[i * 4..i * 4 + 4];
                if p[3] == 0 {
                    p.copy_from_slice(&[l.color[0], l.color[1], l.color[2], l.color[3]]);
                } else {
                    for k in 0..3 {
                        p[k] = (p[k] as f32 * (1.0 - alpha) + l.color[k] as f32 * alpha) as u8;
                    }
                }
            }
        }
    }
}
