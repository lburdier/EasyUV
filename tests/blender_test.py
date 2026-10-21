# Test headless de l'addon :
#   blender --background --factory-startup --python tests/blender_test.py
import os
import sys

import bpy
import numpy as np

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(here, "..", "blender_addon"))
import tempfile  # noqa: E402

# modele d'apprentissage isole pour le test
LEARN = os.path.join(tempfile.gettempdir(), "easyuv-blender-test-learn.txt")
if os.path.exists(LEARN):
    os.remove(LEARN)
os.environ["EASYUV_LEARN_FILE"] = LEARN

import easyuv  # noqa: E402

easyuv.register()
s = bpy.context.scene.easyuv

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()


def check(name, add, **kw):
    add(**kw)
    obj = bpy.context.object
    obj.scale = (2.0, 2.0, 2.0)
    s.density_mode = "FIT"
    assert bpy.ops.easyuv.unwrap() == {"FINISHED"}
    print("[%s] %s" % (name, s.last_report))
    uv = easyuv.read_uvs(obj.data.uv_layers.active, len(obj.data.loops))
    assert uv.min() >= -1e-4 and uv.max() <= 1.0001, "UV hors tuile"
    seams = sum(e.use_seam for e in obj.data.edges)
    # densité cible : 512 px/m sur texture 2048
    s.density_mode = "TARGET"
    s.target_density = 512.0
    bpy.ops.easyuv.unwrap()
    bpy.ops.easyuv.measure_density()
    print("[%s] coutures=%d densite cible 512 -> mesuree %.2f px/m" % (name, seams, s.measured))
    assert abs(s.measured - 512.0) < 1.0
    # application d'une autre densité
    s.target_density = 300.0
    bpy.ops.easyuv.set_density()
    bpy.ops.easyuv.measure_density()
    assert abs(s.measured - 300.0) < 1.0, s.measured
    obj.select_set(False)
    return obj


check("cube", bpy.ops.mesh.primitive_cube_add)
check("cylindre", bpy.ops.mesh.primitive_cylinder_add, vertices=32)
check("sphere", bpy.ops.mesh.primitive_uv_sphere_add)
monkey = check("suzanne", bpy.ops.mesh.primitive_monkey_add)

# mode édition
monkey.select_set(True)
bpy.context.view_layer.objects.active = monkey
bpy.ops.object.mode_set(mode="EDIT")
assert bpy.ops.easyuv.unwrap() == {"FINISHED"}
assert bpy.context.object.mode == "EDIT"
bpy.ops.object.mode_set(mode="OBJECT")

# damier on / off avec restauration du matériau
mat = bpy.data.materials.new("Original")
monkey.data.materials.append(mat)
bpy.ops.easyuv.checker_toggle()
assert monkey.material_slots[0].material.name == "EasyUV_Checker"
img = bpy.data.images["EasyUV_Checker"]
print("damier :", img.size[0], "px")
bpy.ops.easyuv.checker_toggle()
assert monkey.material_slots[0].material.name == "Original"

# rangement commun : deux objets d'echelles differentes -> meme densite
bpy.ops.object.select_all(action="DESELECT")
bpy.ops.mesh.primitive_cube_add(location=(10, 0, 0))
a = bpy.context.object
bpy.ops.mesh.primitive_torus_add(location=(14, 0, 0))
b = bpy.context.object
b.scale = (3.0, 3.0, 3.0)
a.select_set(True)
b.select_set(True)
s.density_mode = "FIT"
s.pack_together = True
assert bpy.ops.easyuv.unwrap() == {"FINISHED"}
da, db = easyuv.measure(a, 2048), easyuv.measure(b, 2048)
print("ensemble : cube %.1f px/m, tore %.1f px/m" % (da, db))
assert abs(da - db) / da < 0.02
uv = np.concatenate([easyuv.read_uvs(o.data.uv_layers.active, len(o.data.loops)) for o in (a, b)])
assert uv.min() >= -1e-4 and uv.max() <= 1.0001
s.pack_together = False
bpy.ops.easyuv.unwrap()
da2, db2 = easyuv.measure(a, 2048), easyuv.measure(b, 2048)
print("separe : cube %.1f px/m, tore %.1f px/m" % (da2, db2))
assert da2 > da * 1.2

# auto-apprentissage : l'experience est ecrite et le rapport la mentionne
assert s.auto_learn
assert os.path.isfile(LEARN), "fichier d'apprentissage absent"
n_lines = sum(1 for _ in open(LEARN, encoding="utf-8"))
assert "Appris" in s.last_report, s.last_report
print("apprentissage :", n_lines - 1, "experiences ;", s.last_report.split(" | ")[-1])
s.auto_learn = False
bpy.ops.easyuv.unwrap()
assert sum(1 for _ in open(LEARN, encoding="utf-8")) == n_lines, "sans auto-apprentissage, rien ne doit etre appris"

easyuv.unregister()
print("TOUS LES TESTS BLENDER OK")
