//! easyuv-cli : dépliage en ligne de commande (traitement par lots, intégration pipeline).
//!
//!   easyuv-cli modele.fbx                      -> modele_uv.obj
//!   easyuv-cli a.glb b.obj -o sortie.glb       -> une scène, rangement commun
//!   easyuv-cli dossier\ --format fbx --auto    -> chaque fichier, réglages appris
//!   easyuv-cli --train corpus\ --rounds 3      -> entraîne le modèle
//!   easyuv-cli --eval autres\                  -> mesure le gain sur des maillages jamais vus

#[path = "../io.rs"]
mod io;
#[path = "../update.rs"]
#[allow(dead_code)]
mod update;

use easyuv::learn::{self, unwrap_learned, Learner};
use easyuv::model::{unwrap_model, Model};
use easyuv::{DensityMode, Params};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "easyuv-cli - dépliage UV intelligent

USAGE : easyuv-cli [options] <fichier|dossier>...

  -o, --output <fichier>   fusionne toutes les entrées dans une scène et l'écrit ici
  --format <obj|glb|fbx>   format de sortie (défaut : obj)
  --separate               une tuile UV par objet (défaut : rangement commun)
  --density <px/m>         densité de texels cible (défaut : remplir 0-1)
  --tex <px>               taille de texture (défaut : 2048)
  --shape <forme>          auto | hard | cyl | organic (défaut : auto, détecté par morceau)
  --angle <deg>            angle de couture (défaut : 66)
  --distortion <x>         tolérance de distorsion (défaut : 0.12)
  --relax <n>              itérations ARAP (défaut : 4)
  --margin <x>             marge entre îlots (défaut : 0.008)
  --quality <0-3>          qualité du solveur (défaut : 1)

Auto-apprentissage (modèle partagé avec l'application et Blender) :
  --auto                   réglages appris + essais ; chaque dépliage enrichit le modèle
  --trials <n>             essais par dépliage avec --auto/--train (défaut : 6)
  --train                  entraîne sur les entrées sans rien écrire (avec --rounds)
  --rounds <n>             passes d'entraînement (défaut : 3)
  --eval                   compare réglages prédits et réglages par défaut (n'apprend pas)
  --learn-file <chemin>    fichier du modèle (défaut : %APPDATA%\\EasyUV\\apprentissage.txt)
  --priority <0-1>         0 = moins de coutures, 1 = moins de distorsion (défaut : 0.5)
  --no-base                ignore le modèle de base livré avec EasyUV
  --export-model <chemin>  écrit le modèle complet (base + local)
  --reset-learning         efface l'expérience locale

  --check-update           vérifie la dernière release GitHub
  --self-update            met à jour EasyUV.exe (placé à côté de easyuv-cli)
  -h, --help               cette aide";

#[derive(PartialEq)]
enum Mode {
    Process,
    Train,
    Eval,
    Bench,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut output: Option<PathBuf> = None;
    let mut format = "obj".to_string();
    let mut together = true;
    let mut p = Params::default();
    let mut auto = false;
    let mut trials = 6usize;
    let mut rounds = 3usize;
    let mut mode = Mode::Process;
    let mut learn_file = learn::default_path();
    let mut use_base = true;
    let mut export_model: Option<PathBuf> = None;
    let mut priority = 0.5f64;
    let num = |v: Option<String>, name: &str| -> Result<f64, String> { v.ok_or(format!("{name} : valeur manquante"))?.parse::<f64>().map_err(|e| format!("{name} : {e}")) };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-o" | "--output" => output = Some(PathBuf::from(args.next().ok_or("--output : chemin manquant")?)),
            "--format" => format = args.next().ok_or("--format : valeur manquante")?.to_lowercase(),
            "--separate" => together = false,
            "--density" => p.density = DensityMode::Target(num(args.next(), "--density")?),
            "--tex" => p.texture_size = num(args.next(), "--tex")? as u32,
            "--shape" => {
                let v = args.next().ok_or("--shape : valeur manquante")?;
                p.shape = easyuv::regions::ShapePreset::parse(&v).ok_or(format!("--shape : forme inconnue « {v} » (auto, hard, cyl, organic)"))?;
            }
            "--angle" => p.angle_deg = num(args.next(), "--angle")?,
            "--distortion" => p.max_distortion = num(args.next(), "--distortion")?,
            "--relax" => p.relax_iterations = num(args.next(), "--relax")? as u32,
            "--margin" => p.margin = num(args.next(), "--margin")?,
            "--quality" => p.quality = (num(args.next(), "--quality")? as u32).min(3),
            "--auto" => auto = true,
            "--trials" => trials = (num(args.next(), "--trials")? as usize).clamp(2, 64),
            "--rounds" => rounds = (num(args.next(), "--rounds")? as usize).max(1),
            "--train" => mode = Mode::Train,
            "--eval" => mode = Mode::Eval,
            "--bench" => mode = Mode::Bench,
            "--learn-file" => learn_file = PathBuf::from(args.next().ok_or("--learn-file : chemin manquant")?),
            "--no-base" => use_base = false,
            "--priority" => priority = num(args.next(), "--priority")?.clamp(0.0, 1.0),
            "--export-model" => export_model = Some(PathBuf::from(args.next().ok_or("--export-model : chemin manquant")?)),
            "--features" => {
                // diagnostic : empreinte de chaque maillage et distances au plus proche
                let files = expand(args.map(PathBuf::from).collect())?;
                let fs: Vec<(String, [f64; learn::NF])> = files
                    .iter()
                    .filter_map(|f| io::import(f).ok().map(|m| (f.file_name().unwrap_or_default().to_string_lossy().to_string(), learn::features(&m.mesh))))
                    .collect();
                for (i, (n, f)) in fs.iter().enumerate() {
                    let nearest = fs.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, (m, g))| (f.iter().zip(g).map(|(a, b)| (a - b) * (a - b)).sum::<f64>(), m)).min_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    let fv: Vec<String> = f.iter().map(|v| format!("{v:.3}")).collect();
                    println!("{n:<24} [{}]  plus proche : {} (d²={:.4})", fv.join(" "), nearest.map(|x| x.1.as_str()).unwrap_or("-"), nearest.map(|x| x.0).unwrap_or(0.0));
                }
                return Ok(());
            }
            "--reset-learning" => {
                let _ = std::fs::remove_file(&learn_file);
                println!("expérience locale effacée ({})", learn_file.display());
                return Ok(());
            }
            "--check-update" => {
                println!("version installée : {}", update::current_version());
                match update::check()? {
                    Some(r) => println!("nouvelle version disponible : {} ({})", r.version, r.page),
                    None => println!("à jour"),
                }
                return Ok(());
            }
            "--self-update" => {
                match update::check()? {
                    Some(r) => {
                        println!("installation de {}…", r.version);
                        let target = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("EasyUV.exe");
                        let exe = update::install_to(&r, Some(&target))?;
                        println!("installé : {}", exe.display());
                    }
                    None => println!("déjà à jour ({})", update::current_version()),
                }
                return Ok(());
            }
            s if s.starts_with('-') => return Err(format!("option inconnue : {s}\n\n{HELP}")),
            s => inputs.push(PathBuf::from(s)),
        }
    }
    let load = || {
        let mut l = if use_base { Learner::load(&learn_file) } else { Learner::load_local(&learn_file) };
        l.priority = priority;
        l
    };
    if inputs.is_empty() {
        if let Some(path) = export_model {
            std::fs::write(&path, load().serialize_all()).map_err(|e| e.to_string())?;
            println!("modèle écrit : {}", path.display());
            return Ok(());
        }
        println!("{HELP}");
        return Err("aucune entrée".into());
    }
    if !["obj", "glb", "fbx"].contains(&format.as_str()) {
        return Err(format!("format inconnu : {format}"));
    }
    let files = expand(inputs)?;

    match mode {
        Mode::Train => {
            let mut l = load();
            println!("entraînement : {} maillages, {} passes, {} essais ; {} expériences au départ", files.len(), rounds, trials, l.samples.len());
            let models: Vec<(PathBuf, Model)> = files.iter().filter_map(|f| io::import(f).ok().map(|m| (f.clone(), m))).collect();
            for r in 1..=rounds {
                let (mut best, mut pred, mut def) = (0.0, 0.0, 0.0);
                for (_, m) in &models {
                    let res = unwrap_learned(m, &p, together, &mut l, trials);
                    best += res.score;
                    pred += res.predicted_score;
                    def += res.default_score;
                }
                let n = models.len().max(1) as f64;
                println!(
                    "  passe {r} : score prédit {:+.4}  meilleur {:+.4}  défaut {:+.4}  (gain de la prédiction {:+.4})",
                    pred / n,
                    best / n,
                    def / n,
                    (pred - def) / n
                );
                l.save(&learn_file).map_err(|e| e.to_string())?;
            }
            println!("modèle enregistré : {} ({} expériences locales)", learn_file.display(), l.local_len());
        }
        Mode::Bench => {
            // banc d'essai : réglages fixes, mesures indépendantes de l'algorithme
            println!("{:<26} {:>7} {:>6} {:>9} {:>8} {:>8} {:>9} {:>8}", "maillage", "faces", "îlots", "couvert.", "distors.", "retourn.", "chevauch.", "temps");
            let (mut n, mut cov, mut dist, mut ov, mut ms, mut flips) = (0.0, 0.0, 0.0, 0.0, 0.0, 0usize);
            for f in &files {
                let Ok(m) = io::import(f) else { continue };
                let r = unwrap_model(&m, &p, together);
                let s = &r.stats;
                let o = easyuv::metrics::overlap_ratio(&m.mesh, &r.corner_uv, 1024);
                println!(
                    "{:<26} {:>7} {:>6} {:>8.1}% {:>8.3} {:>8} {:>8.2}% {:>6.0}ms",
                    f.file_name().unwrap_or_default().to_string_lossy(),
                    m.mesh.face_count(),
                    s.charts,
                    s.coverage * 100.0,
                    s.mean_distortion,
                    s.flipped,
                    o * 100.0,
                    s.time_ms
                );
                n += 1.0;
                cov += s.coverage;
                dist += s.mean_distortion;
                ov += o;
                ms += s.time_ms;
                flips += s.flipped;
            }
            if n > 0.0 {
                println!(
                    "MOYENNE : couverture {:.1}%  distorsion {:.4}  chevauchement {:.3}%  retournés {}  temps {:.0} ms",
                    cov / n * 100.0,
                    dist / n,
                    ov / n * 100.0,
                    flips,
                    ms / n
                );
            }
        }
        Mode::Eval => {
            let l = load();
            let (mut gain, mut wins, mut n) = (0.0, 0, 0);
            println!("évaluation (1 essai, sans apprendre) : {} maillages, modèle de {} expériences", files.len(), l.samples.len());
            for f in &files {
                let Ok(m) = io::import(f) else { continue };
                let mut tmp = l.clone();
                let res = unwrap_learned(&m, &p, together, &mut tmp, 1);
                let d = unwrap_model(&m, &p, together);
                let ds = learn::score_with(&d.stats, &m.mesh, priority);
                let g = res.score - ds;
                println!(
                    "  {:<28} prédit {:+.4}  défaut {:+.4}  {:+.4}   (angle {:.0}°, tol. {:.2}, ARAP {})",
                    f.file_name().unwrap_or_default().to_string_lossy(),
                    res.score,
                    ds,
                    g,
                    res.params.angle_deg,
                    res.params.max_distortion,
                    res.params.relax_iterations
                );
                gain += g;
                n += 1;
                if g > 1e-4 {
                    wins += 1;
                }
            }
            if n > 0 {
                println!("gain moyen {:+.4} ; meilleur que les défauts sur {wins}/{n} maillages", gain / n as f64);
            }
        }
        Mode::Process => {
            let mut learner = if auto { Some(load()) } else { None };
            if let Some(out) = output {
                let mut model = Model::new();
                for f in &files {
                    model.append(&io::import(f).map_err(|e| format!("{} : {e}", f.display()))?);
                }
                process(&model, &p, together, &out, learner.as_mut(), trials)?;
            } else {
                let mut failed = 0;
                for f in &files {
                    let out = f.with_file_name(format!("{}_uv.{format}", f.file_stem().unwrap_or_default().to_string_lossy()));
                    if let Err(e) = io::import(f).and_then(|m| process(&m, &p, together, &out, learner.as_mut(), trials)) {
                        eprintln!("{} : {e}", f.display());
                        failed += 1;
                    }
                }
                if failed > 0 {
                    return Err(format!("{failed} fichier(s) en échec"));
                }
            }
            if let Some(l) = learner.as_mut() {
                l.save(&learn_file).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Dossiers -> fichiers pris en charge (hors sorties `_uv`).
fn expand(inputs: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for i in inputs {
        if i.is_dir() {
            let mut v: Vec<PathBuf> = std::fs::read_dir(&i)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|f| f.extension().is_some_and(|e| io::IMPORT_EXT.contains(&e.to_string_lossy().to_lowercase().as_str())))
                .filter(|f| !f.file_stem().is_some_and(|s| s.to_string_lossy().ends_with("_uv")))
                .collect();
            v.sort();
            files.extend(v);
        } else {
            files.push(i);
        }
    }
    Ok(files)
}

fn process(model: &Model, p: &Params, together: bool, out: &Path, learner: Option<&mut Learner>, trials: usize) -> Result<(), String> {
    let (r, extra) = match learner {
        Some(l) => {
            let res = unwrap_learned(model, p, together, l, trials);
            let extra = format!(
                " [auto : angle {:.0}°, tol. {:.2}, ARAP {} ; score {:+.3} vs défaut {:+.3}]",
                res.params.angle_deg, res.params.max_distortion, res.params.relax_iterations, res.score, res.default_score
            );
            (res.result, extra)
        }
        None => (unwrap_model(model, p, together), String::new()),
    };
    io::export(out, model, Some(&r.corner_uv))?;
    let s = &r.stats;
    println!(
        "{} : {} objet(s), {} faces -> {} îlots [hard {} · cyl {} · orga {}], distorsion {:.3}, {:.0} px/m{}, {:.0} ms{extra}",
        out.display(),
        model.objects.len(),
        model.mesh.face_count(),
        s.charts,
        s.shapes[0],
        s.shapes[1],
        s.shapes[2],
        s.mean_distortion,
        s.density_px_per_unit,
        if s.overflow { " (déborde de 0-1)" } else { "" },
        s.time_ms
    );
    Ok(())
}
