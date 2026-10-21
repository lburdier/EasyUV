//! Lecture / écriture Wavefront OBJ multi-objets (polygones conservés).
//! Convention du fichier : Y vers le haut (standard OBJ, export Blender par défaut) ;
//! en mémoire : Z vers le haut, comme Blender.

use crate::math::V3;
use crate::mesh::Mesh;
use crate::model::Model;
use std::collections::HashMap;
use std::fmt::Write as _;

fn from_y_up(x: f64, y: f64, z: f64) -> V3 {
    V3::new(x, -z, y)
}
fn to_y_up(p: V3) -> (f64, f64, f64) {
    (p.x, p.z, -p.y)
}

/// Lit un OBJ ; chaque `o` (ou à défaut `g`) devient un objet.
pub fn parse_obj_model(text: &str, default_name: &str) -> Result<Model, String> {
    let mut positions: Vec<V3> = Vec::new();
    // objets : (nom, faces en indices globaux)
    let mut objects: Vec<(String, Vec<Vec<u32>>)> = vec![(default_name.to_string(), Vec::new())];
    let has_o = text.lines().any(|l| l.trim_start().starts_with("o "));
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f64> = it.take(3).map(|s| s.parse::<f64>()).collect::<Result<_, _>>().map_err(|e| format!("ligne {}: {e}", ln + 1))?;
                if c.len() < 3 {
                    return Err(format!("ligne {}: sommet incomplet", ln + 1));
                }
                positions.push(from_y_up(c[0], c[1], c[2]));
            }
            Some(tag @ ("o" | "g")) if (tag == "o") == has_o => {
                let name = it.collect::<Vec<_>>().join(" ");
                let name = if name.is_empty() { format!("{default_name}.{:03}", objects.len()) } else { name };
                if objects.last().is_some_and(|o| o.1.is_empty()) {
                    objects.last_mut().unwrap().0 = name;
                } else {
                    objects.push((name, Vec::new()));
                }
            }
            Some("f") => {
                let mut face = Vec::new();
                for tok in it {
                    let idx = tok.split('/').next().unwrap_or("");
                    let i: i64 = idx.parse().map_err(|_| format!("ligne {}: indice invalide '{tok}'", ln + 1))?;
                    let n = positions.len() as i64;
                    let r = if i < 0 { n + i } else { i - 1 };
                    if r < 0 || r >= n {
                        return Err(format!("ligne {}: indice hors limites {i}", ln + 1));
                    }
                    face.push(r as u32);
                }
                if face.len() >= 3 {
                    objects.last_mut().unwrap().1.push(face);
                }
            }
            _ => {}
        }
    }
    let mut model = Model::new();
    for (name, faces) in objects {
        if faces.is_empty() {
            continue;
        }
        // sous-ensemble de sommets propre à l'objet
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut m = Mesh::new();
        for f in &faces {
            let lf: Vec<u32> = f
                .iter()
                .map(|&v| {
                    *remap.entry(v).or_insert_with(|| {
                        m.positions.push(positions[v as usize]);
                        (m.positions.len() - 1) as u32
                    })
                })
                .collect();
            m.add_face(&lf);
        }
        model.push(name, &m);
    }
    if model.objects.is_empty() {
        return Err("aucune face trouvée".into());
    }
    Ok(model)
}

/// Compatibilité : OBJ à un seul maillage (tous les objets fusionnés).
pub fn parse_obj(text: &str) -> Result<Mesh, String> {
    parse_obj_model(text, "Objet").map(|m| m.mesh)
}

/// Écrit l'OBJ avec une coordonnée `vt` par coin de face (îlots séparés proprement).
pub fn write_obj_model(model: &Model, corner_uv: Option<&[[f32; 2]]>) -> String {
    let mesh = &model.mesh;
    let mut s = String::new();
    let _ = writeln!(s, "# EasyUV - export");
    for p in &mesh.positions {
        let (x, y, z) = to_y_up(*p);
        let _ = writeln!(s, "v {x:.6} {y:.6} {z:.6}");
    }
    if let Some(uv) = corner_uv {
        for t in uv {
            let _ = writeln!(s, "vt {:.6} {:.6}", t[0], t[1]);
        }
    }
    for o in &model.objects {
        let _ = writeln!(s, "o {}", o.name);
        for f in o.face_start..o.face_end {
            s.push('f');
            for c in mesh.face_range(f) {
                let v = mesh.corners[c] + 1;
                if corner_uv.is_some() {
                    let _ = write!(s, " {}/{}", v, c + 1);
                } else {
                    let _ = write!(s, " {v}");
                }
            }
            s.push('\n');
        }
    }
    s
}

pub fn write_obj(mesh: &Mesh, corner_uv: Option<&[[f32; 2]]>) -> String {
    write_obj_model(&Model::single("Objet", mesh.clone()), corner_uv)
}
