//! Export glTF 2.0 binaire (.glb) : un nœud + un maillage par objet, avec UV (TEXCOORD_0)
//! et normales lissées. Conversion Z-up (interne) -> Y-up (glTF).

use crate::math::V3;
use crate::model::Model;
use std::collections::HashMap;
use std::fmt::Write as _;

fn y_up(p: V3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn write_glb(model: &Model, corner_uv: Option<&[[f32; 2]]>) -> Vec<u8> {
    let mesh = &model.mesh;
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new(); // (offset, longueur, cible)
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();

    // normales lissées par sommet
    let mut vn = vec![V3::ZERO; mesh.positions.len()];
    for f in 0..mesh.face_count() {
        let n = mesh.face_newell(f);
        for &v in mesh.face(f) {
            vn[v as usize] = vn[v as usize] + n;
        }
    }

    let mut push_view = |bin: &mut Vec<u8>, data: &[u8], target: u32| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.push((bin.len(), data.len(), target));
        bin.extend_from_slice(data);
        views.len() - 1
    };

    for (oi, o) in model.objects.iter().enumerate() {
        // sommets glTF = paires (sommet, uv) uniques
        let mut key_map: HashMap<(u32, u32, u32), u32> = HashMap::new();
        let (mut pos, mut nor, mut tex): (Vec<f32>, Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new(), Vec::new());
        let mut idx: Vec<u32> = Vec::new();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for f in o.face_start..o.face_end {
            let r = mesh.face_range(f);
            let mut corner_ids = Vec::with_capacity(r.len());
            for c in r.clone() {
                let v = mesh.corners[c];
                let uv = corner_uv.map(|u| u[c]).unwrap_or([0.0, 0.0]);
                let key = (v, uv[0].to_bits(), uv[1].to_bits());
                let id = *key_map.entry(key).or_insert_with(|| {
                    let p = y_up(mesh.positions[v as usize]);
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                    pos.extend_from_slice(&p);
                    nor.extend_from_slice(&y_up(vn[v as usize].normalized()));
                    tex.extend_from_slice(&[uv[0], 1.0 - uv[1]]);
                    (pos.len() / 3 - 1) as u32
                });
                corner_ids.push(id);
            }
            for t in mesh.fan(f) {
                idx.extend_from_slice(&[corner_ids[t[0]], corner_ids[t[1]], corner_ids[t[2]]]);
            }
        }
        if idx.is_empty() {
            continue;
        }
        let nverts = pos.len() / 3;
        let as_bytes = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let vp = push_view(&mut bin, &as_bytes(&pos), 34962);
        let vnr = push_view(&mut bin, &as_bytes(&nor), 34962);
        let vt = push_view(&mut bin, &as_bytes(&tex), 34962);
        let vi = push_view(&mut bin, &idx.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(), 34963);
        let a0 = accessors.len();
        accessors.push(format!(
            "{{\"bufferView\":{vp},\"componentType\":5126,\"count\":{nverts},\"type\":\"VEC3\",\"min\":[{},{},{}],\"max\":[{},{},{}]}}",
            lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
        ));
        accessors.push(format!("{{\"bufferView\":{vnr},\"componentType\":5126,\"count\":{nverts},\"type\":\"VEC3\"}}"));
        accessors.push(format!("{{\"bufferView\":{vt},\"componentType\":5126,\"count\":{nverts},\"type\":\"VEC2\"}}"));
        accessors.push(format!("{{\"bufferView\":{vi},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}", idx.len()));
        meshes.push(format!(
            "{{\"name\":{},\"primitives\":[{{\"attributes\":{{\"POSITION\":{},\"NORMAL\":{},\"TEXCOORD_0\":{}}},\"indices\":{}}}]}}",
            json_str(&o.name),
            a0,
            a0 + 1,
            a0 + 2,
            a0 + 3
        ));
        nodes.push(format!("{{\"name\":{},\"mesh\":{}}}", json_str(&o.name), meshes.len() - 1));
        let _ = oi;
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let views_json: Vec<String> = views
        .iter()
        .map(|(o, l, t)| format!("{{\"buffer\":0,\"byteOffset\":{o},\"byteLength\":{l},\"target\":{t}}}"))
        .collect();
    let node_ids: Vec<String> = (0..nodes.len()).map(|i| i.to_string()).collect();
    let mut json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"EasyUV\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[{}]}}],\"nodes\":[{}],\"meshes\":[{}],\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]}}",
        node_ids.join(","),
        nodes.join(","),
        meshes.join(","),
        accessors.join(","),
        views_json.join(","),
        bin.len()
    );
    while json.len() % 4 != 0 {
        json.push(' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json.as_bytes());
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}
