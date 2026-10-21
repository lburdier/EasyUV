//! Auto-apprentissage des réglages de dépliage.
//!
//! Chaque dépliage est une expérience. On décrit le maillage par une empreinte invariante
//! d'échelle, on essaie plusieurs réglages (angle de couture, tolérance de distorsion,
//! itérations ARAP) et on mémorise, pour chacun, l'écart de ses mesures de qualité
//! (distorsion, coutures, îlots, remplissage, retournements) par rapport aux réglages d'usine
//! sur le même maillage.
//!
//! Pour un nouveau maillage, un modèle de substitution à noyau estime le gain de chaque réglage
//! déjà observé sur des maillages similaires, et propose le meilleur. Les mesures étant stockées
//! brutes, le compromis « coutures ↔ distorsion » peut changer sans réentraîner.
//!
//! Le modèle est un fichier texte en ajout seul, partagé par l'application, le CLI et Blender.

use crate::mesh::{Mesh, Topology};
use crate::model::{unwrap_model, Model};
use crate::{Params, Stats, UvResult};
use std::path::{Path, PathBuf};

/// Nombre de composantes de l'empreinte d'un maillage.
pub const NF: usize = 9;
/// Réglages appris : angle de couture, tolérance de distorsion, itérations ARAP.
pub const NP: usize = 3;
/// Mesures de qualité mémorisées (voir [`components`]).
pub const NC: usize = 6;
const MAX_SAMPLES: usize = 20_000;
const FORMAT: &str = "easyuv-learn v3";

/// Modèle de base livré avec EasyUV (entraîné sur un corpus de maillages variés).
const BASE_MODEL: &str = include_str!("base_model.txt");

#[derive(Clone, Debug)]
pub struct Sample {
    pub features: [f64; NF],
    pub params: [f64; NP],
    /// Mesures de ce réglage moins celles des réglages d'usine sur le même maillage
    /// (NaN si les réglages d'usine n'ont pas été essayés).
    pub delta: [f64; NC],
}

#[derive(Clone, Debug)]
pub struct Learner {
    pub samples: Vec<Sample>,
    /// Compromis : 0 = le moins de coutures/îlots possible, 1 = le moins de distorsion possible.
    pub priority: f64,
    /// Nombre d'échantillons venant du modèle de base (jamais réécrits sur disque).
    base_len: usize,
    /// Expériences pas encore écrites sur disque.
    pending: Vec<Sample>,
}

impl Default for Learner {
    fn default() -> Self {
        Learner { samples: Vec::new(), priority: 0.5, base_len: 0, pending: Vec::new() }
    }
}

/// Résultat d'un dépliage appris.
pub struct LearnedUnwrap {
    pub result: UvResult,
    pub params: Params,
    pub score: f64,
    /// Score obtenu avec les réglages d'usine (pour mesurer le gain).
    pub default_score: f64,
    /// Score des réglages prédits par le modèle, avant exploration.
    pub predicted_score: f64,
    pub trials: usize,
    /// Confiance du modèle pour ce maillage (≈ nombre d'expériences similaires).
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Empreinte du maillage
// ---------------------------------------------------------------------------

/// Empreinte invariante d'échelle, chaque composante ≈ dans [0, 1].
pub fn features(mesh: &Mesh) -> [f64; NF] {
    let topo = Topology::build(mesh);
    let nf = mesh.face_count().max(1) as f64;
    let ne = topo.edges.len().max(1) as f64;
    let (mut sharp30, mut sharp60, mut boundary, mut dih_sum, mut len_sum) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for e in &topo.edges {
        if e.faces.len() != 2 {
            boundary += 1.0;
            continue;
        }
        let d = e.dihedral.to_degrees();
        if d > 30.0 {
            sharp30 += e.length;
        }
        if d > 60.0 {
            sharp60 += e.length;
        }
        dih_sum += e.dihedral * e.length;
        len_sum += e.length;
    }
    let len_sum = f64::max(len_sum, 1e-12);
    let tris = (0..mesh.face_count()).filter(|&f| mesh.face(f).len() == 3).count() as f64;
    let area = mesh.surface_area().max(1e-12);
    let (lo, hi) = mesh.bounds();
    let mut ext = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
    ext.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let ext0 = ext[0].max(1e-12);
    // χ / F : 0 pour une grille ouverte, positif pour des objets fermés morcelés
    let euler = mesh.positions.len() as f64 - ne + nf;
    [
        (nf.ln() / 12.0).min(1.5),                        // taille
        tris / nf,                                         // part de triangles
        sharp30 / len_sum,                                 // arêtes > 30°
        sharp60 / len_sum,                                 // arêtes > 60°
        (dih_sum / len_sum) / std::f64::consts::FRAC_PI_2, // courbure moyenne
        boundary / ne,                                     // bords ouverts
        (euler / nf * 20.0).clamp(-1.0, 1.0),              // topologie
        ext[2] / ext0,                                     // aplatissement
        (area / (ext0 * ext0) / 6.0).min(1.5),             // compacité
    ]
}

// ---------------------------------------------------------------------------
// Qualité d'un dépliage
// ---------------------------------------------------------------------------

/// Mesures de qualité : distorsion moyenne, distorsion max, longueur des coutures, log du
/// nombre d'îlots, remplissage de la tuile, part de triangles retournés.
pub fn components(s: &Stats, mesh: &Mesh) -> [f64; NC] {
    let tris = mesh.triangle_count().max(1) as f64;
    [s.mean_distortion, s.max_distortion.min(1.0), s.seam_length, (1.0 + s.charts as f64).ln(), s.coverage.min(1.0), s.flipped as f64 / tris]
}

/// Poids des mesures pour une priorité donnée (0 = coutures, 1 = distorsion).
pub fn weights(priority: f64) -> [f64; NC] {
    let p = priority.clamp(0.0, 1.0);
    [-(1.5 + 5.0 * p), -(0.2 + 0.6 * p), -(0.02 + 0.10 * (1.0 - p)), -(0.01 + 0.07 * (1.0 - p)), 0.8, -20.0]
}

fn dot(w: &[f64; NC], c: &[f64; NC]) -> f64 {
    w.iter().zip(c).map(|(a, b)| a * b).sum()
}

/// Note un dépliage (plus haut = meilleur) selon la priorité de l'artiste.
pub fn score_with(s: &Stats, mesh: &Mesh, priority: f64) -> f64 {
    dot(&weights(priority), &components(s, mesh))
}

/// Score avec la priorité équilibrée par défaut.
pub fn score(s: &Stats, mesh: &Mesh) -> f64 {
    score_with(s, mesh, 0.5)
}

// ---------------------------------------------------------------------------
// Réglages <-> espace normalisé [0, 1]
// ---------------------------------------------------------------------------

const ANGLE: (f64, f64) = (30.0, 89.0);
const DIST: (f64, f64) = (0.03, 0.40); // échelle log
const RELAX: (f64, f64) = (0.0, 12.0);

pub fn encode(p: &Params) -> [f64; NP] {
    [
        ((p.angle_deg - ANGLE.0) / (ANGLE.1 - ANGLE.0)).clamp(0.0, 1.0),
        ((p.max_distortion.max(1e-6).ln() - DIST.0.ln()) / (DIST.1.ln() - DIST.0.ln())).clamp(0.0, 1.0),
        ((p.relax_iterations as f64 - RELAX.0) / (RELAX.1 - RELAX.0)).clamp(0.0, 1.0),
    ]
}

pub fn decode(x: &[f64; NP], base: &Params) -> Params {
    let mut p = base.clone();
    let c = |v: f64| v.clamp(0.0, 1.0);
    p.angle_deg = ANGLE.0 + c(x[0]) * (ANGLE.1 - ANGLE.0);
    p.max_distortion = (DIST.0.ln() + c(x[1]) * (DIST.1.ln() - DIST.0.ln())).exp();
    p.relax_iterations = (RELAX.0 + c(x[2]) * (RELAX.1 - RELAX.0)).round() as u32;
    p
}

/// Petit générateur pseudo-aléatoire déterministe (xorshift).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.next().max(1e-12), self.next());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

fn dist2(a: &[f64; NF], b: &[f64; NF]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

fn dist_p(a: &[f64; NP], b: &[f64; NP]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum()
}

// ---------------------------------------------------------------------------
// Modèle
// ---------------------------------------------------------------------------

impl Learner {
    /// Modèle vide (sans le modèle de base).
    pub fn empty() -> Self {
        Learner::default()
    }

    /// Modèle de base seul.
    pub fn base() -> Self {
        let samples = parse(BASE_MODEL);
        Learner { base_len: samples.len(), samples, ..Default::default() }
    }

    /// Modèle de base + expérience locale enregistrée dans `path`.
    pub fn load(path: &Path) -> Self {
        let mut l = Learner::base();
        if let Ok(txt) = std::fs::read_to_string(path) {
            l.samples.extend(parse(&txt));
        }
        l
    }

    /// Expérience locale seule, sans le modèle de base.
    pub fn load_local(path: &Path) -> Self {
        let samples = std::fs::read_to_string(path).map(|t| parse(&t)).unwrap_or_default();
        Learner { samples, ..Default::default() }
    }

    /// Ajoute les nouvelles expériences au fichier. L'écriture se fait en ajout seul : l'application,
    /// le CLI et Blender peuvent partager le même fichier sans écraser l'expérience des autres.
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        if self.pending.is_empty() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // un fichier d'un ancien format est remplacé (ses mesures ne sont pas compatibles)
        let compatible = std::fs::read_to_string(path).map(|t| t.starts_with(FORMAT)).unwrap_or(false);
        let mut txt = serialize(&self.pending);
        let mut file = if compatible {
            txt = txt.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
            std::fs::OpenOptions::new().append(true).open(path)?
        } else {
            std::fs::File::create(path)?
        };
        file.write_all(txt.as_bytes())?;
        drop(file);
        self.pending.clear();
        // compaction occasionnelle : on garde les MAX_SAMPLES expériences les plus récentes
        if let Ok(all) = std::fs::read_to_string(path) {
            let lines = all.lines().count();
            if lines > MAX_SAMPLES + MAX_SAMPLES / 5 {
                let keep: Vec<&str> = all.lines().skip(lines - MAX_SAMPLES).collect();
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, format!("{FORMAT}\n{}\n", keep.join("\n")))?;
                std::fs::rename(tmp, path)?;
            }
        }
        Ok(())
    }

    /// Exporte tout le modèle (base comprise), par ex. pour régénérer le modèle de base.
    pub fn serialize_all(&self) -> String {
        serialize(&self.samples)
    }

    /// Nombre d'expériences locales (hors modèle de base).
    pub fn local_len(&self) -> usize {
        self.samples.len().saturating_sub(self.base_len)
    }

    pub fn record(&mut self, s: Sample) {
        self.pending.push(s.clone());
        self.samples.push(s);
        if self.samples.len() > MAX_SAMPLES + self.base_len {
            // on oublie la plus ancienne expérience locale
            self.samples.remove(self.base_len);
        }
    }

    /// Prédiction : réglages (normalisés) qui maximisent le gain estimé par rapport aux
    /// réglages d'usine, confiance (≈ nombre d'expériences sur des maillages similaires)
    /// et gain attendu.
    ///
    /// G(c) = Σ wf·wp·gain / (Σ wf·wp + λ), où wf mesure la proximité des maillages, wp celle
    /// des réglages et gain = poids(priorité) · delta. λ ramène l'estimation vers 0 (réglages
    /// d'usine) tant que les données sont rares.
    pub fn predict(&self, f: &[f64; NF]) -> ([f64; NP], f64, f64) {
        let reference = encode(&Params::default());
        let w = weights(self.priority);
        // largeur du noyau fixe : un maillage à d² = H2 pèse e^-1. Une largeur adaptative ferait
        // passer des maillages très différents pour des voisins quand le modèle est encore jeune.
        const H2: f64 = 0.02;
        let near: Vec<(f64, &Sample, f64)> = self
            .samples
            .iter()
            .filter(|s| s.delta[0].is_finite())
            .map(|s| ((-dist2(f, &s.features) / H2).exp(), s, dot(&w, &s.delta)))
            .filter(|(wf, _, _)| *wf > 0.01)
            .collect();
        let confidence: f64 = near.iter().map(|(wf, _, _)| wf).sum();
        if near.is_empty() {
            return (reference, 0.0, 0.0);
        }
        // λ : retrait vers 0 ; MIN_SUPPORT : poids minimal d'observations autour d'un réglage
        // avant d'oser s'écarter des réglages d'usine.
        const LAMBDA: f64 = 1.0;
        const MIN_SUPPORT: f64 = 0.5;
        const BW_P: f64 = 0.015;
        let gain_at = |c: &[f64; NP]| -> (f64, f64) {
            let (mut num, mut den) = (0.0, 0.0);
            for (wf, s, g) in &near {
                let dp: f64 = c.iter().zip(&s.params).map(|(a, b)| (a - b) * (a - b)).sum();
                let ww = wf * (-dp / BW_P).exp();
                num += ww * g;
                den += ww;
            }
            (num / (den + LAMBDA), den)
        };
        // candidats : réglages observés chez les voisins les plus proches
        let mut order: Vec<usize> = (0..near.len()).collect();
        order.sort_by(|&a, &b| near[b].0.partial_cmp(&near[a].0).unwrap());
        let mut best = (reference, 0.0);
        for &i in order.iter().take(400) {
            let c = near[i].1.params;
            let (g, support) = gain_at(&c);
            if support >= MIN_SUPPORT && g > best.1 {
                best = (c, g);
            }
        }
        (best.0, confidence, best.1)
    }

    /// Réglages candidats : prédiction, réglages d'usine (référence du gain), réglages de
    /// l'utilisateur, puis explorations autour de la prédiction.
    pub fn candidates(&self, f: &[f64; NF], user: &Params, trials: usize, seed: u64) -> (Vec<[f64; NP]>, f64) {
        let (pred, conf, _) = self.predict(f);
        let mut c = vec![pred];
        for extra in [encode(&Params::default()), encode(user)] {
            if c.len() < trials && c.iter().all(|x| dist_p(x, &extra) > 1e-3) {
                c.push(extra);
            }
        }
        let sigma = (0.25 / (1.0 + conf).sqrt()).max(0.04);
        let mut rng = Rng(seed | 1);
        while c.len() < trials.max(1) {
            let x = std::array::from_fn(|j| (pred[j] + sigma * rng.normal()).clamp(0.0, 1.0));
            c.push(x);
        }
        (c, conf)
    }
}

fn serialize(samples: &[Sample]) -> String {
    let mut s = String::from(FORMAT);
    s.push('\n');
    let fmt = |v: &[f64]| v.iter().map(|x| format!("{x:.5}")).collect::<Vec<_>>().join(" ");
    for x in samples {
        s.push_str(&format!("{} | {} | {}\n", fmt(&x.features), fmt(&x.params), fmt(&x.delta)));
    }
    s
}

fn parse(txt: &str) -> Vec<Sample> {
    if !txt.starts_with(FORMAT) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in txt.lines().skip(1) {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 3 {
            continue;
        }
        // "NaN" est accepté par parse::<f64>()
        let nums = |s: &str| s.split_whitespace().filter_map(|v| v.parse::<f64>().ok()).collect::<Vec<_>>();
        let (f, p, d) = (nums(parts[0]), nums(parts[1]), nums(parts[2]));
        if f.len() == NF && p.len() == NP && d.len() == NC {
            out.push(Sample { features: std::array::from_fn(|i| f[i]), params: std::array::from_fn(|i| p[i]), delta: std::array::from_fn(|i| d[i]) });
        }
    }
    out
}

/// Emplacement par défaut : %APPDATA%\EasyUV\apprentissage.txt (ou ~/.config/easyuv).
pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("EASYUV_LEARN_FILE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(a) = std::env::var("APPDATA") {
        return PathBuf::from(a).join("EasyUV").join("apprentissage.txt");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("easyuv").join("apprentissage.txt")
}

// ---------------------------------------------------------------------------
// Dépliage appris
// ---------------------------------------------------------------------------

/// Déplie `model` en essayant `trials` réglages (en parallèle), garde le meilleur et
/// enregistre toutes les expériences dans `learner`. Marge, densité, texture et rotation
/// de `base` sont conservées ; seuls angle, tolérance et ARAP sont appris.
pub fn unwrap_learned(model: &Model, base: &Params, together: bool, learner: &mut Learner, trials: usize) -> LearnedUnwrap {
    unwrap_learned_with_seams(model, base, together, learner, trials, &[])
}

/// Comme [`unwrap_learned`], avec des coutures imposées (paires de sommets du maillage fusionné).
pub fn unwrap_learned_with_seams(model: &Model, base: &Params, together: bool, learner: &mut Learner, trials: usize, seams: &[(u32, u32)]) -> LearnedUnwrap {
    let f = features(&model.mesh);
    // gros maillages : moins d'essais
    let faces = model.mesh.face_count();
    let trials = if faces > 200_000 { trials.min(2) } else if faces > 50_000 { trials.min(4) } else { trials }.max(1);
    let seed = (f.iter().fold(0u64, |h, v| h.wrapping_mul(31).wrapping_add((v * 1e5) as u64)) ^ learner.samples.len() as u64).wrapping_add(0x9E37_79B9);
    let (cands, confidence) = learner.candidates(&f, base, trials, seed);
    let def = encode(&Params::default());

    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(1);
    let mut results: Vec<Option<(UvResult, [f64; NC])>> = (0..cands.len()).map(|_| None).collect();
    for chunk in (0..cands.len()).collect::<Vec<_>>().chunks(threads) {
        std::thread::scope(|sc| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|&i| {
                    let p = decode(&cands[i], base);
                    let m = &model;
                    (
                        i,
                        sc.spawn(move || {
                            let r = if seams.is_empty() { unwrap_model(m, &p, together) } else { crate::smart_unwrap(&m.mesh, &p, seams) };
                            let c = components(&r.stats, &m.mesh);
                            (r, c)
                        }),
                    )
                })
                .collect();
            for (i, h) in handles {
                results[i] = h.join().ok();
            }
        });
    }
    let w = weights(learner.priority);
    // mesures des réglages d'usine sur ce maillage : référence des écarts enregistrés
    let reference = (0..cands.len()).find(|&i| dist_p(&cands[i], &def) < 1e-3).and_then(|i| results[i].as_ref().map(|r| r.1));
    let default_score = reference.map_or(f64::NAN, |c| dot(&w, &c));
    let mut best: Option<(usize, f64)> = None;
    for (i, r) in results.iter().enumerate() {
        let Some((_, c)) = r else { continue };
        let delta = match &reference {
            Some(rc) => std::array::from_fn(|k| c[k] - rc[k]),
            None => [f64::NAN; NC],
        };
        learner.record(Sample { features: f, params: cands[i], delta });
        let s = dot(&w, c);
        if best.map_or(true, |(_, b)| s > b) {
            best = Some((i, s));
        }
    }
    let predicted_score = results[0].as_ref().map_or(f64::NAN, |r| dot(&w, &r.1));
    let (bi, bs) = best.expect("au moins un essai");
    let (result, _) = results[bi].take().unwrap();
    LearnedUnwrap { result, params: decode(&cands[bi], base), score: bs, default_score, predicted_score, trials: cands.len(), confidence }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn encode_decode_roundtrip() {
        let p = Params::default();
        let q = decode(&encode(&p), &p);
        assert!((p.angle_deg - q.angle_deg).abs() < 1e-9);
        assert!((p.max_distortion - q.max_distortion).abs() < 1e-9);
        assert_eq!(p.relax_iterations, q.relax_iterations);
    }

    #[test]
    fn features_are_scale_invariant() {
        let a = features(&primitives::torus(1.0, 0.3, 32, 12));
        let b = features(&primitives::torus(10.0, 3.0, 32, 12));
        assert!(dist2(&a, &b) < 1e-9, "{a:?} {b:?}");
    }

    #[test]
    fn save_load_and_learning_improves() {
        let mut l = Learner::empty();
        let m = Model::single("blob", primitives::blob(1.0, 32, 16));
        let p = Params::default();
        let first = unwrap_learned(&m, &p, true, &mut l, 6);
        // jamais pire que les réglages d'usine (ils font partie des essais)
        assert!(first.score >= first.default_score - 1e-12);
        // déterminisme : mêmes réglages -> même score
        let r1 = unwrap_model(&m, &p, true);
        let r2 = unwrap_model(&m, &p, true);
        assert_eq!(score(&r1.stats, &m.mesh), score(&r2.stats, &m.mesh));
        // avec l'expérience acquise, un seul essai retrouve le meilleur réglage
        let again = unwrap_learned(&m, &p, true, &mut l, 1);
        assert!(again.score >= first.score - 1e-9, "{} < {}", again.score, first.score);
        assert!(again.confidence > first.confidence);
        let path = std::env::temp_dir().join(format!("easyuv-learn-test-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        l.save(&path).unwrap();
        let back = Learner::load_local(&path);
        assert_eq!(back.samples.len(), l.samples.len());
        // ajout seul : une seconde instance ajoute sans écraser
        let mut other = Learner::empty();
        unwrap_learned(&m, &p, true, &mut other, 2);
        other.save(&path).unwrap();
        assert_eq!(Learner::load_local(&path).samples.len(), l.samples.len() + other.samples.len());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn priority_changes_choice_without_retraining() {
        // deux réglages fictifs : A = moins de coutures, B = moins de distorsion
        let f = [0.5; NF];
        let mut l = Learner::empty();
        let a = [0.9, 0.9, 0.3];
        let b = [0.2, 0.1, 0.8];
        for _ in 0..5 {
            l.record(Sample { features: f, params: a, delta: [0.05, 0.05, -2.0, -0.5, 0.0, 0.0] });
            l.record(Sample { features: f, params: b, delta: [-0.05, -0.05, 2.0, 0.5, 0.0, 0.0] });
        }
        l.priority = 0.0;
        assert_eq!(l.predict(&f).0, a);
        l.priority = 1.0;
        assert_eq!(l.predict(&f).0, b);
    }
}
