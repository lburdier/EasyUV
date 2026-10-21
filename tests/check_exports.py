# Vérifie des exports EasyUV dans Blender (densité par objet, UV présentes).
#   blender --background --factory-startup --python tests/check_exports.py -- fichier [px/m attendus] [texture]
import sys

import bmesh
import bpy

args = sys.argv[sys.argv.index("--") + 1:]
path = args[0]
expected = float(args[1]) if len(args) > 1 else None
tex = float(args[2]) if len(args) > 2 else 2048.0

bpy.ops.wm.read_factory_settings(use_empty=True)
if path.lower().endswith(".fbx"):
    bpy.ops.import_scene.fbx(filepath=path)
elif path.lower().endswith((".glb", ".gltf")):
    bpy.ops.import_scene.gltf(filepath=path)
else:
    bpy.ops.wm.obj_import(filepath=path)

ok = True
for obj in [o for o in bpy.context.scene.objects if o.type == "MESH"]:
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.transform(obj.matrix_world)
    uvl = bm.loops.layers.uv.active
    if uvl is None:
        print("ECHEC", obj.name, ": pas d'UV")
        ok = False
        continue
    a3 = sum(f.calc_area() for f in bm.faces)
    au = 0.0
    outside = 0
    for f in bm.faces:
        uv = [l[uvl].uv for l in f.loops]
        for k in range(1, len(uv) - 1):
            au += abs((uv[k] - uv[0]).cross(uv[k + 1] - uv[0])) * 0.5
        outside += sum(1 for p in uv if p.x < -1e-3 or p.y < -1e-3 or p.x > 1.001 or p.y > 1.001)
    d = (au / a3) ** 0.5 * tex if a3 > 0 else 0
    msg = "%-12s faces=%5d densite=%7.1f px/m  coins hors 0-1=%d" % (obj.name, len(bm.faces), d, outside)
    if expected is not None and abs(d - expected) > expected * 0.01:
        msg += "  <-- ECHEC (attendu %.0f)" % expected
        ok = False
    print(msg)
    bm.free()
print("RESULTAT", "OK" if ok else "ECHEC")
