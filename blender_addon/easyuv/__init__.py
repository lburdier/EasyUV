# EasyUV - addon Blender : depliage UV intelligent via la bibliotheque Rust easyuv.dll
# SPDX-License-Identifier: GPL-3.0-or-later

bl_info = {
    "name": "EasyUV",
    "author": "Lucas",
    "version": (0, 3, 0),
    "blender": (4, 2, 0),
    "location": "Vue 3D > Barre laterale (N) > EasyUV",
    "description": "Depliage UV intelligent (Rust) : coutures auto, LSCM + ARAP, densite de texels, damier",
    "category": "UV",
}

import ctypes
import os

import bpy
import numpy as np
from bpy.props import BoolProperty, EnumProperty, FloatProperty, IntProperty, PointerProperty, StringProperty

# ---------------------------------------------------------------------------
# Liaison avec easyuv.dll
# ---------------------------------------------------------------------------


class EuvParams(ctypes.Structure):
    _fields_ = [
        ("angle_deg", ctypes.c_float),
        ("max_distortion", ctypes.c_float),
        ("relax_iterations", ctypes.c_uint32),
        ("margin", ctypes.c_float),
        ("density_mode", ctypes.c_uint32),
        ("texture_size", ctypes.c_uint32),
        ("target_density", ctypes.c_float),
        ("quality", ctypes.c_uint32),
        ("rotate_islands", ctypes.c_uint32),
        ("priority", ctypes.c_float),
        ("shape", ctypes.c_uint32),
    ]


class EuvLearnInfo(ctypes.Structure):
    _fields_ = [
        ("angle_deg", ctypes.c_float),
        ("max_distortion", ctypes.c_float),
        ("relax_iterations", ctypes.c_uint32),
        ("trials", ctypes.c_uint32),
        ("score", ctypes.c_float),
        ("default_score", ctypes.c_float),
        ("confidence", ctypes.c_float),
        ("experiences", ctypes.c_uint32),
    ]


class EuvStats(ctypes.Structure):
    _fields_ = [
        ("charts", ctypes.c_uint32),
        ("flipped", ctypes.c_uint32),
        ("seams", ctypes.c_uint32),
        ("overflow", ctypes.c_uint32),
        ("mean_distortion", ctypes.c_float),
        ("max_distortion", ctypes.c_float),
        ("density_px_per_unit", ctypes.c_float),
        ("coverage", ctypes.c_float),
        ("time_ms", ctypes.c_float),
    ]


_lib = None
_API_VERSION = 3


def lib():
    global _lib
    if _lib is None:
        path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "easyuv.dll")
        if not os.path.isfile(path):
            raise RuntimeError("easyuv.dll introuvable dans " + os.path.dirname(path))
        l = ctypes.CDLL(path)
        P = ctypes.POINTER
        f32, u32 = ctypes.c_float, ctypes.c_uint32
        l.euv_api_version.restype = u32
        l.euv_unwrap.restype = ctypes.c_int32
        l.euv_unwrap.argtypes = [
            P(f32), u32, P(u32), u32, P(u32), u32, P(u32), u32,
            P(EuvParams), P(f32), P(u32), P(u32), u32, P(u32), P(EuvStats),
        ]
        l.euv_unwrap_learned.restype = ctypes.c_int32
        l.euv_unwrap_learned.argtypes = [
            P(f32), u32, P(u32), u32, P(u32), u32, P(u32), u32,
            P(EuvParams), ctypes.c_char_p, u32,
            P(f32), P(u32), P(u32), u32, P(u32), P(EuvStats), P(EuvLearnInfo),
        ]
        l.euv_measure_density.restype = f32
        l.euv_measure_density.argtypes = [P(f32), u32, P(u32), u32, P(u32), u32, P(f32), u32]
        l.euv_checker.restype = ctypes.c_int32
        l.euv_checker.argtypes = [u32, u32, u32, P(f32)]
        if l.euv_api_version() != _API_VERSION:
            raise RuntimeError("Version de easyuv.dll incompatible")
        _lib = l
    return _lib


def _ptr(a, ctype):
    return a.ctypes.data_as(ctypes.POINTER(ctype))


# ---------------------------------------------------------------------------
# Extraction des donnees de maillage
# ---------------------------------------------------------------------------


def mesh_arrays(obj):
    """Positions en espace monde (la densite de texels tient compte de l'echelle de l'objet)."""
    me = obj.data
    nv, nf, nl = len(me.vertices), len(me.polygons), len(me.loops)
    co = np.empty(nv * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    m = np.array(obj.matrix_world, dtype=np.float64)
    co = (co @ m[:3, :3].T + m[:3, 3]).astype(np.float32)
    sizes = np.empty(nf, dtype=np.uint32)
    me.polygons.foreach_get("loop_total", sizes)
    starts = np.empty(nf, dtype=np.uint32)
    me.polygons.foreach_get("loop_start", starts)
    loop_v = np.empty(nl, dtype=np.uint32)
    me.loops.foreach_get("vertex_index", loop_v)
    # Les polygones sont en principe contigus dans l'ordre des boucles ; on reconstruit
    # l'ordre « coin » pour le cas general.
    order = np.concatenate([np.arange(s, s + n, dtype=np.int64) for s, n in zip(starts, sizes)]) if nf else np.zeros(0, np.int64)
    corners = loop_v[order]
    return np.ascontiguousarray(co.ravel()), sizes, np.ascontiguousarray(corners), order


def edge_pairs(me, attr):
    ne = len(me.edges)
    if ne == 0:
        return np.zeros(0, np.uint32)
    flags = np.zeros(ne, dtype=bool)
    try:
        me.edges.foreach_get(attr, flags)
    except (AttributeError, TypeError, RuntimeError):
        a = me.attributes.get("sharp_edge" if attr == "use_edge_sharp" else attr)
        if a is None or a.domain != "EDGE":
            return np.zeros(0, np.uint32)
        a.data.foreach_get("value", flags)
    verts = np.empty(ne * 2, dtype=np.uint32)
    me.edges.foreach_get("vertices", verts)
    return np.ascontiguousarray(verts.reshape(-1, 2)[flags].ravel())


def get_uv_layer(me, create=True):
    uvl = me.uv_layers.active
    if uvl is None and create:
        uvl = me.uv_layers.new(name="UVMap")
    return uvl


def write_uvs(uvl, uv_loops):
    try:
        uvl.uv.foreach_set("vector", uv_loops)
    except AttributeError:
        uvl.data.foreach_set("uv", uv_loops)


def read_uvs(uvl, nl):
    out = np.empty(nl * 2, dtype=np.float32)
    try:
        uvl.uv.foreach_get("vector", out)
    except AttributeError:
        uvl.data.foreach_get("uv", out)
    return out


class _ObjectMode:
    """Passe temporairement en mode objet (les donnees du maillage sont alors a jour)."""

    def __init__(self, context):
        self.context = context
        self.mode = None

    def __enter__(self):
        if self.context.mode != "OBJECT":
            self.mode = self.context.object.mode if self.context.object else None
            bpy.ops.object.mode_set(mode="OBJECT")
        return self

    def __exit__(self, *exc):
        if self.mode:
            bpy.ops.object.mode_set(mode=self.mode)
        return False


def selected_meshes(context):
    objs = [o for o in context.selected_objects if o.type == "MESH"]
    if not objs and context.object and context.object.type == "MESH":
        objs = [context.object]
    return objs


def tex_size(s):
    return int(s.texture_size)


# ---------------------------------------------------------------------------
# Proprietes
# ---------------------------------------------------------------------------

TEX_SIZES = [(str(v), "%d px" % v, "") for v in (256, 512, 1024, 2048, 4096, 8192)]


class EasyUVSettings(bpy.types.PropertyGroup):
    angle: FloatProperty(name="Angle de couture", default=66.0, min=5.0, max=89.0, subtype="NONE",
                         description="Angle diedre au-dela duquel une arete devient couture")
    max_distortion: FloatProperty(name="Tolerance distorsion", default=0.12, min=0.01, max=1.0,
                                  description="Distorsion moyenne toleree par ilot avant re-decoupage")
    relax: IntProperty(name="Relaxation ARAP", default=4, min=0, max=50,
                       description="Iterations As-Rigid-As-Possible (reduit l'etirement d'aire)")
    margin: FloatProperty(name="Marge", default=0.008, min=0.0, max=0.1, precision=3)
    quality: EnumProperty(name="Qualite", default="1", items=[
        ("0", "Rapide", ""), ("1", "Normale", ""), ("2", "Haute", ""), ("3", "Maximale", "")])
    rotate: BoolProperty(name="Pivoter les ilots", default=True)
    respect_seams: BoolProperty(name="Respecter coutures existantes", default=True,
                                description="Les aretes marquees couture sont toujours coupees")
    respect_sharp: BoolProperty(name="Couper aretes vives", default=False,
                                description="Les aretes marquees vives (sharp) sont aussi coupees")
    auto_learn: BoolProperty(name="Auto-apprentissage", default=True,
                             description="Essaie plusieurs reglages, garde le meilleur et memorise ce qui marche "
                                         "(modele partage avec l'application EasyUV)")
    shape: EnumProperty(name="Forme", default="0", items=[
        ("0", "Auto (detection)", "Detecte la forme de chaque morceau connexe"),
        ("1", "Hard-surface", "Cube, boite, piece mecanique : grandes faces planes"),
        ("2", "Cylindre / revolution", "Pot, tuyau, bouteille : bandes droites + couvercles"),
        ("3", "Organique", "Personnage, sculpture : decoupage guide par la distorsion")])
    priority: FloatProperty(name="Priorite", default=0.5, min=0.0, max=1.0, subtype="FACTOR",
                            description="0 : le moins de coutures et d'ilots possible ; 1 : le moins de distorsion possible")
    trials: IntProperty(name="Essais", default=6, min=2, max=32,
                        description="Reglages essayes en parallele a chaque depliage")
    pack_together: BoolProperty(name="Ranger ensemble", default=True,
                                description="Les objets selectionnes partagent la meme tuile UV et la meme densite de texels")
    mark_seams: BoolProperty(name="Marquer les coutures", default=True,
                             description="Ecrit les coutures calculees sur le maillage")
    density_mode: EnumProperty(name="Mode", default="FIT", items=[
        ("FIT", "Remplir 0-1", "Maximise l'occupation de la tuile UV"),
        ("TARGET", "Densite cible", "Echelle fixe en pixels par metre (coherente entre objets)")])
    texture_size: EnumProperty(name="Texture", items=TEX_SIZES, default="2048")
    target_density: FloatProperty(name="Densite cible (px/m)", default=1024.0, min=1.0, max=100000.0)
    measured: FloatProperty(name="Densite mesuree", default=0.0)
    checker_style: EnumProperty(
        name="Style",
        default="1",
        items=[
            ("1", "Couleur", "Couleur par colonne et par rangee, repere L : orientation et position"),
            ("0", "Mono", "Damier gris neutre : densite et etirement"),
            ("2", "Numerote", "Chaque case porte ses coordonnees (A1, B2...)"),
            ("3", "Cercles", "Un cercle par case : les ellipses montrent l'etirement"),
            ("4", "Fleches", "Une fleche vers +V par case : rotations et miroirs"),
            ("5", "Degrade UV", "Rouge = U, vert = V : position et coutures"),
            ("6", "Diagonales", "Lignes a 45 degres : cisaillement"),
        ],
    )
    checker_cells: IntProperty(name="Cellules", default=16, min=2, max=128)
    checker_tiling: FloatProperty(name="Repetition", default=1.0, min=0.01, max=64.0)
    last_report: StringProperty(default="")


# ---------------------------------------------------------------------------
# Operateurs
# ---------------------------------------------------------------------------


class EASYUV_OT_unwrap(bpy.types.Operator):
    bl_idname = "easyuv.unwrap"
    bl_label = "Depliage intelligent"
    bl_description = "Coutures automatiques, depliage LSCM + ARAP et empaquetage (moteur Rust)"
    bl_options = {"REGISTER", "UNDO"}

    def execute(self, context):
        s = context.scene.easyuv
        objs = selected_meshes(context)
        if not objs:
            self.report({"WARNING"}, "Aucun maillage selectionne")
            return {"CANCELLED"}
        try:
            L = lib()
        except Exception as e:  # noqa: BLE001
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        p = EuvParams(
            angle_deg=s.angle, max_distortion=s.max_distortion, relax_iterations=s.relax,
            margin=s.margin, density_mode=1 if s.density_mode == "TARGET" else 0,
            texture_size=tex_size(s), target_density=s.target_density,
            quality=int(s.quality), rotate_islands=1 if s.rotate else 0,
            priority=s.priority,
            shape=int(s.shape),
        )
        reports = []
        with _ObjectMode(context):
            items = []
            for obj in objs:
                me = obj.data
                if len(me.polygons) == 0:
                    continue
                pos, sizes, corners, order = mesh_arrays(obj)
                seams = np.zeros(0, np.uint32)
                if s.respect_seams:
                    seams = edge_pairs(me, "use_seam")
                if s.respect_sharp:
                    seams = np.concatenate([seams, edge_pairs(me, "use_edge_sharp")]).astype(np.uint32)
                items.append(dict(obj=obj, pos=pos, sizes=sizes, corners=corners, order=order, seams=seams))
            if not items:
                return {"CANCELLED"}
            groups = [items] if (s.pack_together and len(items) > 1) else [[it] for it in items]
            for group in groups:
                res = self.unwrap_group(L, group, p, s.trials if s.auto_learn else 0)
                if res is None:
                    self.report({"ERROR"}, "Echec du depliage : " + ", ".join(it["obj"].name for it in group))
                    continue
                st, uv_all, seam_pairs, info = res
                if info is not None:
                    # les reglages affiches deviennent ceux retenus par le modele
                    s.angle, s.max_distortion, s.relax = info.angle_deg, info.max_distortion, info.relax_iterations
                voff = coff = 0
                for it in group:
                    me = it["obj"].data
                    nv, nc = len(it["pos"]) // 3, len(it["corners"])
                    uv = uv_all[coff * 2:(coff + nc) * 2]
                    uv_loops = np.empty(nc * 2, dtype=np.float32)
                    uv_loops.reshape(-1, 2)[it["order"]] = uv.reshape(-1, 2)
                    write_uvs(get_uv_layer(me), uv_loops)
                    if s.mark_seams:
                        mine = seam_pairs[(seam_pairs[:, 0] >= voff) & (seam_pairs[:, 0] < voff + nv)] - voff
                        ev = np.empty(len(me.edges) * 2, dtype=np.uint32)
                        me.edges.foreach_get("vertices", ev)
                        ev = np.sort(ev.reshape(-1, 2), axis=1)
                        key = lambda a: a[:, 0].astype(np.uint64) << np.uint64(32) | a[:, 1].astype(np.uint64)
                        flags = np.isin(key(ev), key(np.sort(mine.astype(np.uint32), axis=1)))
                        me.edges.foreach_set("use_seam", flags)
                    me.update()
                    voff += nv
                    coff += nc
                names = ", ".join(it["obj"].name for it in group)
                reports.append("%s : %d ilots, %.0f px/m, distorsion %.3f, %.0f ms%s" % (
                    names, st.charts, st.density_px_per_unit, st.mean_distortion, st.time_ms,
                    " (deborde 0-1 !)" if st.overflow else ""))
                if info is not None:
                    reports.append("Appris : %.0f deg, tol. %.2f, ARAP %d | score %+.3f (%+.3f vs defaut), %d essais, %d experiences" % (
                        info.angle_deg, info.max_distortion, info.relax_iterations, info.score,
                        info.score - info.default_score, info.trials, info.experiences))
                s.measured = st.density_px_per_unit
        s.last_report = " | ".join(reports)
        if reports:
            self.report({"INFO"}, s.last_report)
        return {"FINISHED"}

    @staticmethod
    def unwrap_group(L, group, p, trials=0):
        """Concatène les objets (indices de sommets décalés) et les déplie en un seul appel :
        leurs îlots partagent alors la même tuile UV et la même densité de texels."""
        voff = 0
        pos, sizes, corners, seams = [], [], [], []
        for it in group:
            pos.append(it["pos"])
            sizes.append(it["sizes"])
            corners.append(it["corners"] + np.uint32(voff))
            seams.append(it["seams"] + np.uint32(voff))
            voff += len(it["pos"]) // 3
        pos = np.ascontiguousarray(np.concatenate(pos).astype(np.float32))
        sizes = np.ascontiguousarray(np.concatenate(sizes).astype(np.uint32))
        corners = np.ascontiguousarray(np.concatenate(corners).astype(np.uint32))
        seams = np.ascontiguousarray(np.concatenate(seams).astype(np.uint32))
        nc, nf = len(corners), len(sizes)
        out_uv = np.zeros(nc * 2, dtype=np.float32)
        out_chart = np.zeros(nf, dtype=np.uint32)
        cap = nc  # nombre d'aretes <= nombre de coins
        out_seams = np.zeros(max(cap, 1) * 2, dtype=np.uint32)
        n_out = ctypes.c_uint32(0)
        st = EuvStats()
        common = (
            _ptr(pos, ctypes.c_float), len(pos) // 3, _ptr(sizes, ctypes.c_uint32), nf,
            _ptr(corners, ctypes.c_uint32), nc,
            _ptr(seams, ctypes.c_uint32) if len(seams) else None, len(seams) // 2,
            ctypes.byref(p),
        )
        outs = (_ptr(out_uv, ctypes.c_float), _ptr(out_chart, ctypes.c_uint32),
                _ptr(out_seams, ctypes.c_uint32), cap, ctypes.byref(n_out), ctypes.byref(st))
        info = None
        if trials > 0:
            info = EuvLearnInfo()
            path = os.environ.get("EASYUV_LEARN_FILE", "").encode("utf-8") or None
            rc = L.euv_unwrap_learned(*common, path, trials, *outs, ctypes.byref(info))
        else:
            rc = L.euv_unwrap(*common, *outs)
        if rc != 0:
            return None
        n = min(n_out.value, cap)
        return st, out_uv, out_seams[: n * 2].reshape(-1, 2).astype(np.int64), info

class EASYUV_OT_measure(bpy.types.Operator):
    bl_idname = "easyuv.measure_density"
    bl_label = "Mesurer la densite"
    bl_description = "Densite de texels moyenne des UV actives (px/m)"

    def execute(self, context):
        s = context.scene.easyuv
        obj = context.object
        if not obj or obj.type != "MESH":
            return {"CANCELLED"}
        with _ObjectMode(context):
            d = measure(obj, tex_size(s))
        if d is None:
            self.report({"WARNING"}, "Pas de carte UV")
            return {"CANCELLED"}
        s.measured = d
        self.report({"INFO"}, "%s : %.1f px/m" % (obj.name, d))
        return {"FINISHED"}


def measure(obj, size):
    me = obj.data
    uvl = me.uv_layers.active
    if uvl is None:
        return None
    pos, sizes, corners, order = mesh_arrays(obj)
    uv = read_uvs(uvl, len(me.loops)).reshape(-1, 2)[order]
    uv = np.ascontiguousarray(uv.ravel())
    return float(lib().euv_measure_density(
        _ptr(pos, ctypes.c_float), len(pos) // 3, _ptr(sizes, ctypes.c_uint32), len(sizes),
        _ptr(corners, ctypes.c_uint32), len(corners), _ptr(uv, ctypes.c_float), size))


class EASYUV_OT_set_density(bpy.types.Operator):
    bl_idname = "easyuv.set_density"
    bl_label = "Appliquer la densite cible"
    bl_description = "Met a l'echelle les UV de chaque objet selectionne pour atteindre la densite cible"
    bl_options = {"REGISTER", "UNDO"}

    def execute(self, context):
        s = context.scene.easyuv
        done = 0
        with _ObjectMode(context):
            for obj in selected_meshes(context):
                d = measure(obj, tex_size(s))
                if not d:
                    continue
                me = obj.data
                uvl = me.uv_layers.active
                uv = read_uvs(uvl, len(me.loops)).reshape(-1, 2)
                lo = uv.min(axis=0)
                uv = (uv - lo) * (s.target_density / d) + lo
                write_uvs(uvl, np.ascontiguousarray(uv.ravel().astype(np.float32)))
                me.update()
                done += 1
        s.measured = s.target_density
        self.report({"INFO"}, "Densite %.0f px/m appliquee a %d objet(s)" % (s.target_density, done))
        return {"FINISHED"}


class EASYUV_OT_pick_density(bpy.types.Operator):
    bl_idname = "easyuv.pick_density"
    bl_label = "Prendre la densite de l'actif"
    bl_description = "Copie la densite de l'objet actif dans la densite cible"

    def execute(self, context):
        s = context.scene.easyuv
        if bpy.ops.easyuv.measure_density() == {"FINISHED"}:
            s.target_density = round(s.measured)
        return {"FINISHED"}


CHECKER_IMG = "EasyUV_Checker"
CHECKER_MAT = "EasyUV_Checker"


def build_checker(s):
    size = min(tex_size(s), 2048)
    img = bpy.data.images.get(CHECKER_IMG)
    if img is None or img.size[0] != size:
        if img:
            bpy.data.images.remove(img)
        img = bpy.data.images.new(CHECKER_IMG, size, size, alpha=False)
    px = np.empty(size * size * 4, dtype=np.float32)
    lib().euv_checker(size, s.checker_cells, int(s.checker_style), _ptr(px, ctypes.c_float))
    img.pixels.foreach_set(px)
    img.update()
    mat = bpy.data.materials.get(CHECKER_MAT) or bpy.data.materials.new(CHECKER_MAT)
    if hasattr(mat, "use_nodes") and not mat.use_nodes:
        mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    tex = nt.nodes.new("ShaderNodeTexImage")
    mapping = nt.nodes.new("ShaderNodeMapping")
    coord = nt.nodes.new("ShaderNodeTexCoord")
    tex.image = img
    tex.interpolation = "Linear"
    mapping.inputs["Scale"].default_value = (s.checker_tiling, s.checker_tiling, 1.0)
    out.location, bsdf.location, tex.location, mapping.location, coord.location = (400, 0), (100, 0), (-250, 0), (-450, 0), (-650, 0)
    nt.links.new(coord.outputs["UV"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], tex.inputs["Vector"])
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    nt.nodes.active = tex
    return mat


class EASYUV_OT_checker(bpy.types.Operator):
    bl_idname = "easyuv.checker_toggle"
    bl_label = "Damier on/off"
    bl_description = "Affiche / retire le damier sur les objets selectionnes (les materiaux d'origine sont restaures)"
    bl_options = {"REGISTER", "UNDO"}

    def execute(self, context):
        s = context.scene.easyuv
        objs = selected_meshes(context)
        if not objs:
            return {"CANCELLED"}
        enable = not any("easyuv_orig_mats" in o for o in objs)
        mat = build_checker(s) if enable else None
        for o in objs:
            slots = o.material_slots
            if enable:
                o["easyuv_orig_mats"] = [sl.material.name if sl.material else "" for sl in slots]
                if len(slots) == 0:
                    o.data.materials.append(mat)
                else:
                    for sl in slots:
                        sl.material = mat
            elif "easyuv_orig_mats" in o:
                names = list(o["easyuv_orig_mats"])
                if names:
                    for sl, n in zip(slots, names):
                        sl.material = bpy.data.materials.get(n) if n else None
                else:
                    o.data.materials.clear()
                del o["easyuv_orig_mats"]
        if enable:
            for area in context.screen.areas if context.screen else []:
                if area.type == "VIEW_3D":
                    sh = area.spaces.active.shading
                    if sh.type == "SOLID":
                        sh.color_type = "TEXTURE"
        return {"FINISHED"}


class EASYUV_OT_checker_refresh(bpy.types.Operator):
    bl_idname = "easyuv.checker_refresh"
    bl_label = "Regenerer le damier"
    bl_description = "Applique style, cellules et repetition au damier"

    def execute(self, context):
        build_checker(context.scene.easyuv)
        return {"FINISHED"}


# ---------------------------------------------------------------------------
# Interface
# ---------------------------------------------------------------------------


def draw_ui(layout, context):
    s = context.scene.easyuv
    layout.prop(s, "shape")
    col = layout.column(align=True)
    col.prop(s, "angle")
    col.prop(s, "max_distortion")
    col.prop(s, "relax")
    col.prop(s, "margin")
    layout.prop(s, "quality")
    col = layout.column(heading="Options")
    col.prop(s, "rotate")
    col.prop(s, "pack_together")
    col.prop(s, "auto_learn")
    if s.auto_learn:
        col.prop(s, "trials")
        row = col.row()
        row.prop(s, "priority", text="Coutures <> Distorsion")
    col.prop(s, "respect_seams")
    col.prop(s, "respect_sharp")
    col.prop(s, "mark_seams")
    row = layout.row()
    row.scale_y = 1.5
    row.operator(EASYUV_OT_unwrap.bl_idname, icon="UV")
    if s.last_report:
        box = layout.box()
        for part in s.last_report.split(" | ")[:4]:
            box.label(text=part)


class EASYUV_PT_main(bpy.types.Panel):
    bl_label = "Depliage intelligent"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "EasyUV"

    def draw(self, context):
        draw_ui(self.layout, context)


class EASYUV_PT_density(bpy.types.Panel):
    bl_label = "Densite de texels"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "EasyUV"

    def draw(self, context):
        s = context.scene.easyuv
        l = self.layout
        l.prop(s, "texture_size")
        l.prop(s, "density_mode", expand=True)
        l.prop(s, "target_density")
        row = l.row(align=True)
        for v in (256, 512, 1024, 2048):
            op = row.operator("wm.context_set_float", text=str(v))
            op.data_path = "scene.easyuv.target_density"
            op.value = v
        box = l.box()
        box.label(text="Mesuree : %.1f px/m (%.2f px/cm)" % (s.measured, s.measured / 100.0), icon="TEXTURE")
        row = l.row(align=True)
        row.operator(EASYUV_OT_measure.bl_idname, icon="DRIVER_DISTANCE")
        row.operator(EASYUV_OT_pick_density.bl_idname, text="", icon="EYEDROPPER")
        l.operator(EASYUV_OT_set_density.bl_idname, icon="FULLSCREEN_ENTER")


class EASYUV_PT_checker(bpy.types.Panel):
    bl_label = "Damier"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "EasyUV"

    def draw(self, context):
        s = context.scene.easyuv
        l = self.layout
        l.prop(s, "checker_style")
        l.prop(s, "checker_cells")
        l.prop(s, "checker_tiling")
        cell = tex_size(s) / (s.checker_cells * s.checker_tiling)
        l.label(text="1 case = %.0f px de texture" % cell)
        row = l.row(align=True)
        row.scale_y = 1.3
        row.operator(EASYUV_OT_checker.bl_idname, icon="TEXTURE")
        row.operator(EASYUV_OT_checker_refresh.bl_idname, text="", icon="FILE_REFRESH")


class EASYUV_PT_uv_editor(bpy.types.Panel):
    bl_label = "EasyUV"
    bl_space_type = "IMAGE_EDITOR"
    bl_region_type = "UI"
    bl_category = "EasyUV"

    def draw(self, context):
        draw_ui(self.layout, context)


def menu_func(self, context):
    self.layout.separator()
    self.layout.operator(EASYUV_OT_unwrap.bl_idname, icon="UV")


classes = (
    EasyUVSettings,
    EASYUV_OT_unwrap,
    EASYUV_OT_measure,
    EASYUV_OT_set_density,
    EASYUV_OT_pick_density,
    EASYUV_OT_checker,
    EASYUV_OT_checker_refresh,
    EASYUV_PT_main,
    EASYUV_PT_density,
    EASYUV_PT_checker,
    EASYUV_PT_uv_editor,
)

addon_keymaps = []


def register():
    for c in classes:
        bpy.utils.register_class(c)
    bpy.types.Scene.easyuv = PointerProperty(type=EasyUVSettings)
    bpy.types.VIEW3D_MT_uv_map.append(menu_func)
    wm = bpy.context.window_manager
    kc = wm.keyconfigs.addon if wm else None
    if kc:
        km = kc.keymaps.new(name="Mesh", space_type="EMPTY")
        kmi = km.keymap_items.new(EASYUV_OT_unwrap.bl_idname, "U", "PRESS", alt=True)
        addon_keymaps.append((km, kmi))


def unregister():
    for km, kmi in addon_keymaps:
        km.keymap_items.remove(kmi)
    addon_keymaps.clear()
    bpy.types.VIEW3D_MT_uv_map.remove(menu_func)
    del bpy.types.Scene.easyuv
    for c in reversed(classes):
        bpy.utils.unregister_class(c)


if __name__ == "__main__":
    register()
