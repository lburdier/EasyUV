# Corpus d'entraînement varié (tests/corpus/*.obj) pour l'auto-apprentissage :
#   blender --background --factory-startup --python tests/make_corpus.py
import math
import os
import random

import bpy

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "corpus")
os.makedirs(out, exist_ok=True)

PRIMS = [
    ("cube", lambda: bpy.ops.mesh.primitive_cube_add()),
    ("cylindre", lambda: bpy.ops.mesh.primitive_cylinder_add(vertices=random.choice([8, 16, 32]))),
    ("cone", lambda: bpy.ops.mesh.primitive_cone_add(vertices=random.choice([12, 24]))),
    ("sphere", lambda: bpy.ops.mesh.primitive_uv_sphere_add(segments=random.choice([16, 32]), ring_count=random.choice([8, 16]))),
    ("ico", lambda: bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=random.choice([2, 3]))),
    ("tore", lambda: bpy.ops.mesh.primitive_torus_add()),
    ("suzanne", lambda: bpy.ops.mesh.primitive_monkey_add()),
    ("grille", lambda: bpy.ops.mesh.primitive_grid_add(x_subdivisions=12, y_subdivisions=12)),
]


def modify(obj):
    """Applique 1 à 3 modificateurs aléatoires pour varier courbure, plis et topologie."""
    ops = random.sample(["subsurf", "bevel", "twist", "bend", "displace", "solidify", "taper"], k=random.randint(1, 3))
    for op in ops:
        if op == "subsurf":
            m = obj.modifiers.new(op, "SUBSURF")
            m.levels = random.choice([1, 2])
        elif op == "bevel":
            m = obj.modifiers.new(op, "BEVEL")
            m.width = random.uniform(0.03, 0.2)
            m.segments = random.choice([1, 2, 3])
        elif op in ("twist", "bend", "taper"):
            m = obj.modifiers.new(op, "SIMPLE_DEFORM")
            m.deform_method = op.upper()
            m.angle = math.radians(random.uniform(30, 120)) if op != "taper" else 0
            if op == "taper":
                m.factor = random.uniform(-0.8, 0.8)
            if len(obj.data.polygons) < 200:
                s = obj.modifiers.new("sub_" + op, "SUBSURF")
                s.levels = 2
                s.subdivision_type = "SIMPLE"
                obj.modifiers.move(len(obj.modifiers) - 1, len(obj.modifiers) - 2)
        elif op == "displace":
            if len(obj.data.polygons) < 300:
                s = obj.modifiers.new("sub_d", "SUBSURF")
                s.levels = 2
            m = obj.modifiers.new(op, "DISPLACE")
            tex = bpy.data.textures.new("n%d" % random.randint(0, 1 << 30), "CLOUDS")
            tex.noise_scale = random.uniform(0.3, 1.0)
            m.texture = tex
            m.strength = random.uniform(0.05, 0.3)
        elif op == "solidify":
            m = obj.modifiers.new(op, "SOLIDIFY")
            m.thickness = random.uniform(0.05, 0.2)
    for m in list(obj.modifiers):
        bpy.ops.object.modifier_apply(modifier=m.name)


random.seed(7)
for i in range(40):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    name, add = PRIMS[i % len(PRIMS)]
    add()
    obj = bpy.context.object
    obj.scale = [random.uniform(0.5, 2.0) for _ in range(3)]
    bpy.ops.object.transform_apply(scale=True)
    modify(obj)
    path = os.path.join(out, "%02d_%s.obj" % (i, name))
    bpy.ops.wm.obj_export(filepath=path, export_materials=False, export_uv=False, export_normals=False)
print("corpus :", len(os.listdir(out)), "fichiers dans", out)
