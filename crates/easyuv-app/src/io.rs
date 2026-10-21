//! Import / export de scènes : OBJ, glTF/GLB, FBX.
//! Toute la géométrie est ramenée en espace monde, Z vers le haut, en mètres
//! (indispensable pour une densité de texels exprimée en px/m).

use easyuv::math::V3;
use easyuv::mesh::Mesh;
use easyuv::model::{tris_to_quads, weld, Model};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const IMPORT_EXT: &[&str] = &["obj", "gltf", "glb", "fbx"];

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Objet".into())
}

pub fn import(path: &Path) -> Result<Model, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let model = match ext.as_str() {
        "obj" => {
            let txt = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            easyuv::obj::parse_obj_model(&txt, &stem(path))?
        }
        "gltf" | "glb" => import_gltf(path)?,
        "fbx" => import_fbx(path)?,
        _ => return Err(format!("format non pris en charge : .{ext}")),
    };
    if model.mesh.face_count() == 0 {
        return Err("le fichier ne contient aucun maillage".into());
    }
    Ok(model)
}

/// Nettoie un maillage importé : soudure des sommets dupliqués et reconstruction des quads.
fn cleanup(mesh: Mesh, rebuild_quads: bool) -> Mesh {
    let w = weld(&mesh);
    if rebuild_quads {
        tris_to_quads(&w)
    } else {
        w
    }
}

fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // matrices colonne-major (convention glTF) : r[c][l]
    let mut r = [[0.0f32; 4]; 4];
    for c in 0..4 {
        for l in 0..4 {
            r[c][l] = (0..4).map(|k| a[k][l] * b[c][k]).sum();
        }
    }
    r
}

fn import_gltf(path: &Path) -> Result<Model, String> {
    let (doc, buffers, _) = gltf::import(path).map_err(|e| format!("glTF : {e}"))?;
    let mut model = Model::new();
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let scene = doc.default_scene().or_else(|| doc.scenes().next()).ok_or("glTF sans scène")?;
    let mut stack: Vec<(gltf::Node, [[f32; 4]; 4])> = scene.nodes().map(|n| (n, identity)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = mat_mul(&parent, &node.transform().matrix());
        for child in node.children() {
            stack.push((child, world));
        }
        let Some(gmesh) = node.mesh() else { continue };
        let mut m = Mesh::new();
        for prim in gmesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| Some(&buffers[b.index()]));
            let Some(pos) = reader.read_positions() else { continue };
            let base = m.positions.len() as u32;
            for p in pos {
                let w = [
                    world[0][0] * p[0] + world[1][0] * p[1] + world[2][0] * p[2] + world[3][0],
                    world[0][1] * p[0] + world[1][1] * p[1] + world[2][1] * p[2] + world[3][1],
                    world[0][2] * p[0] + world[1][2] * p[1] + world[2][2] * p[2] + world[3][2],
                ];
                // glTF : Y-up -> Z-up
                m.positions.push(V3::new(w[0] as f64, -(w[2] as f64), w[1] as f64));
            }
            let count = m.positions.len() as u32 - base;
            let idx: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..count).collect(),
            };
            for t in idx.chunks_exact(3) {
                m.add_face(&[base + t[0], base + t[1], base + t[2]]);
            }
        }
        if m.face_count() > 0 {
            let name = node.name().or(gmesh.name()).map(str::to_string).unwrap_or_else(|| format!("{}.{:03}", stem(path), model.objects.len()));
            model.push(name, &cleanup(m, true));
        }
    }
    Ok(model)
}

fn import_fbx(path: &Path) -> Result<Model, String> {
    let opts = ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes {
            right: ufbx::CoordinateAxis::PositiveX,
            up: ufbx::CoordinateAxis::PositiveZ,
            front: ufbx::CoordinateAxis::NegativeY,
        },
        target_unit_meters: 1.0,
        space_conversion: ufbx::SpaceConversion::ModifyGeometry,
        ..Default::default()
    };
    let scene = ufbx::load_file(&path.to_string_lossy(), opts).map_err(|e| format!("FBX : {}", AsRef::<str>::as_ref(&e.description)))?;
    let mut model = Model::new();
    for node in scene.nodes.iter() {
        let Some(fm) = node.mesh.as_ref() else { continue };
        let mut m = Mesh::new();
        for v in fm.vertices.iter() {
            let p = ufbx::transform_position(&node.geometry_to_world, *v);
            m.positions.push(V3::new(p.x, p.y, p.z));
        }
        // si la transformation inverse l'orientation (échelle négative), on retourne les faces
        let g = &node.geometry_to_world;
        let det = g.m00 * (g.m11 * g.m22 - g.m12 * g.m21) - g.m01 * (g.m10 * g.m22 - g.m12 * g.m20) + g.m02 * (g.m10 * g.m21 - g.m11 * g.m20);
        for f in fm.faces.iter() {
            if f.num_indices < 3 {
                continue;
            }
            let mut face: Vec<u32> = (f.index_begin..f.index_begin + f.num_indices).map(|i| fm.vertex_indices[i as usize]).collect();
            if det < 0.0 {
                face.reverse();
            }
            m.add_face(&face);
        }
        if m.face_count() > 0 {
            let name = node.element.name.to_string();
            let name = if name.is_empty() { format!("{}.{:03}", stem(path), model.objects.len()) } else { name };
            model.push(name, &cleanup(m, false));
        }
    }
    Ok(model)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

pub fn export(path: &Path, model: &Model, uv: Option<&[[f32; 2]]>) -> Result<(), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "obj" => std::fs::write(path, easyuv::obj::write_obj_model(model, uv)).map_err(|e| e.to_string()),
        "glb" => std::fs::write(path, easyuv::glb::write_glb(model, uv)).map_err(|e| e.to_string()),
        "fbx" => export_fbx(path, model, uv),
        _ => Err(format!("format d'export non pris en charge : .{ext}")),
    }
}

/// Blender installé le plus récent (utilisé pour écrire le FBX : pas d'écrivain FBX libre fiable en Rust).
pub fn find_blender() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("EASYUV_BLENDER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for root in [r"C:\Program Files\Blender Foundation", r"C:\Program Files (x86)\Blender Foundation"] {
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let exe = e.path().join("blender.exe");
                if exe.is_file() {
                    found.push(exe);
                }
            }
        }
    }
    // tri par numéro de version dans le nom du dossier (« Blender 5.2 »)
    found.sort_by_key(|p| {
        let n = p.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        n.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>()
    });
    found.pop()
}

const FBX_SCRIPT: &str = r#"
import bpy, sys
args = sys.argv[sys.argv.index("--") + 1:]
src, dst = args[0], args[1]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.wm.obj_import(filepath=src, forward_axis="NEGATIVE_Z", up_axis="Y")
bpy.ops.export_scene.fbx(filepath=dst, use_selection=False, object_types={"MESH"}, mesh_smooth_type="FACE", path_mode="AUTO")
"#;

fn export_fbx(path: &Path, model: &Model, uv: Option<&[[f32; 2]]>) -> Result<(), String> {
    let blender = find_blender().ok_or("L'export FBX passe par Blender, introuvable. Installez Blender ou définissez EASYUV_BLENDER.")?;
    let dir = std::env::temp_dir().join(format!("easyuv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let obj = dir.join("scene.obj");
    let script = dir.join("to_fbx.py");
    std::fs::write(&obj, easyuv::obj::write_obj_model(model, uv)).map_err(|e| e.to_string())?;
    std::fs::write(&script, FBX_SCRIPT).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&blender);
    cmd.args(["--background", "--factory-startup", "--python"]).arg(&script).arg("--").arg(&obj).arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("lancement de Blender : {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    if !path.is_file() {
        let log = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Blender n'a pas produit le FBX.\n{}", log.lines().rev().take(6).collect::<Vec<_>>().join("\n")));
    }
    Ok(())
}
