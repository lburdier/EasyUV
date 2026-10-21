//! easyuv-cli : dépliage en ligne de commande (traitement par lots, intégration pipeline).
//!
//!   easyuv-cli modele.fbx                      -> modele_uv.obj
//!   easyuv-cli a.glb b.obj -o sortie.glb       -> une scène, rangement commun
//!   easyuv-cli dossier\ --format fbx           -> chaque fichier du dossier
//!   easyuv-cli x.obj --density 512 --tex 2048  -> densité cible (px/m)
//!   easyuv-cli --check-update

#[path = "../io.rs"]
mod io;
#[path = "../update.rs"]
#[allow(dead_code)]
mod update;

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
  --angle <deg>            angle de couture (défaut : 66)
  --distortion <x>         tolérance de distorsion (défaut : 0.12)
  --relax <n>              itérations ARAP (défaut : 4)
  --margin <x>             marge entre îlots (défaut : 0.008)
  --quality <0-3>          qualité du solveur (défaut : 1)
  --check-update           vérifie la dernière release GitHub
  --self-update            installe la dernière release
  -h, --help               cette aide";

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
            "--angle" => p.angle_deg = num(args.next(), "--angle")?,
            "--distortion" => p.max_distortion = num(args.next(), "--distortion")?,
            "--relax" => p.relax_iterations = num(args.next(), "--relax")? as u32,
            "--margin" => p.margin = num(args.next(), "--margin")?,
            "--quality" => p.quality = (num(args.next(), "--quality")? as u32).min(3),
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
                        let exe = update::install(&r)?;
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
    if inputs.is_empty() {
        println!("{HELP}");
        return Err("aucune entrée".into());
    }
    if !["obj", "glb", "fbx"].contains(&format.as_str()) {
        return Err(format!("format inconnu : {format}"));
    }
    // dossiers -> fichiers pris en charge
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
    if let Some(out) = output {
        let mut model = Model::new();
        for f in &files {
            model.append(&io::import(f).map_err(|e| format!("{} : {e}", f.display()))?);
        }
        process(&model, &p, together, &out)?;
    } else {
        let mut failed = 0;
        for f in &files {
            let out = f.with_file_name(format!("{}_uv.{format}", f.file_stem().unwrap_or_default().to_string_lossy()));
            match io::import(f).and_then(|m| process(&m, &p, together, &out)) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{} : {e}", f.display());
                    failed += 1;
                }
            }
        }
        if failed > 0 {
            return Err(format!("{failed} fichier(s) en échec"));
        }
    }
    Ok(())
}

fn process(model: &Model, p: &Params, together: bool, out: &Path) -> Result<(), String> {
    let r = unwrap_model(model, p, together);
    io::export(out, model, Some(&r.corner_uv))?;
    let s = &r.stats;
    println!(
        "{} : {} objet(s), {} faces -> {} îlots, distorsion {:.3}, {:.0} px/m{}, {:.0} ms",
        out.display(),
        model.objects.len(),
        model.mesh.face_count(),
        s.charts,
        s.mean_distortion,
        s.density_px_per_unit,
        if s.overflow { " (déborde de 0-1)" } else { "" },
        s.time_ms
    );
    Ok(())
}
