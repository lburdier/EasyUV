# Génère des modèles de test dans tests/models :
#   blender --background --factory-startup --python tests/make_models.py
import math
import os

import bpy

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "models")
os.makedirs(out, exist_ok=True)


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def save_all(name):
    base = os.path.join(out, name)
    bpy.ops.export_scene.fbx(filepath=base + ".fbx", object_types={"MESH"})
    bpy.ops.export_scene.gltf(filepath=base + ".glb", export_format="GLB")
    bpy.ops.wm.obj_export(filepath=base + ".obj", export_materials=False)


# 1. Suzanne subdivisée (organique, n-gones et quads)
reset()
bpy.ops.mesh.primitive_monkey_add()
m = bpy.context.object.modifiers.new("sub", "SUBSURF")
m.levels = 1
bpy.ops.object.modifier_apply(modifier="sub")
save_all("suzanne")

# 2. Scène multi-objets avec transformations (échelle, rotation) : teste l'espace monde
reset()
bpy.ops.mesh.primitive_cube_add(location=(0, 0, 0))
bpy.context.object.name = "Caisse"
bpy.ops.mesh.primitive_cylinder_add(vertices=24, location=(3, 0, 0), rotation=(math.radians(90), 0, 0))
bpy.context.object.name = "Tuyau"
bpy.context.object.scale = (0.5, 0.5, 2.0)
bpy.ops.mesh.primitive_torus_add(location=(-3, 0, 0))
bpy.context.object.name = "Anneau"
bpy.ops.mesh.primitive_uv_sphere_add(location=(0, 3, 0), scale=(2, 2, 2))
bpy.context.object.name = "Boule"
save_all("props")

print("modeles ecrits dans", out)
