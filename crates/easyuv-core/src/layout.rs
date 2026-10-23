//! Mode rangement (layout) : retrouve les îlots d'UV existantes et les range dans la tuile
//! sans redéplier, comme le « Pack Islands » de Blender ou le Layout de RizomUV.

use crate::math::V2;
use crate::mesh::{Mesh, Topology};
use crate::{place_islands, shapepack, Params, Stats, UvResult};
use std::collections::HashMap;
use std::time::Instant;

fn find(p: &mut [u32], mut i: u32) -> u32 {
    while p[i as usize] != i {
        p[i as usize] = p[p[i as usize] as usize];
        i = p[i as usize];
    }
    i
}

/// Îlots UV : deux faces voisines sont dans le même îlot si les UV de l'arête partagée
/// coïncident des deux côtés. Retourne (îlot de chaque face, nombre d'îlots), numérotés
/// dans l'ordre de leur première face.
pub fn uv_islands(mesh: &Mesh, corner_uv: &[[f32; 2]]) -> (Vec<u32>, usize) {
    let nf = mesh.face_count();
    let mut parent: Vec<u32> = (0..nf as u32).collect();
    // arête -> (face, uv du plus petit sommet, uv du plus grand)
    let mut edges: HashMap<(u32, u32), (u32, [f32; 2], [f32; 2])> = HashMap::with_capacity(mesh.corners.len());
    let same = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() <= 1e-6 && (a[1] - b[1]).abs() <= 1e-6;
    for f in 0..nf {
        let r = mesh.face_range(f);
        let n = r.len();
        for i in 0..n {
            let (ca, cb) = (r.start + i, r.start + (i + 1) % n);
            let (a, b) = (mesh.corners[ca], mesh.corners[cb]);
            if a == b {
                continue;
            }
            let (key, ua, ub) = if a < b { ((a, b), corner_uv[ca], corner_uv[cb]) } else { ((b, a), corner_uv[cb], corner_uv[ca]) };
            match edges.get(&key) {
                Some(&(g, va, vb)) => {
                    if same(ua, va) && same(ub, vb) {
                        let (x, y) = (find(&mut parent, f as u32), find(&mut parent, g));
                        if x != y {
                            parent[x.max(y) as usize] = x.min(y);
                        }
                    }
                }
                None => {
                    edges.insert(key, (f as u32, ua, ub));
                }
            }
        }
    }
    let mut id = vec![u32::MAX; nf];
    let mut out = vec![0u32; nf];
    let mut count = 0u32;
    for f in 0..nf {
        let root = find(&mut parent, f as u32) as usize;
        if id[root] == u32::MAX {
            id[root] = count;
            count += 1;
        }
        out[f] = id[root];
    }
    (out, count as usize)
}

/// Résultat d'un rangement.
#[derive(Clone, Debug, Default)]
pub struct LayoutReport {
    /// Îlots rangés.
    pub islands: usize,
    /// Îlots ignorés car sans surface UV (objets jamais dépliés).
    pub skipped: usize,
    /// Surface UV couverte par les îlots rangés (fraction de tuile).
    pub coverage: f64,
    pub overflow: bool,
}

/// Range les îlots `which[island] == true` (tous si `None`) dans la tuile 0-1, en place.
/// `equalize` : remet chaque îlot à la même densité de texels (sinon les tailles relatives
/// sont conservées). Les autres îlots ne bougent pas : un sous-ensemble est rangé dans la
/// place qu'ils laissent libre, à leur densité (ou à sa taille actuelle sans égalisation),
/// réduit seulement s'il ne tient pas.
pub fn layout_islands(mesh: &Mesh, corner_uv: &mut [[f32; 2]], face_island: &[u32], which: Option<&[bool]>, params: &Params, equalize: bool) -> LayoutReport {
    let n_isl = face_island.iter().map(|&i| i as usize + 1).max().unwrap_or(0);
    let take = |i: u32| which.is_none_or(|w| w.get(i as usize).copied().unwrap_or(false));
    let subset = which.is_some_and(|w| (0..n_isl).any(|i| !w.get(i).copied().unwrap_or(false)));
    // sommets locaux de chaque îlot : (sommet du maillage, UV) -> indice local
    let mut keys: Vec<HashMap<(u32, u32, u32), u32>> = vec![HashMap::new(); n_isl];
    // îlots fixes (obstacles) : triangles UV absolus, et leur densité
    let mut fixed_uv: Vec<V2> = Vec::new();
    let mut fixed_tris: Vec<[u32; 3]> = Vec::new();
    let (mut fixed3, mut fixedu) = (0.0, 0.0);
    let mut uvs: Vec<Vec<V2>> = vec![Vec::new(); n_isl];
    let mut tris: Vec<Vec<[u32; 3]>> = vec![Vec::new(); n_isl];
    let mut corner_local: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n_isl];
    let mut area3: Vec<f64> = vec![0.0; n_isl];
    for f in 0..mesh.face_count() {
        let isl = face_island[f];
        if !take(isl) {
            if subset {
                let r = mesh.face_range(f);
                let base = fixed_uv.len() as u32;
                fixed_uv.extend(r.clone().map(|c| V2::new(corner_uv[c][0] as f64, corner_uv[c][1] as f64)));
                for t in mesh.fan(f) {
                    let tri = [base + t[0] as u32, base + t[1] as u32, base + t[2] as u32];
                    let [a, b, c] = tri.map(|k| fixed_uv[k as usize]);
                    fixedu += 0.5 * (b - a).cross(c - a).abs();
                    fixed_tris.push(tri);
                }
                fixed3 += mesh.face_area(f);
            }
            continue;
        }
        let i = isl as usize;
        let r = mesh.face_range(f);
        let local: Vec<u32> = r
            .clone()
            .map(|c| {
                let u = corner_uv[c];
                let k = (mesh.corners[c], u[0].to_bits(), u[1].to_bits());
                let next = uvs[i].len() as u32;
                let l = *keys[i].entry(k).or_insert(next);
                if l == next {
                    uvs[i].push(V2::new(u[0] as f64, u[1] as f64));
                }
                corner_local[i].push((c as u32, l));
                l
            })
            .collect();
        for t in mesh.fan(f) {
            tris[i].push([local[t[0]], local[t[1]], local[t[2]]]);
        }
        area3[i] += mesh.face_area(f);
    }
    drop(keys);
    let uv_area = |uv: &[V2], tris: &[[u32; 3]]| -> f64 {
        tris.iter().map(|t| 0.5 * (uv[t[1] as usize] - uv[t[0] as usize]).cross(uv[t[2] as usize] - uv[t[0] as usize]).abs()).sum()
    };
    let mut report = LayoutReport::default();
    let mut ids: Vec<usize> = Vec::new();
    let (mut sum3, mut sumu) = (0.0, 0.0);
    for i in 0..n_isl {
        if tris[i].is_empty() {
            continue;
        }
        let au = uv_area(&uvs[i], &tris[i]);
        if au <= 1e-14 || area3[i] <= 0.0 {
            report.skipped += 1;
            continue;
        }
        if equalize {
            // îlot en unités monde : 1 unité UV = 1 m
            let k = (area3[i] / au).sqrt();
            uvs[i].iter_mut().for_each(|p| *p = *p * k);
        }
        sum3 += area3[i];
        sumu += au;
        ids.push(i);
    }
    if ids.is_empty() {
        return report;
    }
    if !equalize && !subset {
        // tailles relatives conservées, ramenées en unités monde en moyenne (densité cible)
        let k = (sum3 / sumu).sqrt();
        for &i in &ids {
            uvs[i].iter_mut().for_each(|p| *p = *p * k);
        }
    }
    let mut charts: Vec<Vec<V2>> = ids.iter().map(|&i| std::mem::take(&mut uvs[i])).collect();
    let tri_refs: Vec<&[[u32; 3]]> = ids.iter().map(|&i| tris[i].as_slice()).collect();
    let mut free = None;
    if subset {
        // échelle visée : la densité des îlots fixes, ou la taille actuelle
        let scale = if !equalize {
            1.0
        } else if fixedu > 0.0 && fixed3 > 0.0 {
            (fixedu / fixed3).sqrt()
        } else {
            (sumu / sum3).sqrt()
        };
        let rects: Vec<(f64, f64)> = charts.iter_mut().map(|uv| if params.rotate_islands { crate::pack::orient_min_box(uv) } else { crate::pack::to_origin(uv) }).collect();
        drop(rects);
        let islands: Vec<shapepack::Island> = charts.iter().zip(&tri_refs).map(|(uv, t)| shapepack::Island { uv, tris: t }).collect();
        let obstacles = [shapepack::Island { uv: &fixed_uv, tris: &fixed_tris }];
        if let Some(sp) = shapepack::pack_into_free(&islands, &obstacles, params.margin, params.rotate_islands, 512, scale) {
            free = Some(charts.iter().zip(&sp.xforms).map(|(uv, xf)| uv.iter().map(|&p| xf.apply(p, sp.scale)).collect::<Vec<V2>>()).collect::<Vec<_>>());
        }
    }
    let (placed, overflow) = match free {
        Some(p) => (p, false),
        None => {
            let (p, _, o) = place_islands(&mut charts, &tri_refs, params, params.rotate_islands);
            (p, o)
        }
    };
    for (k, &i) in ids.iter().enumerate() {
        for &(c, l) in &corner_local[i] {
            let p = placed[k][l as usize];
            corner_uv[c as usize] = [p.x as f32, p.y as f32];
        }
        report.coverage += uv_area(&placed[k], &tris[i]);
    }
    report.islands = ids.len();
    report.overflow = overflow;
    report
}

/// Rend droits les îlots `which[island]` (bandes courbes, grilles) : la plus grande bande de
/// quads en grille de l'îlot devient une grille de rectangles, colonnes et rangées gardant la
/// longueur moyenne de leurs arêtes en 3D (comme « Follow Active Quads » de Blender ou
/// « Straighten » de RizomUV). Le reste de l'îlot (couvercle, n-gones, triangles) est
/// recalculé par LSCM en s'appuyant sur la bande droite. L'îlot garde sa surface UV et son
/// centre, côté long horizontal. Retourne (îlots redressés, îlots ignorés : pas de quads).
pub fn straighten_grid(mesh: &Mesh, corner_uv: &mut [[f32; 2]], face_island: &[u32], which: &[bool]) -> (usize, usize) {
    let n_isl = which.len();
    let mut faces: Vec<Vec<usize>> = vec![Vec::new(); n_isl];
    for f in 0..mesh.face_count() {
        let i = face_island[f] as usize;
        if i < n_isl && which[i] {
            faces[i].push(f);
        }
    }
    let (mut done, mut skipped) = (0, 0);
    for fs in faces.iter().filter(|fs| !fs.is_empty()) {
        match straighten_one(mesh, corner_uv, fs) {
            Ok(()) => done += 1,
            Err(why) => {
                if std::env::var_os("EASYUV_PROFILE").is_some() {
                    eprintln!("rendre droit : îlot de {} faces ignoré ({why})", fs.len());
                }
                skipped += 1
            }
        }
    }
    (done, skipped)
}

fn straighten_one(mesh: &Mesh, corner_uv: &mut [[f32; 2]], fs: &[usize]) -> Result<(), &'static str> {
    // sommets locaux : (sommet, UV) -> id
    let key = |c: usize| (mesh.corners[c], corner_uv[c][0].to_bits(), corner_uv[c][1].to_bits());
    let mut ids: HashMap<(u32, u32, u32), usize> = HashMap::new();
    let mut fv: Vec<Vec<usize>> = Vec::with_capacity(fs.len());
    for &f in fs {
        let q: Vec<usize> = mesh
            .face_range(f)
            .map(|c| {
                let n = ids.len();
                *ids.entry(key(c)).or_insert(n)
            })
            .collect();
        fv.push(q);
    }
    let nv = ids.len();
    // (coin, sommet local) : calculé avant toute écriture dans corner_uv
    let corner_ids: Vec<(usize, usize)> = fs.iter().flat_map(|&f| mesh.face_range(f).map(|c| (c, ids[&key(c)]))).collect();
    drop(ids);
    let mut pos3 = vec![crate::math::V3::new(0.0, 0.0, 0.0); nv];
    let mut orig = vec![V2::new(0.0, 0.0); nv];
    for &(c, v) in &corner_ids {
        pos3[v] = mesh.positions[mesh.corners[c] as usize];
        orig[v] = V2::new(corner_uv[c][0] as f64, corner_uv[c][1] as f64);
    }
    let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (fi, q) in fv.iter().enumerate() {
        for k in 0..q.len() {
            let (a, b) = (q[k], q[(k + 1) % q.len()]);
            edge_faces.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }

    // candidats : grille de quads (exacte quand elle couvre l'îlot) et déroulé polaire
    // (arc de cône, quelle que soit la topologie) ; on garde le moins abîmé
    let before = damaged(&fv, &pos3, &orig);
    let mut cands: Vec<(usize, u8, Vec<V2>)> = Vec::new();
    let mut why = "pas une bande";
    match grid_straight(&fv, &pos3, &edge_faces) {
        Ok((uv, full)) => {
            if let Some(uv) = finalize(&fv, &orig, uv) {
                cands.push((damaged(&fv, &pos3, &uv), if full { 0 } else { 2 }, uv));
            }
        }
        Err(e) => why = e,
    }
    if let Some(uv) = polar_straight(&fv, &orig).and_then(|uv| finalize(&fv, &orig, uv)) {
        cands.push((damaged(&fv, &pos3, &uv), 1, uv));
    }
    if std::env::var_os("EASYUV_PROFILE").is_some() {
        eprintln!("rendre droit : {} faces, abîmées avant {before}, candidats (genre, abîmées) {:?}", fv.len(), cands.iter().map(|c| (c.1, c.0)).collect::<Vec<_>>());
    }
    let Some((dmg, _, uv)) = cands.into_iter().min_by_key(|c| (c.0, c.1)) else { return Err(why) };
    // aucun résultat propre : l'îlot reste tel quel plutôt que d'être abîmé. Petite marge :
    // le triangle d'une jonction en T devient forcément un trait sur une grille droite
    if dmg > before + (fv.len() / 50).max(1) {
        return Err("le redressement abîmerait des faces");
    }
    for &(c, v) in &corner_ids {
        corner_uv[c] = [uv[v].x as f32, uv[v].y as f32];
    }
    Ok(())
}

type EdgeFaces = HashMap<(usize, usize), Vec<usize>>;

/// Plus grande grille de quads de l'îlot, dépliée droite ; le reste suit par LSCM. Renvoie
/// les UV (avant mise à l'échelle) et vrai si la grille couvre tout l'îlot.
fn grid_straight(fv: &[Vec<usize>], pos3: &[crate::math::V3], edge_faces: &EdgeFaces) -> Result<(Vec<V2>, bool), &'static str> {
    let nv = pos3.len();
    // --- plus grande grille de quads cohérente (plusieurs départs, la plus grande gagne) ---
    let grow = |seed: usize| -> (Vec<Option<(i64, i64)>>, Vec<bool>) {
        let mut grid: Vec<Option<(i64, i64)>> = vec![None; nv];
        let mut inside = vec![false; fv.len()];
        let mut cells = std::collections::HashSet::new();
        for (k, g) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
            grid[fv[seed][k]] = Some(g);
        }
        inside[seed] = true;
        cells.insert((0i64, 0i64));
        let mut queue = std::collections::VecDeque::from([seed]);
        while let Some(fi) = queue.pop_front() {
            let q = &fv[fi];
            for k in 0..4 {
                let (a, b) = (q[k], q[(k + 1) % 4]);
                let (pa, pb, ia, ib) = (grid[a].unwrap(), grid[b].unwrap(), grid[q[(k + 3) % 4]].unwrap(), grid[q[(k + 2) % 4]].unwrap());
                let out = (pa.0 - ia.0, pa.1 - ia.1);
                if out != (pb.0 - ib.0, pb.1 - ib.1) || out.0.abs() + out.1.abs() != 1 {
                    continue;
                }
                for &g in &edge_faces[&(a.min(b), a.max(b))] {
                    if inside[g] || fv[g].len() != 4 {
                        continue;
                    }
                    let w = &fv[g];
                    // dans g, l'arête est parcourue b -> a ; les deux autres sommets suivent
                    let Some(j) = (0..4).find(|&j| w[j] == b && w[(j + 1) % 4] == a) else { continue };
                    let (na, nb) = (w[(j + 2) % 4], w[(j + 3) % 4]);
                    let (ta, tb) = ((pa.0 + out.0, pa.1 + out.1), (pb.0 + out.0, pb.1 + out.1));
                    let cell = (pa.0.min(pb.0).min(ta.0), pa.1.min(pb.1).min(ta.1));
                    // conflit : sommet déjà placé ailleurs, ou cellule déjà prise (anneau fermé)
                    if grid[na].is_some_and(|p| p != ta) || grid[nb].is_some_and(|p| p != tb) || cells.contains(&cell) {
                        continue;
                    }
                    grid[na] = Some(ta);
                    grid[nb] = Some(tb);
                    cells.insert(cell);
                    inside[g] = true;
                    queue.push_back(g);
                }
            }
        }
        (grid, inside)
    };
    let quads: Vec<usize> = (0..fv.len()).filter(|&f| fv[f].len() == 4).collect();
    if quads.is_empty() {
        return Err("aucun quad");
    }
    let mut best: Option<(Vec<Option<(i64, i64)>>, Vec<bool>, usize)> = None;
    for s in 0..6 {
        let seed = quads[s * quads.len() / 6];
        let (g, ins) = grow(seed);
        let n = ins.iter().filter(|&&x| x).count();
        if best.as_ref().is_none_or(|b| n > b.2) {
            best = Some((g, ins, n));
        }
        if n == quads.len() {
            break;
        }
    }
    let (grid, mut inside, _) = best.unwrap();

    // --- on ne garde que les rangées / colonnes de largeur proche de la bande principale ---
    // (un couvercle en anneau collé à la bande a des rangées bien plus courtes : il n'est pas
    // déroulé, il suit ensuite la bande par LSCM)
    let cell_of = |q: &[usize]| {
        let g: Vec<(i64, i64)> = q.iter().map(|&v| grid[v].unwrap()).collect();
        (g.iter().map(|p| p.0).min().unwrap(), g.iter().map(|p| p.1).min().unwrap())
    };
    for pass in 0..2 {
        // pass 0 : rangées (largeur des cellules), pass 1 : colonnes (hauteur)
        let mut acc: HashMap<i64, (f64, usize)> = HashMap::new();
        for (fi, q) in fv.iter().enumerate() {
            if !inside[fi] {
                continue;
            }
            let c = cell_of(q);
            let line = if pass == 0 { c.1 } else { c.0 };
            for k in 0..4 {
                let (a, b) = (q[k], q[(k + 1) % 4]);
                let (pa, pb) = (grid[a].unwrap(), grid[b].unwrap());
                let along = if pass == 0 { pa.1 == pb.1 } else { pa.0 == pb.0 };
                if along {
                    let e = acc.entry(line).or_insert((0.0, 0));
                    e.0 += (pos3[a] - pos3[b]).length();
                    e.1 += 1;
                }
            }
        }
        let mut lines: Vec<(i64, f64, usize)> = acc.into_iter().map(|(l, (s, n))| (l, s / n as f64, n)).collect();
        lines.sort_by_key(|l| l.0);
        // ligne de référence : la plus peuplée
        let Some(ref_i) = (0..lines.len()).max_by_key(|&i| (lines[i].2, std::cmp::Reverse(i))) else { continue };
        let m = lines[ref_i].1;
        let ok = |i: usize| lines[i].1 >= m / 1.6 && lines[i].1 <= m * 1.6;
        let (mut lo, mut hi) = (ref_i, ref_i);
        while lo > 0 && ok(lo - 1) && lines[lo - 1].0 == lines[lo].0 - 1 {
            lo -= 1;
        }
        while hi + 1 < lines.len() && ok(hi + 1) && lines[hi + 1].0 == lines[hi].0 + 1 {
            hi += 1;
        }
        let (l0, l1) = (lines[lo].0, lines[hi].0);
        for (fi, q) in fv.iter().enumerate() {
            if inside[fi] {
                let c = cell_of(q);
                let line = if pass == 0 { c.1 } else { c.0 };
                inside[fi] = line >= l0 && line <= l1;
            }
        }
    }
    let n_in = inside.iter().filter(|&&x| x).count();
    if n_in < 2 {
        return Err("pas de bande de quads");
    }

    // --- grille droite : colonnes et rangées à la longueur moyenne de leurs arêtes 3D ---
    let mut in_v = vec![false; nv];
    for (fi, q) in fv.iter().enumerate() {
        if inside[fi] {
            q.iter().for_each(|&v| in_v[v] = true);
        }
    }
    let g: Vec<(i64, i64)> = (0..nv).map(|v| if in_v[v] { grid[v].unwrap() } else { (0, 0) }).collect();
    let used: Vec<(i64, i64)> = (0..nv).filter(|&v| in_v[v]).map(|v| g[v]).collect();
    let (xmin, ymin) = (used.iter().map(|p| p.0).min().unwrap(), used.iter().map(|p| p.1).min().unwrap());
    let (nx, ny) = ((used.iter().map(|p| p.0).max().unwrap() - xmin) as usize, (used.iter().map(|p| p.1).max().unwrap() - ymin) as usize);
    let (mut wsum, mut wcnt) = (vec![0.0; nx], vec![0usize; nx]);
    let (mut hsum, mut hcnt) = (vec![0.0; ny], vec![0usize; ny]);
    for (fi, q) in fv.iter().enumerate() {
        if !inside[fi] {
            continue;
        }
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let (pa, pb) = (g[a], g[b]);
            let len = (pos3[a] - pos3[b]).length();
            if pa.1 == pb.1 {
                let c = (pa.0.min(pb.0) - xmin) as usize;
                wsum[c] += len;
                wcnt[c] += 1;
            } else {
                let r = (pa.1.min(pb.1) - ymin) as usize;
                hsum[r] += len;
                hcnt[r] += 1;
            }
        }
    }
    let cum = |sum: &[f64], cnt: &[usize]| {
        let mean = sum.iter().sum::<f64>() / cnt.iter().sum::<usize>().max(1) as f64;
        let mut out = vec![0.0];
        for k in 0..sum.len() {
            out.push(out[k] + if cnt[k] > 0 { sum[k] / cnt[k] as f64 } else { mean });
        }
        out
    };
    let (xs, ys) = (cum(&wsum, &wcnt), cum(&hsum, &hcnt));
    let mut uv: Vec<V2> = (0..nv).map(|v| if in_v[v] { V2::new(xs[(g[v].0 - xmin) as usize], ys[(g[v].1 - ymin) as usize]) } else { V2::new(0.0, 0.0) }).collect();
    // --- le reste de l'îlot (couvercles, n-gones…) suit la bande : LSCM épinglé ---
    if in_v.iter().any(|&x| !x) {
        let mut tris: Vec<[u32; 3]> = Vec::new();
        for q in fv {
            for k in 1..q.len() - 1 {
                tris.push([q[0] as u32, q[k] as u32, q[k + 1] as u32]);
            }
        }
        let mut fixed = vec![None; 2 * nv];
        for v in 0..nv {
            if in_v[v] {
                fixed[v] = Some(uv[v].x);
                fixed[nv + v] = Some(uv[v].y);
            }
        }
        let solved = crate::unwrap::lscm_pinned(&pos3, &tris, &fixed, &uv, 3000);
        if solved.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err("résolution impossible");
        }
        uv = solved;
    }

    Ok((uv, n_in == fv.len()))
}

/// Déroulé polaire : une bande courbe dépliée (arc de cône) est un secteur d'anneau. On
/// ajuste son cercle ; l'angle devient l'abscisse, le rayon l'ordonnée. Marche avec des
/// triangles, des n-gones et des jonctions en T, là où la grille de quads s'arrête.
fn polar_straight(fv: &[Vec<usize>], orig: &[V2]) -> Option<Vec<V2>> {
    let n = orig.len();
    if n < 4 {
        return None;
    }
    let mean = orig.iter().fold(V2::new(0.0, 0.0), |s, p| s + *p) * (1.0 / n as f64);
    let (lo, hi) = bounds2(orig);
    let ext = (hi - lo).x.hypot((hi - lo).y).max(1e-12);
    // cercle algébrique (Kåsa) : x² + y² + D x + E y + F = 0, en coordonnées centrées réduites
    let mut m = [[0.0f64; 3]; 3];
    let mut r = [0.0f64; 3];
    for p in orig {
        let (x, y) = ((p.x - mean.x) / ext, (p.y - mean.y) / ext);
        let row = [x, y, 1.0];
        let z = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
            r[i] += row[i] * z;
        }
    }
    let sol = solve3(m, r)?;
    let c = V2::new(-sol[0] / 2.0, -sol[1] / 2.0);
    let rad2 = c.x * c.x + c.y * c.y - sol[2];
    // presque droite (rayon immense) : rien à dérouler
    if !rad2.is_finite() || rad2 <= 0.0 || rad2.sqrt() > 20.0 {
        return None;
    }
    let mut c = mean + c * ext;
    // affinage géométrique (Gauss-Newton sur |p - c| - R) : l'ajustement algébrique est
    // biaisé quand les points remplissent un anneau au lieu d'un cercle
    for _ in 0..30 {
        let d: Vec<(V2, f64)> = orig.iter().map(|p| (*p - c, (*p - c).x.hypot((*p - c).y).max(1e-300))).collect();
        let rm = d.iter().map(|x| x.1).sum::<f64>() / n as f64;
        // gradient de Σ (|p - c| - rm)² par rapport à c
        let (mut gx, mut gy, mut jj) = (0.0, 0.0, [[0.0f64; 2]; 2]);
        for (v, l) in &d {
            let (ux, uy) = (-v.x / l, -v.y / l);
            let e = l - rm;
            gx += ux * e;
            gy += uy * e;
            jj[0][0] += ux * ux;
            jj[0][1] += ux * uy;
            jj[1][1] += uy * uy;
        }
        let det = jj[0][0] * jj[1][1] - jj[0][1] * jj[0][1];
        if det.abs() < 1e-18 {
            break;
        }
        let step = V2::new((jj[1][1] * gx - jj[0][1] * gy) / det, (jj[0][0] * gy - jj[0][1] * gx) / det);
        c = c - step;
        if step.x.hypot(step.y) < 1e-9 * ext {
            break;
        }
    }
    if !c.x.is_finite() || (c - mean).x.hypot((c - mean).y) > 20.0 * ext {
        return None;
    }
    let dir = (mean - c).y.atan2((mean - c).x);
    let polar: Vec<(f64, f64)> = orig
        .iter()
        .map(|p| {
            let d = *p - c;
            let mut t = d.y.atan2(d.x) - dir;
            while t > std::f64::consts::PI {
                t -= std::f64::consts::TAU;
            }
            while t < -std::f64::consts::PI {
                t += std::f64::consts::TAU;
            }
            (t, d.x.hypot(d.y))
        })
        .collect();
    let (tmin, tmax) = polar.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (rmin, rmax) = polar.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    // anneau presque fermé ou disque (couvercle) : pas une bande à dérouler
    if tmax - tmin > 1.75 * std::f64::consts::PI || rmin < 0.05 * rmax {
        return None;
    }
    // une face à cheval sur la coupure de l'angle : l'îlot fait le tour du centre
    if fv.iter().any(|q| q.iter().any(|&a| q.iter().any(|&b| (polar[a].0 - polar[b].0).abs() > std::f64::consts::PI))) {
        return None;
    }
    let r_ref = polar.iter().map(|p| p.1).sum::<f64>() / n as f64;
    Some(polar.iter().map(|&(t, rr)| V2::new(t * r_ref, rr)).collect())
}

/// Système 3x3 par la règle de Cramer.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    let d = det(m);
    if d.abs() < 1e-18 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut a = m;
        for (row, &rv) in a.iter_mut().zip(&r) {
            row[k] = rv;
        }
        *o = det(a) / d;
    }
    Some(out)
}

fn bounds2(pts: &[V2]) -> (V2, V2) {
    pts.iter().fold((V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN)), |(lo, hi), p| (V2::new(lo.x.min(p.x), lo.y.min(p.y)), V2::new(hi.x.max(p.x), hi.y.max(p.y))))
}

/// Aire UV signée d'une face (sommets locaux).
fn face_area(q: &[usize], pts: &[V2]) -> f64 {
    (1..q.len() - 1).map(|k| 0.5 * (pts[q[k]] - pts[q[0]]).cross(pts[q[k + 1]] - pts[q[0]])).sum()
}

/// Même sens que l'îlot d'origine (pas de miroir), côté long horizontal, surface et centre
/// de l'îlot d'origine.
fn finalize(fv: &[Vec<usize>], orig: &[V2], mut uv: Vec<V2>) -> Option<Vec<V2>> {
    if uv.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return None;
    }
    let signed = |pts: &[V2]| -> f64 { fv.iter().map(|q| face_area(q, pts)).sum() };
    if signed(orig) * signed(&uv) < 0.0 {
        uv.iter_mut().for_each(|p| p.x = -p.x);
    }
    let (lo, hi) = bounds2(&uv);
    if hi.y - lo.y > hi.x - lo.x {
        uv.iter_mut().for_each(|p| *p = V2::new(-p.y, p.x));
    }
    let area = |pts: &[V2]| -> f64 { fv.iter().map(|q| face_area(q, pts).abs()).sum() };
    let (ao, an) = (area(orig), area(&uv));
    if an <= 0.0 || ao <= 0.0 {
        return None;
    }
    let k = (ao / an).sqrt();
    let (olo, ohi) = bounds2(orig);
    uv.iter_mut().for_each(|p| *p = *p * k);
    let (nlo, nhi) = bounds2(&uv);
    let shift = (olo + ohi) * 0.5 - (nlo + nhi) * 0.5;
    uv.iter_mut().for_each(|p| *p = *p + shift);
    Some(uv)
}

/// Faces abîmées : retournées, ou écrasées (aire UV rapportée à l'aire 3D très sous la
/// médiane de l'îlot). Les faces presque nulles en 3D (fines jonctions en T) ne comptent pas.
fn damaged(fv: &[Vec<usize>], pos3: &[crate::math::V3], uv: &[V2]) -> usize {
    let a3: Vec<f64> = fv.iter().map(|q| (1..q.len() - 1).map(|k| 0.5 * (pos3[q[k]] - pos3[q[0]]).cross(pos3[q[k + 1]] - pos3[q[0]]).length()).sum()).collect();
    let mut sorted = a3.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med3 = sorted[sorted.len() / 2];
    let sign = fv.iter().map(|q| face_area(q, uv)).sum::<f64>().signum();
    let ratio: Vec<f64> = fv.iter().zip(&a3).map(|(q, &a)| face_area(q, uv) * sign / a.max(1e-30)).collect();
    let mut rs: Vec<f64> = ratio.iter().zip(&a3).filter(|(_, &a)| a > 0.05 * med3).map(|(r, _)| *r).collect();
    if rs.is_empty() {
        return 0;
    }
    rs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = rs[rs.len() / 2];
    ratio.iter().zip(&a3).filter(|(r, &a)| a > 0.05 * med3 && **r < 0.15 * med).count()
}

/// Coutures des UV : arêtes dont les deux côtés sont dans des îlots différents ou n'ont pas
/// les mêmes UV. Renvoie les arêtes (paires de sommets) et leur longueur totale rapportée à
/// la racine de l'aire du maillage.
pub fn uv_seams(mesh: &Mesh, corner_uv: &[[f32; 2]], face_chart: &[u32]) -> (Vec<(u32, u32)>, f64) {
    let topo = Topology::build(mesh);
    let mut seams = Vec::new();
    let mut length = 0.0;
    for e in &topo.edges {
        let is_seam = e.faces.len() > 2
            || (e.faces.len() == 2
                && (face_chart[e.faces[0].face as usize] != face_chart[e.faces[1].face as usize]
                    || corner_uv[e.faces[0].ca as usize] != corner_uv[e.faces[1].ca as usize]
                    || corner_uv[e.faces[0].cb as usize] != corner_uv[e.faces[1].cb as usize]));
        if is_seam {
            seams.push(e.key);
            length += e.length;
        }
    }
    let total: f64 = topo.face_areas.iter().sum();
    if total > 0.0 {
        length /= total.sqrt();
    }
    (seams, length)
}

/// Range toutes les UV existantes d'un maillage (sans redéplier) et calcule les
/// statistiques et coutures comme après un dépliage.
pub fn pack_existing(mesh: &Mesh, corner_uv: &[[f32; 2]], params: &Params, equalize: bool) -> UvResult {
    let t0 = Instant::now();
    let mut uv = corner_uv.to_vec();
    let (face_chart, n) = uv_islands(mesh, &uv);
    let rep = layout_islands(mesh, &mut uv, &face_chart, None, params, equalize);
    let mut stats = Stats { charts: n, coverage: rep.coverage, overflow: rep.overflow, ..Default::default() };
    let (seams, seam_length) = uv_seams(mesh, &uv, &face_chart);
    stats.seam_length = seam_length;
    stats.seams = seams.len();
    stats.density_px_per_unit = crate::measure_density(mesh, &uv, params.texture_size);
    stats.time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    UvResult { corner_uv: uv, face_chart, seams, stats }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{metrics, primitives, smart_unwrap};

    #[test]
    fn islands_match_unwrap() {
        let m = primitives::cube(2.0);
        let r = smart_unwrap(&m, &Params::default(), &[]);
        let (_, n) = uv_islands(&m, &r.corner_uv);
        assert_eq!(n, 6);
    }

    #[test]
    fn repack_scrambled_layout() {
        let mut model = crate::model::Model::single("Tore", primitives::torus(1.0, 0.35, 32, 12));
        model.push("Cube", &primitives::cube(1.0));
        let p = Params::default();
        // deux dépliages séparés qui se superposent (chaque objet remplit sa tuile)
        let sep = crate::model::unwrap_model(&model, &p, false);
        assert!(metrics::overlap_ratio(&model.mesh, &sep.corner_uv, 256) > 0.05);
        let r = pack_existing(&model.mesh, &sep.corner_uv, &p, true);
        assert!(metrics::overlap_ratio(&model.mesh, &r.corner_uv, 256) < 0.002, "chevauchement après rangement");
        assert!(r.corner_uv.iter().all(|c| c[0] >= -1e-4 && c[1] >= -1e-4 && c[0] <= 1.0001 && c[1] <= 1.0001));
        assert!(r.stats.coverage > 0.4, "couverture {}", r.stats.coverage);
        // densité égalisée : le cube et le tore ont la même densité
        let d0 = crate::measure_density(&model.object_mesh(0), &r.corner_uv[model.corner_range(0)], 2048);
        let d1 = crate::measure_density(&model.object_mesh(1), &r.corner_uv[model.corner_range(1)], 2048);
        assert!((d0 - d1).abs() / d0 < 0.03, "{d0} vs {d1}");
    }

    #[test]
    fn subset_leaves_others() {
        let m = primitives::cube(2.0);
        let r = smart_unwrap(&m, &Params::default(), &[]);
        let (fi, n) = uv_islands(&m, &r.corner_uv);
        let mut which = vec![false; n];
        which[0] = true;
        let mut uv = r.corner_uv.clone();
        layout_islands(&m, &mut uv, &fi, Some(&which), &Params::default(), true);
        for f in 0..m.face_count() {
            if fi[f] != 0 {
                for c in m.face_range(f) {
                    assert_eq!(uv[c], r.corner_uv[c]);
                }
            }
        }
        // l'îlot rangé va dans la place libre : aucun chevauchement avec les autres
        // (la mesure compte déjà ~0,2 % sur les bords qui se touchent dans le dépliage d'origine)
        let (before, after) = (metrics::overlap_ratio(&m, &r.corner_uv, 256), metrics::overlap_ratio(&m, &uv, 256));
        assert!(after <= before + 0.001, "chevauchement {before} -> {after}");
    }

    #[test]
    fn subset_into_free_space_after_unwrap_of_one_object() {
        // cas réel : deux objets dépliés séparément se superposent ; on range l'un des deux
        let mut model = crate::model::Model::single("Tore", primitives::torus(1.0, 0.35, 32, 12));
        model.push("Cube", &primitives::cube(1.0));
        let p = Params::default();
        let all = pack_existing(&model.mesh, &crate::model::unwrap_model(&model, &p, true).corner_uv, &p, true);
        let mut uv = all.corner_uv.clone();
        // le cube est déplacé hors de la tuile, puis rangé seul : il doit trouver une place libre
        for c in model.corner_range(1) {
            uv[c][0] += 3.0;
        }
        let (fi, n) = uv_islands(&model.mesh, &uv);
        let which: Vec<bool> = (0..n).map(|i| (0..model.mesh.face_count()).any(|f| fi[f] as usize == i && f >= model.objects[1].face_start)).collect();
        layout_islands(&model.mesh, &mut uv, &fi, Some(&which), &p, true);
        assert!(uv.iter().all(|c| c[0] >= -1e-4 && c[1] >= -1e-4 && c[0] <= 1.0001 && c[1] <= 1.0001));
        assert!(metrics::overlap_ratio(&model.mesh, &uv, 256) < 0.002);
    }

    #[test]
    fn straighten_strip_with_triangles() {
        // arc de cône ouvert dont les 3 premières colonnes sont en triangles : la grille de
        // quads s'arrête avant le bout, le reste ne doit être ni écrasé ni retourné
        let (seg, rings) = (20usize, 4usize);
        let mut pos = Vec::new();
        for j in 0..=rings {
            let r = 1.0 + 0.6 * j as f64 / rings as f64;
            for i in 0..=seg {
                let a = i as f64 / seg as f64 * 3.5;
                pos.push(crate::math::V3::new(r * a.cos(), r * a.sin(), j as f64 * 0.15));
            }
        }
        let w = seg as u32 + 1;
        let mut faces = Vec::new();
        for j in 0..rings as u32 {
            for i in 0..seg as u32 {
                let (a, b, c, d) = (j * w + i, j * w + i + 1, (j + 1) * w + i + 1, (j + 1) * w + i);
                if i < 3 {
                    faces.push(vec![a, b, c]);
                    faces.push(vec![a, c, d]);
                } else {
                    faces.push(vec![a, b, c, d]);
                }
            }
        }
        let m = Mesh::from_faces(pos, &faces);
        let mut p = Params::default();
        p.shape = crate::regions::ShapePreset::Cylindrical;
        let r = smart_unwrap(&m, &p, &[]);
        let (fi, n) = uv_islands(&m, &r.corner_uv);
        assert_eq!(n, 1);
        let mut uv = r.corner_uv.clone();
        assert_eq!(straighten_grid(&m, &mut uv, &fi, &[true]), (1, 0));
        // chaque rangée de sommets est sur une même horizontale
        let mut rows: Vec<Vec<f32>> = vec![Vec::new(); rings + 1];
        for f in 0..m.face_count() {
            for c in m.face_range(f) {
                rows[m.corners[c] as usize / w as usize].push(uv[c][1]);
            }
        }
        let height = uv.iter().map(|c| c[1]).fold(f32::MIN, f32::max) - uv.iter().map(|c| c[1]).fold(f32::MAX, f32::min);
        for (j, row) in rows.iter().enumerate() {
            let spread = row.iter().fold(f32::MIN, |a, &b| a.max(b)) - row.iter().fold(f32::MAX, |a, &b| a.min(b));
            assert!(spread < 0.05 * height, "rangée {j} pas droite : écart {spread} pour une hauteur {height}");
        }
    }

    #[test]
    fn straighten_curved_strip() {
        // paroi d'un cône tronqué : son dépliage est un arc ; redressé, c'est un rectangle
        let (seg, rings) = (24usize, 3usize);
        let mut pos = Vec::new();
        for j in 0..=rings {
            let r = 1.0 + 0.5 * j as f64 / rings as f64;
            for i in 0..seg {
                let a = i as f64 / seg as f64 * std::f64::consts::TAU;
                pos.push(crate::math::V3::new(r * a.cos(), r * a.sin(), j as f64 * 0.3));
            }
        }
        let mut faces = Vec::new();
        for j in 0..rings {
            for i in 0..seg {
                let (a, b) = ((j * seg + i) as u32, (j * seg + (i + 1) % seg) as u32);
                faces.push(vec![a, b, b + seg as u32, a + seg as u32]);
            }
        }
        let m = Mesh::from_faces(pos, &faces);
        let mut p = Params::default();
        p.shape = crate::regions::ShapePreset::Cylindrical;
        let r = smart_unwrap(&m, &p, &[]);
        let (fi, n) = uv_islands(&m, &r.corner_uv);
        assert_eq!(n, 1, "une seule bande");
        let mut uv = r.corner_uv.clone();
        let (done, skipped) = straighten_grid(&m, &mut uv, &fi, &vec![true; n]);
        assert_eq!((done, skipped), (1, 0));
        // toutes les UV tombent sur une grille alignée : peu de valeurs distinctes de v
        let mut vs: Vec<i64> = uv.iter().map(|c| (c[1] * 1e4).round() as i64).collect();
        vs.sort();
        vs.dedup();
        assert!(vs.len() <= rings + 1, "{} rangées distinctes", vs.len());
    }
}
